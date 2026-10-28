//! Sidecar lifecycle glued to the Tauri app: settings → tier assignment → bridge → `Sidecar` (spawn/health/restart),
//! status broadcast as `caret://router/status`, and reconciliation on settings changes.
#[path = "sidecar_core.rs"]
pub mod sidecar_core;

use super::assign::assign_tiers;
use super::bridge::Bridge;
use super::runtime::LynkrRuntime;
use sidecar_core::{BridgeInfo, Sidecar, StartSpec, StatusSink};
use super::{Assignment, Endpoint, RouterStatus};
use crate::core::settings::{ProviderKind, Settings, SettingsState};
use crate::error::Result;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Listener, Manager};

const INSTALL_HINT: &str = "Lynkr is not installed yet — click Install";
const SETTINGS_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);

pub struct RouterManager {
    pub app: AppHandle,
    core: Arc<Sidecar>,
    bridge: tokio::sync::Mutex<Option<Arc<Bridge>>>,
    native_ok: AtomicBool,
    flow_lock: tokio::sync::Mutex<()>,
    settings_epoch: Arc<AtomicU64>,
}

impl RouterManager {
    pub fn new(app: AppHandle) -> Arc<Self> {
        let settings = app.state::<SettingsState>().effective();
        let runtime = LynkrRuntime::for_source(&settings.router.source, resource_dir(&app));
        let sink: StatusSink = { let app = app.clone(); Arc::new(move |s: &RouterStatus| crate::events::emit(&app, crate::events::ROUTER_STATUS, s.clone())) };
        let core = Sidecar::new(runtime.clone(), sink);
        let installed = runtime.is_installed();
        core.update_status(|s| {
            s.installed = installed; s.version = runtime.installed_version();
            if !settings.router.enabled { s.state = "disabled".into(); }
            else if !installed { s.state = "not-installed".into(); s.reason = Some(if LynkrRuntime::node_bin().is_none() { LynkrRuntime::NODE_MISSING.to_string() } else { INSTALL_HINT.to_string() }); }
            else { s.state = "degraded".into(); s.reason = Some("Sidecar not started yet".into()); }
        });
        let me = Arc::new(Self { app: app.clone(), core, bridge: tokio::sync::Mutex::new(None), native_ok: AtomicBool::new(false), flow_lock: tokio::sync::Mutex::new(()), settings_epoch: Arc::new(AtomicU64::new(0)) });
        // Debounced reconciliation whenever settings change (models, providers, keys, router section).
        let weak = Arc::downgrade(&me);
        let epoch = me.settings_epoch.clone();
        app.listen(crate::events::SETTINGS_CHANGED, move |_| {
            let Some(m) = weak.upgrade() else { return };
            let n = epoch.fetch_add(1, Ordering::SeqCst) + 1;
            let epoch = epoch.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(SETTINGS_DEBOUNCE).await;
                if epoch.load(Ordering::SeqCst) == n { let _ = m.start_flow(false).await; }
            });
        });
        me
    }

    pub fn status(&self) -> RouterStatus { self.core.status() }
    /// `Some` only while the sidecar is healthy.
    pub fn endpoint(&self) -> Option<Endpoint> { self.core.endpoint() }
    pub fn logs(&self) -> Vec<String> { self.core.logs() }
    pub fn current_assignment(&self) -> Assignment { self.core.status().assignment }
    pub fn log(&self, line: impl Into<String>) { self.core.log(line) }

    /// Start (or keep) the sidecar in the background; never blocks.
    pub fn ensure_started(self: &Arc<Self>) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move { let _ = me.start_flow(false).await; });
    }

    /// Install the pinned Lynkr release into the prefix, then start.
    pub async fn install(self: &Arc<Self>) -> Result<RouterStatus> {
        let runtime = self.runtime();
        {
            let _g = self.flow_lock.lock().await;
            self.core.set_runtime(runtime.clone());
            self.core.update_status(|s| { s.state = "installing".into(); s.reason = Some(format!("Installing Lynkr {}…", super::runtime::LYNKR_VERSION)); });
            let core = self.core.clone();
            let log = move |l: String| core.log(l);
            match runtime.install(&log).await {
                Ok(rep) => { self.native_ok.store(rep.native_ok, Ordering::SeqCst); self.core.update_status(|s| { s.installed = true; s.version = Some(rep.version); s.native_ok = rep.native_ok; }); }
                Err(e) => {
                    let installed = runtime.is_installed();
                    self.core.update_status(|s| { s.installed = installed; s.state = if installed { "error" } else { "not-installed" }.into(); s.reason = Some(format!("Install failed: {}", e.message)); });
                    return Err(e);
                }
            }
        }
        // Not forced: a settings-change flow may already have started the sidecar while npm was running.
        self.start_flow(false).await
    }

    pub async fn restart(self: &Arc<Self>) -> Result<RouterStatus> { self.start_flow(true).await }

    /// Stop now (SIGTERM, SIGKILL after 3 s) and publish the resting state.
    pub fn stop(&self) {
        self.core.stop_sync();
        let settings = self.app.state::<SettingsState>().effective();
        let installed = self.runtime().is_installed();
        self.core.update_status(|s| {
            s.installed = installed;
            if !settings.router.enabled { s.state = "disabled".into(); s.reason = None; }
            else if !installed { s.state = "not-installed".into(); s.reason = Some(INSTALL_HINT.into()); }
            else { s.state = "degraded".into(); s.reason = Some("Stopped — use Restart to bring the router back".into()); }
        });
    }

    /// Settings changed: recompute the assignment, rewrite `.env` (hot reload) or restart when needed.
    pub fn on_settings_changed(self: &Arc<Self>) { self.ensure_started(); }

    fn resource_dir(&self) -> Option<PathBuf> { resource_dir(&self.app) }
    fn runtime(&self) -> LynkrRuntime { LynkrRuntime::for_source(&self.app.state::<SettingsState>().effective().router.source, self.resource_dir()) }

    /// The one reconciliation path: disabled → stop; not installed → wait for Install; incomplete assignment → degraded;
    /// otherwise make sure bridge + sidecar run with the current assignment (hot-rewrite or restart as needed).
    async fn start_flow(self: &Arc<Self>, force: bool) -> Result<RouterStatus> {
        let _g = self.flow_lock.lock().await;
        let st = self.app.state::<SettingsState>();
        let settings = st.effective();
        let runtime = LynkrRuntime::for_source(&settings.router.source, self.resource_dir());
        self.core.set_runtime(runtime.clone());
        if !settings.router.enabled {
            self.core.stop().await;
            self.core.update_status(|s| { s.state = "disabled".into(); s.reason = None; });
            return Ok(self.status());
        }
        if !runtime.is_installed() {
            self.core.stop().await;
            self.core.update_status(|s| { s.installed = false; s.version = runtime.installed_version(); s.state = "not-installed".into(); s.reason = Some(if LynkrRuntime::node_bin().is_none() { LynkrRuntime::NODE_MISSING.to_string() } else { INSTALL_HINT.to_string() }); });
            return Ok(self.status());
        }
        let assignment = assign_tiers(&settings, &|p| crate::ai::util::usable(p, &st));
        if !assignment.complete() {
            self.core.stop().await;
            self.core.update_status(|s| { s.installed = true; s.version = runtime.installed_version(); s.assignment = assignment.clone(); s.warnings = assignment.warnings.clone(); s.state = "degraded".into(); s.reason = assignment.warnings.first().cloned().or_else(|| Some("Auto routing needs at least 2 enabled models".into())); });
            return Ok(self.status());
        }
        let bridge = match self.bridge().await {
            Ok(b) => b,
            Err(e) => {
                self.core.stop().await;
                self.core.update_status(|s| { s.installed = true; s.assignment = assignment.clone(); s.warnings = assignment.warnings.clone(); s.state = "error".into(); s.reason = Some(format!("Bridge failed to start: {}", e.message)); });
                return Err(e);
            }
        };
        if !self.native_ok.load(Ordering::SeqCst) && runtime.probe_native().await { self.native_ok.store(true, Ordering::SeqCst); }
        let spec = StartSpec {
            assignment, models: settings.models.clone(), sticky_sessions: settings.router.sticky_sessions, tpm_limit: settings.router.tpm_limit,
            ollama_embeddings: ollama_embeddings(&settings), bridge: BridgeInfo { port: bridge.port, token: bridge.token.clone() },
            version: runtime.installed_version(), native_ok: self.native_ok.load(Ordering::SeqCst), lan_warning: !runtime.is_patched(), extra_env: vec![],
        };
        if !force && self.core.status().state == "healthy" {
            if let Some(prev) = self.core.spec() {
                if prev.restart_signature() == spec.restart_signature() {
                    if prev.env_signature() != spec.env_signature() { self.core.rewrite_env(&spec)?; }
                    return Ok(self.status());
                }
                self.core.log("[router] prices/context/embeddings changed — restarting the sidecar");
            }
        }
        self.core.start(spec).await
    }

    async fn bridge(&self) -> Result<Arc<Bridge>> {
        let mut g = self.bridge.lock().await;
        if let Some(b) = g.as_ref() { return Ok(b.clone()); }
        let b = Bridge::start(self.app.clone()).await?;
        self.core.log(format!("[router] bridge listening on 127.0.0.1:{}", b.port));
        *g = Some(b.clone());
        Ok(b)
    }
}

fn resource_dir(app: &AppHandle) -> Option<PathBuf> { app.path().resource_dir().ok() }

/// (base URL, model) for Lynkr's embeddings when `router.embeddingsProviderId` names an Ollama provider.
fn ollama_embeddings(settings: &Settings) -> Option<(String, String)> {
    let id = settings.router.embeddings_provider_id.as_deref()?;
    let p = settings.providers.iter().find(|p| p.id == id && p.enabled && matches!(p.kind, ProviderKind::Ollama))?;
    let base = p.base_url.clone().unwrap_or_else(|| "http://localhost:11434/v1".into());
    let model = settings.indexing.embeddings_model.clone().filter(|m| !m.trim().is_empty()).unwrap_or_else(|| "nomic-embed-text".into());
    Some((base, model))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ollama_embeddings_only_for_enabled_ollama_providers() {
        let mut s = Settings::default();
        assert!(ollama_embeddings(&s).is_none());
        s.router.embeddings_provider_id = Some("ollama".into());
        assert_eq!(ollama_embeddings(&s), Some(("http://localhost:11434/v1".into(), "nomic-embed-text".into())));
        s.indexing.embeddings_model = Some("mxbai-embed-large".into());
        assert_eq!(ollama_embeddings(&s).unwrap().1, "mxbai-embed-large");
        s.router.embeddings_provider_id = Some("openai".into());
        assert!(ollama_embeddings(&s).is_none(), "non-Ollama providers are not used for Lynkr embeddings");
    }
}

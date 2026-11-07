//! Caret as an MCP server ("orchestrator"). Any MCP client on this machine (Claude Code in a terminal, Codex, Cursor,
//! another Caret) can connect to `http://127.0.0.1:<port>/mcp` with a bearer token and spawn, steer, read and approve
//! Caret threads on any configured provider or external ACP agent. See docs/ORCHESTRATOR.md.
//!
//! - `server.rs`: the Streamable HTTP JSON-RPC transport (stateless, loopback, bearer auth).
//! - `tools.rs`: the tool surface over `AgentManager` / the ACP bridge.
//! - This file: lifecycle (settings-driven start/stop), the persisted token (`~/.caret/orchestrator/token`, 0600),
//!   the discovery file (`~/.caret/orchestrator/endpoint.json`), Tauri commands and ready-to-paste client snippets.
pub mod server;
pub mod tools;
#[cfg(test)] mod e2e;

use crate::acp::AcpState;
use crate::ai::agent::manager::AgentManager;
use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use crate::paths;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Listener, Manager, State};
use tools::Ctx;

const SETTINGS_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);

// ───────────────────────── files ─────────────────────────

/// `~/.caret/orchestrator` (honours `CARET_HOME`).
pub fn dir() -> PathBuf { paths::sub_dir("orchestrator") }
pub fn token_path() -> PathBuf { dir().join("token") }
pub fn endpoint_path() -> PathBuf { dir().join("endpoint.json") }

fn new_token() -> String { format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple()) }

fn write_private(path: &PathBuf, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)); }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// The bearer token, created on first use (mode 0600).
pub fn load_or_create_token() -> Result<String> {
    let p = token_path();
    if let Ok(t) = std::fs::read_to_string(&p) { let t = t.trim().to_string(); if t.len() >= 32 { return Ok(t); } }
    let t = new_token();
    write_private(&p, &format!("{t}\n"))?;
    Ok(t)
}

/// Replace the token on disk; connected clients must be reconfigured.
pub fn rotate_token() -> Result<String> { let t = new_token(); write_private(&token_path(), &format!("{t}\n"))?; Ok(t) }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointFile { pub url: String, pub port: u16, pub pid: u32, pub started_at: u64 }

fn write_endpoint(s: &server::Server) {
    let v = EndpointFile { url: s.url(), port: s.port, pid: std::process::id(), started_at: s.started_at };
    if let Err(e) = serde_json::to_string_pretty(&v).map_err(CaretError::from).and_then(|j| write_private(&endpoint_path(), &j)) { tracing::warn!(error = %e, "orchestrator: could not write endpoint.json"); }
}
fn remove_endpoint() { let _ = std::fs::remove_file(endpoint_path()); }

// ───────────────────────── snippets ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Snippets { pub claude_code: String, pub codex_toml: String, pub cursor_json: String, pub caret_json: String }

/// Ready-to-paste client configuration for a given endpoint.
pub fn snippets(url: &str, token: &str) -> Snippets {
    let json = serde_json::to_string_pretty(&serde_json::json!({ "mcpServers": { "caret": { "url": url, "headers": { "Authorization": format!("Bearer {token}") } } } })).unwrap_or_default();
    Snippets {
        claude_code: format!("claude mcp add --transport http caret {url} --header \"Authorization: Bearer {token}\""),
        codex_toml: format!("# ~/.codex/config.toml\n[mcp_servers.caret]\nurl = \"{url}\"\nbearer_token_env_var = \"CARET_MCP_TOKEN\"\n\n# in your shell profile (or before launching codex):\nexport CARET_MCP_TOKEN={token}"),
        cursor_json: json.clone(),
        caret_json: json,
    }
}

// ───────────────────────── state / lifecycle ─────────────────────────

#[derive(Default)]
pub struct OrchestratorState { inner: Arc<Inner> }

#[derive(Default)]
struct Inner {
    app: OnceCell<AppHandle>,
    ctx: OnceCell<Arc<Ctx>>,
    server: tokio::sync::Mutex<Option<Arc<server::Server>>>,
    epoch: AtomicU64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrchestratorStatus {
    pub enabled: bool,
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub port: Option<u16>,
    pub token_path: String,
    pub token: String,
    pub endpoint_path: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub error: Option<String>,
}

impl Inner {
    fn settings(&self) -> Option<crate::core::settings::OrchestratorSettings> { self.app.get().map(|a| a.state::<SettingsState>().effective().orchestrator) }

    /// Bring the server in line with settings: start when enabled and stopped, stop when disabled, restart on a port change.
    async fn reconcile(self: &Arc<Self>) -> Result<Option<Arc<server::Server>>> {
        let Some(cfg) = self.settings() else { return Ok(None) };
        let Some(ctx) = self.ctx.get().cloned() else { return Ok(None) };
        let mut slot = self.server.lock().await;
        if !cfg.enabled {
            if let Some(s) = slot.take() { s.shutdown(); remove_endpoint(); tracing::info!("orchestrator MCP server stopped (disabled)"); }
            return Ok(None);
        }
        if let Some(s) = slot.as_ref() { if s.port >= cfg.port && s.port <= cfg.port.saturating_add(server::PORT_ATTEMPTS) { return Ok(Some(s.clone())); } s.shutdown(); }
        let token = load_or_create_token()?;
        let s = server::Server::start(ctx, token, cfg.port).await?;
        write_endpoint(&s);
        *slot = Some(s.clone());
        Ok(Some(s))
    }

    async fn restart(self: &Arc<Self>) -> Result<Option<Arc<server::Server>>> {
        { let mut slot = self.server.lock().await; if let Some(s) = slot.take() { s.shutdown(); } }
        self.reconcile().await
    }

    async fn status(&self) -> OrchestratorStatus {
        let cfg = self.settings().unwrap_or_default();
        let server = self.server.lock().await.clone();
        let token = load_or_create_token().unwrap_or_default();
        OrchestratorStatus {
            enabled: cfg.enabled, running: server.is_some(),
            url: Some(server.as_ref().map(|s| s.url()).unwrap_or_else(|| server::mcp_url(cfg.port))), port: Some(server.as_ref().map(|s| s.port).unwrap_or(cfg.port)),
            token_path: token_path().to_string_lossy().to_string(), token, endpoint_path: endpoint_path().to_string_lossy().to_string(), error: None,
        }
    }
}

/// Create the tool context from managed state, start the server when enabled and follow settings changes.
pub fn init(app: &AppHandle) {
    let st = app.state::<OrchestratorState>();
    let inner = st.inner.clone();
    let _ = inner.app.set(app.clone());
    let mgr = app.state::<Arc<AgentManager>>().inner().clone();
    let acp = app.state::<AcpState>().inner.clone();
    let _ = inner.ctx.set(Ctx::new(mgr, acp, Some(app.clone())));
    // Debounced reconciliation on every settings change (enable toggle, port).
    let weak = Arc::downgrade(&inner);
    app.listen(crate::events::SETTINGS_CHANGED, move |_| {
        let Some(i) = weak.upgrade() else { return };
        let n = i.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(SETTINGS_DEBOUNCE).await;
            if i.epoch.load(Ordering::SeqCst) == n { if let Err(e) = i.reconcile().await { tracing::warn!(error = %e, "orchestrator: reconcile failed"); } }
        });
    });
    let i = inner.clone();
    tauri::async_runtime::spawn(async move { if let Err(e) = i.reconcile().await { tracing::warn!(error = %e, "orchestrator: start failed"); } });
}

/// Stop the server (app exit) and drop the discovery file.
pub fn shutdown(app: &AppHandle) {
    if let Some(st) = app.try_state::<OrchestratorState>() {
        if let Ok(mut slot) = st.inner.server.try_lock() { if let Some(s) = slot.take() { s.shutdown(); } }
        remove_endpoint();
    }
}

// ───────────────────────── commands ─────────────────────────

#[tauri::command]
pub async fn orchestrator_status(st: State<'_, OrchestratorState>) -> Result<OrchestratorStatus> {
    let inner = st.inner.clone();
    // Reconcile first so the status reflects the current settings even if the debounce has not fired yet.
    let err = inner.reconcile().await.err().map(|e| e.message);
    let mut s = inner.status().await; s.error = err; Ok(s)
}

#[tauri::command]
pub async fn orchestrator_rotate_token(st: State<'_, OrchestratorState>) -> Result<OrchestratorStatus> {
    let inner = st.inner.clone();
    rotate_token()?;
    let err = inner.restart().await.err().map(|e| e.message);
    let mut s = inner.status().await; s.error = err; Ok(s)
}

#[tauri::command]
pub async fn orchestrator_snippets(st: State<'_, OrchestratorState>) -> Result<Snippets> {
    let s = st.inner.status().await;
    Ok(snippets(s.url.as_deref().unwrap_or(""), &s.token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_are_ready_to_paste() {
        let s = snippets("http://127.0.0.1:17431/mcp", "tok123");
        assert_eq!(s.claude_code, "claude mcp add --transport http caret http://127.0.0.1:17431/mcp --header \"Authorization: Bearer tok123\"");
        assert!(s.codex_toml.contains("[mcp_servers.caret]\nurl = \"http://127.0.0.1:17431/mcp\"\nbearer_token_env_var = \"CARET_MCP_TOKEN\""));
        assert!(s.codex_toml.contains("export CARET_MCP_TOKEN=tok123"));
        let v: serde_json::Value = serde_json::from_str(&s.cursor_json).unwrap();
        assert_eq!(v["mcpServers"]["caret"]["url"], "http://127.0.0.1:17431/mcp");
        assert_eq!(v["mcpServers"]["caret"]["headers"]["Authorization"], "Bearer tok123");
        assert_eq!(s.cursor_json, s.caret_json);
        // Caret's own MCP client config parser understands the snippet.
        let cfgs = crate::mcp::config::parse_file(&s.caret_json, "project", None);
        assert_eq!(cfgs.len(), 1); assert_eq!(cfgs[0].url.as_deref(), Some("http://127.0.0.1:17431/mcp"));
        assert_eq!(cfgs[0].headers.as_ref().and_then(|h| h.get("Authorization")).map(String::as_str), Some("Bearer tok123"));
    }

    #[test]
    fn token_is_persisted_and_rotated() {
        let home = std::env::temp_dir().join(format!("caret-orch-token-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        // Exercise the file helpers directly against a scratch dir (CARET_HOME is process-wide and shared with other tests).
        let p = home.join("orchestrator").join("token");
        assert!(std::fs::read_to_string(&p).is_err());
        write_private(&p, "abc").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "abc");
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600); }
        let t1 = new_token(); let t2 = new_token();
        assert_eq!(t1.len(), 64); assert_ne!(t1, t2);
        let _ = std::fs::remove_dir_all(&home);
    }
}

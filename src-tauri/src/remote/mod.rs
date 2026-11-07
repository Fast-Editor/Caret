//! Remote access (`caret serve`): a browser on the LAN or over Tailscale gets the full Caret UI.
//!
//! The Mac keeps owning execution. The server (`server.rs`) serves the same Vite bundle Tauri embeds, with a shim
//! (`remote/shim.js`) that implements `window.__TAURI_INTERNALS__` over a WebSocket. Every `invoke()` from the
//! browser is relayed (`relay.rs`) to the desktop webview, where `src/lib/remoteExecutor.ts` runs the real Tauri
//! command and streams Channel / event callbacks back. Nothing is re-implemented per command.
//!
//! Access: browsers pair with a six digit code (`pairing.rs`) and keep a token (cookie + localStorage); token hashes
//! live in `~/.caret/remote/devices.json`. The executor socket uses a per-process secret handed to the desktop
//! frontend through `remote_executor_secret` and accepted from loopback only.
//!
//! Lifecycle: `Settings.remote.enabled` or the `caret serve` CLI starts the server in `init`; `remote_set_enabled`
//! / `remote_restart` reconcile it; `shutdown` stops it on app exit.
pub mod pairing;
pub mod relay;
pub mod server;

use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use crate::events;
use pairing::{DeviceInfo, DeviceStore, Pairing, PairingCode};
use parking_lot::Mutex;
use relay::Hub;
use serde::{Deserialize, Serialize};
use server::{AssetSource, Deps, RemoteServer};
use std::sync::{Arc, OnceLock, Weak};
use tauri::{AppHandle, Manager, State};

pub const STATUS_EVENT: &str = "caret://remote/status";

// ───────────────────────── `caret serve` argv ─────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServeArgs {
    pub host: Option<String>,
    pub port: Option<u16>,
    /// Hide the main window after setup (the webview keeps running hidden; it must exist for the relay).
    pub no_window: bool,
    /// Print only the URLs (one per line) instead of the URL + pairing code banner.
    pub print_url: bool,
}

static SERVE_ARGS: OnceLock<ServeArgs> = OnceLock::new();
pub fn set_serve_args(a: ServeArgs) { let _ = SERVE_ARGS.set(a); }
pub fn serve_args() -> Option<&'static ServeArgs> { SERVE_ARGS.get() }
/// The process was started as `caret serve …`.
pub fn is_serving() -> bool { SERVE_ARGS.get().is_some() }

/// Split launcher argv (without the program name) into the `serve` options and everything else.
/// `serve` must be the first argument; its options are `--host <h>`, `--port <n>`, `--no-window`, `--print-url`
/// (`--host=<h>` / `--port=<n>` forms too). Remaining arguments are returned for the open-paths parser.
pub fn strip_serve_args(args: &[String]) -> (Option<ServeArgs>, Vec<String>) {
    if args.first().map(String::as_str) != Some("serve") { return (None, args.to_vec()); }
    let mut sa = ServeArgs::default();
    let mut rest = Vec::new();
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--host" => { if let Some(h) = it.next() { sa.host = Some(h.clone()); } }
            s if s.starts_with("--host=") => sa.host = Some(s["--host=".len()..].to_string()),
            "--port" => { if let Some(p) = it.next() { sa.port = p.parse().ok(); } }
            s if s.starts_with("--port=") => sa.port = s["--port=".len()..].parse().ok(),
            "--no-window" => sa.no_window = true,
            "--print-url" => sa.print_url = true,
            other => rest.push(other.to_string()),
        }
    }
    (Some(sa), rest)
}

// ───────────────────────── status / manager ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    /// `Settings.remote.enabled` (the CLI `serve` mode counts as enabled).
    pub enabled: bool,
    pub running: bool,
    /// Started from the command line (`caret serve`).
    pub serving: bool,
    pub host: String,
    pub port: u16,
    pub urls: Vec<String>,
    pub devices: Vec<DeviceInfo>,
    pub executor_connected: bool,
    pub clients: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub error: Option<String>,
}

pub struct RemoteManager {
    app: AppHandle,
    executor_secret: String,
    pub pairing: Arc<Pairing>,
    pub devices: Arc<DeviceStore>,
    pub hub: Arc<Hub>,
    server: Mutex<Option<Arc<RemoteServer>>>,
    last_error: Mutex<Option<String>>,
    /// Serialises start/stop so two quick toggles cannot race on the port.
    lifecycle: tokio::sync::Mutex<()>,
}

#[derive(Default)]
pub struct RemoteState(OnceLock<Arc<RemoteManager>>);
impl RemoteState { pub fn get(&self) -> Option<Arc<RemoteManager>> { self.0.get().cloned() } }

fn manager(app: &AppHandle) -> Option<Arc<RemoteManager>> { app.try_state::<RemoteState>().and_then(|s| s.get()) }
fn require(st: &State<'_, RemoteState>) -> Result<Arc<RemoteManager>> { st.get().ok_or_else(|| CaretError::internal("remote access is not initialised")) }

/// Production frontend: the assets Tauri embedded (`frontendDist`), else `<repo>/dist` for `tauri dev` checkouts.
struct AppAssets { app: AppHandle }
impl AssetSource for AppAssets {
    fn get(&self, path: &str) -> Option<(Vec<u8>, String)> {
        if let Some(a) = self.app.asset_resolver().get(path.to_string()) { return Some((a.bytes, a.mime_type)); }
        let rel = path.trim_start_matches('/');
        if rel.split('/').any(|c| c == "..") { return None; }
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("dist").join(if rel.is_empty() { "index.html" } else { rel });
        let bytes = std::fs::read(&p).ok()?;
        Some((bytes, mime_guess::from_path(&p).first_or_octet_stream().to_string()))
    }
}

impl RemoteManager {
    fn new(app: AppHandle) -> Arc<Self> {
        let m = Arc::new(Self {
            app,
            executor_secret: pairing::random_token(),
            pairing: Arc::new(Pairing::default()),
            devices: Arc::new(DeviceStore::load(DeviceStore::default_path())),
            hub: Hub::new(),
            server: Mutex::new(None),
            last_error: Mutex::new(None),
            lifecycle: tokio::sync::Mutex::new(()),
        });
        let weak: Weak<RemoteManager> = Arc::downgrade(&m);
        m.hub.set_on_change(Arc::new(move || { if let Some(m) = weak.upgrade() { m.broadcast(); } }));
        m
    }

    pub fn executor_secret(&self) -> &str { &self.executor_secret }

    /// (enabled, host, port) after applying the CLI overrides to the settings.
    fn desired(&self) -> (bool, String, u16) {
        let s = self.app.state::<SettingsState>().effective().remote;
        let sa = serve_args();
        (s.enabled || sa.is_some(), sa.and_then(|a| a.host.clone()).unwrap_or(s.host), sa.and_then(|a| a.port).unwrap_or(s.port))
    }

    pub fn status(&self) -> RemoteStatus {
        let (enabled, host, port) = self.desired();
        let server = self.server.lock().clone();
        let running = server.as_ref().map(|s| s.is_running()).unwrap_or(false);
        let (host, port) = match &server { Some(s) if running => (s.host.clone(), s.port), _ => (host, port) };
        RemoteStatus {
            enabled, running, serving: is_serving(), urls: if running { server::urls_for(&host, port) } else { vec![] }, host, port,
            devices: self.devices.list(), executor_connected: self.hub.executor_connected(), clients: self.hub.client_count(), error: self.last_error.lock().clone(),
        }
    }

    fn broadcast(&self) { events::emit(&self.app, STATUS_EVENT, self.status()); }

    /// Start (or restart when host/port changed) according to settings + CLI overrides.
    pub async fn start(self: &Arc<Self>) -> Result<RemoteStatus> {
        let _g = self.lifecycle.lock().await;
        let (_, host, port) = self.desired();
        if let Some(s) = self.server.lock().clone() { if s.is_running() && s.host == host && s.port == port { return Ok(self.status()); } }
        let prev = self.server.lock().take(); // bind first: the guard must not live across the await
        if let Some(prev) = prev { prev.shutdown_and_wait().await; }
        let weak = Arc::downgrade(self);
        let deps = Deps {
            assets: Arc::new(AppAssets { app: self.app.clone() }), pairing: self.pairing.clone(), devices: self.devices.clone(), hub: self.hub.clone(),
            executor_secret: self.executor_secret.clone(), version: self.app.package_info().version.to_string(),
            on_change: Some(Arc::new(move || { if let Some(m) = weak.upgrade() { m.broadcast(); } })),
        };
        match RemoteServer::start(&host, port, deps).await {
            Ok(s) => { *self.server.lock() = Some(s); *self.last_error.lock() = None; self.broadcast(); Ok(self.status()) }
            Err(e) => { *self.last_error.lock() = Some(e.message.clone()); self.broadcast(); Err(e) }
        }
    }

    fn stop_inner(&self) { if let Some(s) = self.server.lock().take() { s.shutdown(); } }
    pub fn stop(&self) { self.stop_inner(); self.broadcast(); }

    /// Bring the server in line with the settings: start when enabled, stop otherwise.
    pub async fn reconcile(self: &Arc<Self>) -> Result<RemoteStatus> {
        let (enabled, _, _) = self.desired();
        if enabled { self.start().await } else { self.stop(); Ok(self.status()) }
    }

    fn print_banner(&self, status: &RemoteStatus) {
        use std::io::Write;
        let print_url_only = serve_args().map(|a| a.print_url).unwrap_or(false);
        let code = if print_url_only { None } else { Some(self.pairing.generate()) };
        let mut out = std::io::stdout().lock();
        for url in &status.urls {
            match &code {
                Some(c) => { let _ = writeln!(out, "Caret remote: {url}  pairing code: {} (valid 5 min)", c.code); }
                None => { let _ = writeln!(out, "{url}"); }
            }
        }
        if status.urls.is_empty() { let _ = writeln!(out, "Caret remote: listening on {}:{}", status.host, status.port); }
        let _ = out.flush();
    }
}

// ───────────────────────── app wiring ─────────────────────────

/// Called from `setup`: create the manager, start when enabled or serving, hide the window for `--no-window`.
pub fn init(app: &AppHandle) {
    let m = RemoteManager::new(app.clone());
    let _ = app.state::<RemoteState>().0.set(m.clone());
    let (enabled, host, port) = m.desired();
    if enabled {
        tracing::info!(host, port, serving = is_serving(), "remote: starting");
        let m2 = m.clone();
        tauri::async_runtime::spawn(async move {
            match m2.start().await {
                Ok(status) => { if is_serving() { m2.print_banner(&status); } }
                Err(e) => {
                    tracing::error!(error = %e, "remote: could not start");
                    events::notify(&m2.app, "error", "Remote access could not start", Some(e.message.clone()));
                    if is_serving() { eprintln!("caret serve: {}", e.message); }
                }
            }
        });
    }
    if serve_args().map(|a| a.no_window).unwrap_or(false) {
        if let Some(w) = app.get_webview_window("main") { let _ = w.hide(); }
    }
}

/// App exit: stop listening and close every relay socket.
pub fn shutdown(app: &AppHandle) { if let Some(m) = manager(app) { m.stop_inner(); } }

// ───────────────────────── commands ─────────────────────────

#[tauri::command(async)]
pub fn remote_status(st: State<'_, RemoteState>) -> Result<RemoteStatus> { Ok(require(&st)?.status()) }

/// Issue a fresh pairing code (valid 5 minutes, one use).
#[tauri::command]
pub fn remote_pairing_code(st: State<'_, RemoteState>) -> Result<PairingCode> { Ok(require(&st)?.pairing.generate()) }

#[tauri::command]
pub fn remote_revoke_device(st: State<'_, RemoteState>, id: String) -> Result<RemoteStatus> {
    let m = require(&st)?;
    if !m.devices.revoke(&id)? { return Err(CaretError::not_found(format!("device {id}"))); }
    m.hub.drop_device(&id);
    m.broadcast();
    Ok(m.status())
}

/// Per-process secret for the desktop executor socket. Never relayed to remote clients (the executor refuses it).
#[tauri::command]
pub fn remote_executor_secret(st: State<'_, RemoteState>) -> Result<String> { Ok(require(&st)?.executor_secret().to_string()) }

#[tauri::command]
pub fn remote_is_serving() -> Result<bool> { Ok(is_serving()) }

/// Persist `remote.enabled` and start/stop the server accordingly.
#[tauri::command]
pub async fn remote_set_enabled(app: AppHandle, enabled: bool) -> Result<RemoteStatus> {
    let m = manager(&app).ok_or_else(|| CaretError::internal("remote access is not initialised"))?;
    let settings = app.state::<SettingsState>();
    let eff = settings.patch(&serde_json::json!({ "remote": { "enabled": enabled } }), "user")?;
    events::emit(&app, events::SETTINGS_CHANGED, eff);
    m.reconcile().await
}

/// Restart on the current settings (after changing host or port).
#[tauri::command]
pub async fn remote_restart(app: AppHandle) -> Result<RemoteStatus> {
    let m = manager(&app).ok_or_else(|| CaretError::internal("remote access is not initialised"))?;
    m.stop();
    m.reconcile().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }

    #[test]
    fn strip_serve_args_parses_options_and_keeps_paths() {
        let (sa, rest) = strip_serve_args(&s(&["serve", "--host", "0.0.0.0", "--port", "18000", "--no-window", "--print-url", ".", "-g", "a.ts:3"]));
        assert_eq!(sa, Some(ServeArgs { host: Some("0.0.0.0".into()), port: Some(18000), no_window: true, print_url: true }));
        assert_eq!(rest, s(&[".", "-g", "a.ts:3"]));
        let (sa, rest) = strip_serve_args(&s(&["serve", "--host=127.0.0.1", "--port=abc"]));
        assert_eq!(sa, Some(ServeArgs { host: Some("127.0.0.1".into()), port: None, no_window: false, print_url: false }));
        assert!(rest.is_empty());
        let (sa, rest) = strip_serve_args(&s(&["serve"]));
        assert_eq!(sa, Some(ServeArgs::default()));
        assert!(rest.is_empty());
        // not serve: untouched
        let (sa, rest) = strip_serve_args(&s(&[".", "serve", "--host", "x"]));
        assert_eq!(sa, None);
        assert_eq!(rest, s(&[".", "serve", "--host", "x"]));
        assert_eq!(strip_serve_args(&[]).0, None);
        // a dangling --host without a value is tolerated
        assert_eq!(strip_serve_args(&s(&["serve", "--host"])).0.unwrap().host, None);
    }

    #[test]
    fn status_serialises_camel_case() {
        let st = RemoteStatus { enabled: true, running: false, serving: false, host: "127.0.0.1".into(), port: 17430, urls: vec![], devices: vec![], executor_connected: false, clients: 0, error: None };
        let v = serde_json::to_value(&st).unwrap();
        assert_eq!(v["executorConnected"], false);
        assert!(v.get("error").is_none());
        assert_eq!(v["port"], 17430);
    }
}

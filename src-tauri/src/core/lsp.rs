//! Language-server manager: spawns servers, relays JSON-RPC framing to the UI over a Channel,
//! mirrors diagnostics into `DiagnosticsStore`, and answers simple server→client requests itself.
use crate::core::diagnostics::{DiagnosticsStore, FileDiagnostics};
use crate::core::settings::{LspServerConfig, SettingsState};
use crate::error::{CaretError, Result};
use crate::events;
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Listener, Manager, State};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspServerStatus { pub server_id: String, pub config_id: String, pub name: String, pub state: String, #[serde(skip_serializing_if = "Option::is_none")] pub pid: Option<u32>, #[serde(skip_serializing_if = "Option::is_none")] pub error: Option<String>, pub languages: Vec<String>, pub root: String }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspConfigEntry { pub config: LspServerConfig, pub installed: bool }

pub struct LspServer {
    pub id: String,
    pub config: LspServerConfig,
    pub root: PathBuf,
    stdin: tokio::sync::Mutex<Option<tokio::process::ChildStdin>>,
    child: tokio::sync::Mutex<Option<tokio::process::Child>>,
    state: RwLock<String>,
    error: RwLock<Option<String>>,
    pid: Option<u32>,
    channel: RwLock<Channel<Value>>,
    stderr_tail: Mutex<VecDeque<String>>,
    internal_id: AtomicI64,
}

impl LspServer {
    fn status(&self) -> LspServerStatus {
        LspServerStatus { server_id: self.id.clone(), config_id: self.config.id.clone(), name: self.config.name.clone(), state: self.state.read().clone(), pid: self.pid, error: self.error.read().clone(), languages: self.config.languages.clone(), root: self.root.to_string_lossy().to_string() }
    }
    async fn write(&self, msg: &Value) -> Result<()> {
        let body = serde_json::to_vec(msg)?;
        let mut guard = self.stdin.lock().await;
        let Some(stdin) = guard.as_mut() else { return Err(CaretError::new("lsp_stopped", "language server is not running")) };
        stdin.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes()).await?;
        stdin.write_all(&body).await?;
        stdin.flush().await?;
        Ok(())
    }
}

#[derive(Default)]
pub struct LspState { pub servers: DashMap<String, Arc<LspServer>>, by_key: DashMap<String, String> }

impl LspState {
    pub fn statuses(&self) -> Vec<LspServerStatus> { let mut v: Vec<_> = self.servers.iter().map(|e| e.value().status()).collect(); v.sort_by(|a, b| a.name.cmp(&b.name)); v }
}

fn broadcast(app: &AppHandle) { let st = app.state::<LspState>(); events::emit(app, events::LSP_STATUS, st.statuses()); }

pub fn uri_to_path(uri: &str) -> Option<String> {
    if let Ok(u) = url::Url::parse(uri) { if u.scheme() == "file" { return u.to_file_path().ok().map(|p| p.to_string_lossy().to_string()); } }
    uri.strip_prefix("file://").map(|s| crate::core::pty::percent_decode(s))
}
pub fn path_to_uri(p: &Path) -> String { url::Url::from_file_path(p).map(|u| u.to_string()).unwrap_or_else(|_| format!("file://{}", p.display())) }

/// Read one framed JSON-RPC message. Returns None on EOF.
async fn read_message<R: tokio::io::AsyncBufRead + Unpin>(r: &mut R) -> std::io::Result<Option<Value>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = r.read_line(&mut line).await?;
        if n == 0 { return Ok(None); }
        let t = line.trim_end_matches(['\r', '\n']);
        if t.is_empty() { if content_length.is_some() { break; } else { continue; } }
        if let Some((k, v)) = t.split_once(':') { if k.eq_ignore_ascii_case("content-length") { content_length = v.trim().parse().ok(); } }
    }
    let len = content_length.unwrap_or(0);
    if len > 64 * 1024 * 1024 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "lsp message too large")); }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    Ok(Some(serde_json::from_slice(&body).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?))
}

async fn handle_server_message(app: &AppHandle, srv: &Arc<LspServer>, mut msg: Value) {
    let method = msg.get("method").and_then(|m| m.as_str()).map(String::from);
    let id = msg.get("id").cloned();
    if let Some(m) = method.as_deref() {
        let mut handled = false;
        match m {
            "textDocument/publishDiagnostics" => {
                if let Some(p) = msg.get("params") {
                    let uri = p.get("uri").and_then(|u| u.as_str()).unwrap_or("").to_string();
                    let diags: Vec<crate::core::diagnostics::Diagnostic> = p.get("diagnostics").cloned().and_then(|d| serde_json::from_value(d).ok()).unwrap_or_default();
                    if let Some(path) = uri_to_path(&uri) {
                        let fd = FileDiagnostics { uri: uri.clone(), path, diagnostics: diags, server_id: srv.id.clone(), version: p.get("version").and_then(|v| v.as_i64()) };
                        app.state::<DiagnosticsStore>().publish(fd.clone());
                        events::emit(app, events::LSP_DIAGNOSTICS, fd);
                    }
                }
            }
            "window/workDoneProgress/create" | "client/registerCapability" | "client/unregisterCapability" | "window/showMessageRequest"
            | "workspace/semanticTokens/refresh" | "workspace/inlayHint/refresh" | "workspace/codeLens/refresh" | "workspace/diagnostic/refresh" | "workspace/foldingRange/refresh" => {
                if let Some(id) = id.clone() { let _ = srv.write(&json!({"jsonrpc":"2.0","id":id,"result":null})).await; handled = true; }
            }
            "workspace/configuration" => {
                if let Some(id) = id.clone() {
                    let n = msg.get("params").and_then(|p| p.get("items")).and_then(|i| i.as_array()).map(|a| a.len()).unwrap_or(1);
                    let items: Vec<Value> = (0..n).map(|_| srv.config.settings.clone().unwrap_or(Value::Null)).collect();
                    let _ = srv.write(&json!({"jsonrpc":"2.0","id":id,"result":items})).await; handled = true;
                }
            }
            "workspace/workspaceFolders" => {
                if let Some(id) = id.clone() {
                    let name = srv.root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let _ = srv.write(&json!({"jsonrpc":"2.0","id":id,"result":[{"uri": path_to_uri(&srv.root), "name": name}]})).await; handled = true;
                }
            }
            "window/logMessage" | "window/showMessage" | "$/logTrace" => {
                let text = msg.get("params").and_then(|p| p.get("message")).and_then(|t| t.as_str()).unwrap_or("");
                tracing::debug!(server = %srv.config.name, "{text}");
            }
            _ => {}
        }
        if handled { if let Some(obj) = msg.as_object_mut() { obj.insert("caretHandled".into(), Value::Bool(true)); } }
    }
    let _ = srv.channel.read().send(msg);
}

async fn spawn_server(app: AppHandle, config: LspServerConfig, root: PathBuf, on_message: Channel<Value>) -> Result<Arc<LspServer>> {
    let st = app.state::<LspState>();
    let id = format!("{}-{}", config.id, &uuid::Uuid::new_v4().to_string()[..8]);
    let key = format!("{}|{}", config.id, root.display());
    // Resolve against the augmented PATH (Homebrew, ~/.local/bin, nvm, …): a Finder-launched app inherits launchd's
    // minimal PATH, so plain `which` misses user-installed servers like rust-analyzer or jdtls.
    let exe = crate::acp::process::resolve_command(&config.command).or_else(|| which::which(&config.command).ok()).ok_or_else(|| CaretError::new("missing", format!("{} not found on PATH{}", config.command, config.install_hint.as_ref().map(|h| format!(". Install: {h}")).unwrap_or_default())));
    let make = |state: &str, error: Option<String>, pid, stdin, child| Arc::new(LspServer { id: id.clone(), config: config.clone(), root: root.clone(), stdin: tokio::sync::Mutex::new(stdin), child: tokio::sync::Mutex::new(child), state: RwLock::new(state.into()), error: RwLock::new(error), pid, channel: RwLock::new(on_message.clone()), stderr_tail: Mutex::new(VecDeque::new()), internal_id: AtomicI64::new(1) });
    let exe = match exe { Ok(e) => e, Err(e) => { let srv = make("missing", Some(e.message.clone()), None, None, None); st.servers.insert(id.clone(), srv.clone()); st.by_key.insert(key, id.clone()); broadcast(&app); return Ok(srv); } };
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(&config.args).current_dir(&root).env("PATH", crate::acp::process::augmented_path()).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    if let Some(env) = &config.env { for (k, v) in env { cmd.env(k, v); } }
    let mut child = match cmd.spawn() { Ok(c) => c, Err(e) => { let srv = make("error", Some(format!("failed to start {}: {e}", config.command)), None, None, None); st.servers.insert(id.clone(), srv.clone()); st.by_key.insert(key, id.clone()); broadcast(&app); return Ok(srv); } };
    let pid = child.id();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let srv = make("running", None, pid, stdin, Some(child));
    st.servers.insert(id.clone(), srv.clone());
    st.by_key.insert(key.clone(), id.clone());
    broadcast(&app);
    tracing::info!(server = %config.name, id = %id, pid = ?pid, root = %root.display(), "lsp server started");
    // stderr tail
    if let Some(err) = stderr {
        let s = srv.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(l)) = lines.next_line().await { tracing::debug!(server = %s.config.name, "stderr: {l}"); let mut t = s.stderr_tail.lock(); if t.len() >= 200 { t.pop_front(); } t.push_back(l); }
        });
    }
    // stdout reader
    if let Some(out) = stdout {
        let s = srv.clone(); let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut r = BufReader::with_capacity(64 * 1024, out);
            loop {
                match read_message(&mut r).await {
                    Ok(Some(msg)) => handle_server_message(&app2, &s, msg).await,
                    Ok(None) => break,
                    Err(e) => { tracing::warn!(server = %s.config.name, error = %e, "lsp read error"); break; }
                }
            }
            // process ended
            let code = { let mut c = s.child.lock().await; match c.as_mut() { Some(ch) => ch.wait().await.ok().and_then(|st| st.code()), None => None } };
            *s.state.write() = "stopped".into();
            if code.unwrap_or(0) != 0 { let tail: Vec<String> = s.stderr_tail.lock().iter().rev().take(5).cloned().collect(); *s.error.write() = Some(format!("exited with code {:?}{}", code, if tail.is_empty() { String::new() } else { format!(": {}", tail.join(" | ")) })); }
            app2.state::<DiagnosticsStore>().clear_server(&s.id);
            let _ = s.channel.read().send(json!({"jsonrpc":"2.0","method":"$/caret/exit","params":{"serverId": s.id, "code": code}}));
            let st = app2.state::<LspState>();
            st.by_key.remove(&key);
            broadcast(&app2);
            tracing::info!(server = %s.config.name, ?code, "lsp server exited");
        });
    }
    Ok(srv)
}

pub async fn stop_server(app: &AppHandle, server_id: &str) {
    let st = app.state::<LspState>();
    let Some((_, srv)) = st.servers.remove(server_id) else { return };
    st.by_key.retain(|_, v| v != server_id);
    if *srv.state.read() == "running" {
        let id = srv.internal_id.fetch_add(1, Ordering::Relaxed);
        let _ = srv.write(&json!({"jsonrpc":"2.0","id":format!("caret-shutdown-{id}"),"method":"shutdown","params":null})).await;
        let _ = srv.write(&json!({"jsonrpc":"2.0","method":"exit","params":null})).await;
        { let mut g = srv.stdin.lock().await; *g = None; }
        for _ in 0..20 { if *srv.state.read() != "running" { break; } tokio::time::sleep(std::time::Duration::from_millis(100)).await; }
        if *srv.state.read() == "running" { if let Some(ch) = srv.child.lock().await.as_mut() { let _ = ch.start_kill(); } }
    }
    *srv.state.write() = "stopped".into();
    app.state::<DiagnosticsStore>().clear_server(server_id);
    broadcast(app);
}

pub fn init(app: &AppHandle) {
    let h = app.clone();
    app.listen(events::WORKSPACE_CHANGED, move |_| {
        let h2 = h.clone();
        tauri::async_runtime::spawn(async move {
            let ids: Vec<String> = h2.state::<LspState>().servers.iter().map(|e| e.key().clone()).collect();
            for id in ids { stop_server(&h2, &id).await; }
        });
    });
}

// ───────────────────────── commands ─────────────────────────

#[tauri::command(async)]
pub fn lsp_configs(settings: State<'_, SettingsState>) -> Result<Vec<LspConfigEntry>> {
    let s = settings.effective();
    Ok(s.lsp.servers.into_iter().map(|c| { let installed = crate::acp::process::resolve_command(&c.command).is_some() || which::which(&c.command).is_ok(); LspConfigEntry { config: c, installed } }).collect())
}

#[tauri::command]
pub async fn lsp_start(app: AppHandle, config_id: String, root: String, on_message: Channel<Value>) -> Result<LspServerStatus> {
    let settings = app.state::<SettingsState>().effective();
    if !settings.lsp.enabled { return Err(CaretError::config("Language servers are disabled in settings")); }
    let config = settings.lsp.servers.iter().find(|c| c.id == config_id).cloned().ok_or_else(|| CaretError::not_found(format!("LSP config {config_id}")))?;
    if !config.enabled { return Err(CaretError::config(format!("{} is disabled", config.name))); }
    let root = std::fs::canonicalize(&root).unwrap_or_else(|_| PathBuf::from(&root));
    let key = format!("{}|{}", config.id, root.display());
    let st = app.state::<LspState>();
    if let Some(existing) = st.by_key.get(&key).and_then(|id| st.servers.get(id.value()).map(|s| s.clone())) {
        // A (re)starting client always needs a fresh `initialize` handshake, so restart rather than reuse.
        let eid = existing.id.clone(); drop(existing);
        stop_server(&app, &eid).await;
        st.servers.remove(&eid); st.by_key.remove(&key);
    }
    let srv = spawn_server(app.clone(), config, root, on_message).await?;
    Ok(srv.status())
}

#[tauri::command]
pub async fn lsp_stop(app: AppHandle, server_id: String) -> Result<()> { stop_server(&app, &server_id).await; Ok(()) }

#[tauri::command]
pub async fn lsp_send(st: State<'_, LspState>, server_id: String, message: Value) -> Result<()> {
    let srv = st.servers.get(&server_id).map(|s| s.clone()).ok_or_else(|| CaretError::not_found(format!("language server {server_id}")))?;
    srv.write(&message).await
}

#[tauri::command(async)]
pub fn lsp_status(st: State<'_, LspState>) -> Result<Vec<LspServerStatus>> { Ok(st.statuses()) }

#[tauri::command(async)]
pub fn lsp_diagnostics(store: State<'_, DiagnosticsStore>) -> Result<Vec<FileDiagnostics>> { Ok(store.all()) }

#[tauri::command(async)]
pub fn lsp_diagnostics_for(store: State<'_, DiagnosticsStore>, path: String) -> Result<Vec<FileDiagnostics>> { Ok(store.for_path(&path)) }

#[tauri::command]
pub fn lsp_report_diagnostics(app: AppHandle, store: State<'_, DiagnosticsStore>, diags: FileDiagnostics) -> Result<()> {
    store.publish(diags.clone());
    events::emit(&app, events::LSP_DIAGNOSTICS, diags);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn frames_parse_including_split_headers() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#;
        let data = format!("Content-Length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{}Content-Length: 2\r\n\r\n{{}}", body.len(), body);
        let mut r = BufReader::new(std::io::Cursor::new(data.into_bytes()));
        let m1 = read_message(&mut r).await.unwrap().unwrap();
        assert_eq!(m1["result"]["ok"], true);
        let m2 = read_message(&mut r).await.unwrap().unwrap();
        assert!(m2.as_object().unwrap().is_empty());
        assert!(read_message(&mut r).await.unwrap().is_none());
    }
    #[test]
    fn uri_roundtrip() {
        let p = Path::new("/Users/me/pro ject/a.rs");
        let u = path_to_uri(p);
        assert!(u.starts_with("file:///"));
        assert_eq!(uri_to_path(&u).as_deref(), Some("/Users/me/pro ject/a.rs"));
        assert_eq!(uri_to_path("file:///tmp/x%20y.ts").as_deref(), Some("/tmp/x y.ts"));
    }
}

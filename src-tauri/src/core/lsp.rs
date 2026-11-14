//! Language-server manager: spawns servers, relays JSON-RPC framing to the UI over a Channel,
//! mirrors diagnostics into `DiagnosticsStore`, and answers simple server→client requests itself.
//!
//! Servers are per root (`config.id|root`); status and diagnostics go to the windows showing that root
//! (`events::emit_to` the owning window); every window runs its own servers, and a window's servers stop when it
//! leaves their folder or is destroyed (`servers_to_stop`, `stop_servers_for_window`).
use crate::core::diagnostics::{DiagnosticsStore, FileDiagnostics};
use crate::core::settings::{LspServerConfig, SettingsState};
use crate::core::workspace::WorkspaceState;
use crate::error::{CaretError, Result};
use crate::events::{self, WorkspaceChange};
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspServerStatus { pub server_id: String, pub config_id: String, pub name: String, pub state: String, #[serde(skip_serializing_if = "Option::is_none")] pub pid: Option<u32>, #[serde(skip_serializing_if = "Option::is_none")] pub error: Option<String>, pub languages: Vec<String>, pub root: String }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LspConfigEntry {
    pub config: LspServerConfig,
    pub installed: bool,
    /// Where the server binary was found (workspace `node_modules/.bin`, `~/.caret/lsp`, or PATH).
    #[serde(skip_serializing_if = "Option::is_none")] pub resolved: Option<String>,
    /// Caret can install this server itself (`lsp_install`): it is published on npm and npm is available.
    pub installable: bool,
}

/// `~/.caret/lsp`: npm prefix for servers Caret installs itself (Java's jdtls lives here too).
pub fn lsp_home() -> PathBuf { crate::paths::sub_dir("lsp") }
fn lsp_bin_dir() -> PathBuf { lsp_home().join("node_modules").join(".bin") }

/// Find a server binary. Order: an explicit path as given; the workspace's own `node_modules/.bin` (a repo that
/// pins its language server wins over a global one); Caret's managed `~/.caret/lsp`; then the augmented PATH
/// (Homebrew, nvm, ~/.local/bin, …) and plain `which`.
pub fn resolve_server_command(command: &str, root: Option<&Path>) -> Option<PathBuf> {
    if command.is_empty() { return None; }
    if command.contains('/') { return crate::acp::process::resolve_command(command); }
    if let Some(r) = root { let c = r.join("node_modules").join(".bin").join(command); if c.is_file() { return Some(c); } }
    let managed = lsp_bin_dir().join(command);
    if managed.is_file() { return Some(managed); }
    crate::acp::process::resolve_command(command).or_else(|| which::which(command).ok())
}

/// A user's saved copy of the server list predates fields added later (`npm`); take those from the built-in entry
/// with the same id when the user has not set them.
fn with_defaults(mut c: LspServerConfig) -> LspServerConfig {
    let Some(d) = crate::core::settings::default_lsp_servers().into_iter().find(|d| d.id == c.id) else { return c };
    if c.npm.is_none() { c.npm = d.npm.clone(); }
    if c.install_hint.is_none() { c.install_hint = d.install_hint.clone(); }
    // A saved explicit path that no longer exists (a wiped checkout, a moved toolchain) must not pin the server to a
    // dead binary forever: fall back to the built-in command name, which the resolver finds wherever it lives now.
    if c.command.contains('/') && crate::acp::process::resolve_command(&c.command).is_none() && !d.command.is_empty() {
        tracing::warn!(server = %c.id, stale = %c.command, fallback = %d.command, "lsp: configured server path is gone; using the default command");
        c.command = d.command.clone();
        if c.args.is_empty() { c.args = d.args.clone(); }
    }
    c
}

fn entry_for(c: LspServerConfig, root: Option<&Path>, npm_available: bool) -> LspConfigEntry {
    let c = with_defaults(c);
    let resolved = resolve_server_command(&c.command, root);
    LspConfigEntry { installed: resolved.is_some(), resolved: resolved.map(|p| p.to_string_lossy().to_string()), installable: npm_available && c.npm.is_some(), config: c }
}

pub struct LspServer {
    pub id: String,
    pub config: LspServerConfig,
    pub root: PathBuf,
    /// Label of the window whose client drives this server. Each window runs its own servers, even for a folder
    /// another window also shows: an LSP `initialize` handshake belongs to exactly one client, so two windows cannot
    /// share one process (they would restart each other's server in a loop).
    pub window: String,
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
    /// Every server of every root (sorted by name).
    pub fn statuses(&self) -> Vec<LspServerStatus> { let mut v: Vec<_> = self.servers.iter().map(|e| e.value().status()).collect(); v.sort_by(|a, b| a.name.cmp(&b.name)); v }
    /// The servers owned by one window (what that window's status bar shows).
    pub fn statuses_for_window(&self, label: &str) -> Vec<LspServerStatus> {
        let mut v: Vec<_> = self.servers.iter().filter(|e| e.value().window == label).map(|e| e.value().status()).collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }
    /// Ids of the servers owned by `label`.
    pub fn server_ids_for_window(&self, label: &str) -> Vec<String> { self.servers.iter().filter(|e| e.value().window == label).map(|e| e.key().clone()).collect() }
}

/// Send a window the status list of its servers.
fn broadcast(app: &AppHandle, window: &str) { let st = app.state::<LspState>(); events::emit_to(app, window, events::LSP_STATUS, st.statuses_for_window(window)); }

/// Which servers a workspace change stops: the changing window's own servers that are not on the folder it now
/// shows (it switched folders, closed its folder, or is going away). Other windows' servers are never touched, even
/// on the same folder: each window owns its servers.
pub fn servers_to_stop(change: &WorkspaceChange, servers: impl IntoIterator<Item = (String, PathBuf, String)>) -> Vec<String> {
    let new_root = change.info.as_ref().map(|i| PathBuf::from(&i.root));
    servers.into_iter().filter(|(_, root, window)| *window == change.label && new_root.as_deref() != Some(root.as_path())).map(|(id, _, _)| id).collect()
}

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
                        events::emit_to(app, &srv.window, events::LSP_DIAGNOSTICS, fd);
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

async fn spawn_server(app: AppHandle, window: String, config: LspServerConfig, root: PathBuf, on_message: Channel<Value>) -> Result<Arc<LspServer>> {
    let st = app.state::<LspState>();
    let id = format!("{}-{}", config.id, &uuid::Uuid::new_v4().to_string()[..8]);
    let key = server_key(&config.id, &root, &window);
    // Resolve against the augmented PATH (Homebrew, ~/.local/bin, nvm, …): a Finder-launched app inherits launchd's
    // minimal PATH, so plain `which` misses user-installed servers like rust-analyzer or jdtls.
    let exe = resolve_server_command(&config.command, Some(&root)).ok_or_else(|| CaretError::new("missing", format!("{} not found on PATH{}", config.command, config.install_hint.as_ref().map(|h| format!(". Install: {h}")).unwrap_or_default())));
    let make = |state: &str, error: Option<String>, pid, stdin, child| Arc::new(LspServer { id: id.clone(), config: config.clone(), root: root.clone(), window: window.clone(), stdin: tokio::sync::Mutex::new(stdin), child: tokio::sync::Mutex::new(child), state: RwLock::new(state.into()), error: RwLock::new(error), pid, channel: RwLock::new(on_message.clone()), stderr_tail: Mutex::new(VecDeque::new()), internal_id: AtomicI64::new(1) });
    let exe = match exe { Ok(e) => e, Err(e) => { let srv = make("missing", Some(e.message.clone()), None, None, None); st.servers.insert(id.clone(), srv.clone()); st.by_key.insert(key, id.clone()); broadcast(&app, &window); return Ok(srv); } };
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(&config.args).current_dir(&root).env("PATH", crate::acp::process::augmented_path()).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    if let Some(env) = &config.env { for (k, v) in env { cmd.env(k, v); } }
    let mut child = match cmd.spawn() { Ok(c) => c, Err(e) => { let srv = make("error", Some(format!("failed to start {}: {e}", config.command)), None, None, None); st.servers.insert(id.clone(), srv.clone()); st.by_key.insert(key, id.clone()); broadcast(&app, &window); return Ok(srv); } };
    let pid = child.id();
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let srv = make("running", None, pid, stdin, Some(child));
    st.servers.insert(id.clone(), srv.clone());
    st.by_key.insert(key.clone(), id.clone());
    broadcast(&app, &window);
    tracing::info!(server = %config.name, id = %id, pid = ?pid, root = %root.display(), window = %window, "lsp server started");
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
            broadcast(&app2, &s.window);
            tracing::info!(server = %s.config.name, ?code, window = %s.window, "lsp server exited");
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
    broadcast(app, &srv.window);
}

fn server_key(config_id: &str, root: &Path, window: &str) -> String { format!("{config_id}|{}|{window}", root.display()) }

/// Stop a window's servers when it leaves their folder (switching folders, closing the folder, or being destroyed).
/// Servers of other windows are untouched, including ones on the same folder.
pub fn init(app: &AppHandle) {
    events::on_workspace_change(app, |app, change| {
        let ids = servers_to_stop(&change, app.state::<LspState>().servers.iter().map(|e| (e.key().clone(), e.value().root.clone(), e.value().window.clone())));
        if ids.is_empty() { return; }
        let h = app.clone();
        let label = change.label.clone();
        tauri::async_runtime::spawn(async move {
            let t0 = std::time::Instant::now();
            let n = ids.len();
            for id in ids { stop_server(&h, &id).await; }
            tracing::info!(servers = n, window = %label, ms = t0.elapsed().as_millis() as u64, "lsp servers stopped: window left the folder");
        });
    });
}

/// Stop every server a destroyed window owned (its webview is gone, nobody will drive them).
pub fn stop_servers_for_window(app: &AppHandle, label: &str) {
    let ids = app.state::<LspState>().server_ids_for_window(label);
    if ids.is_empty() { return; }
    let h = app.clone();
    let label = label.to_string();
    tauri::async_runtime::spawn(async move { let n = ids.len(); for id in ids { stop_server(&h, &id).await; } tracing::info!(servers = n, window = %label, "lsp servers stopped: window destroyed"); });
}

// ───────────────────────── commands ─────────────────────────

#[tauri::command(async)]
pub fn lsp_configs(settings: State<'_, SettingsState>, root: Option<String>) -> Result<Vec<LspConfigEntry>> {
    let s = settings.effective();
    let root = root.map(PathBuf::from);
    let npm = crate::acp::process::resolve_command("npm").is_some();
    Ok(s.lsp.servers.into_iter().map(|c| entry_for(c, root.as_deref(), npm)).collect())
}

/// Install an npm-distributed server into `~/.caret/lsp` (`npm install <pkgs>@latest` with that prefix) and return
/// the refreshed entry. Streams npm's output to the log; the UI shows progress from the `installing` flag it keeps.
#[tauri::command]
pub async fn lsp_install(app: AppHandle, config_id: String, root: Option<String>) -> Result<LspConfigEntry> {
    let settings = app.state::<SettingsState>().effective();
    let config = settings.lsp.servers.iter().find(|c| c.id == config_id).cloned().map(with_defaults).ok_or_else(|| CaretError::not_found(format!("LSP config {config_id}")))?;
    let pkgs = config.npm.clone().ok_or_else(|| CaretError::invalid(format!("{} is not installable from Caret{}", config.name, config.install_hint.as_deref().map(|h| format!("; install it yourself: {h}")).unwrap_or_default())))?;
    let npm = crate::acp::process::resolve_command("npm").ok_or_else(|| CaretError::not_found("npm (install Node.js ≥ 18 from https://nodejs.org and retry)"))?;
    let home = lsp_home();
    if !home.join("package.json").exists() { let _ = std::fs::write(home.join("package.json"), "{\n  \"name\": \"caret-language-servers\",\n  \"private\": true\n}\n"); }
    let specs: Vec<String> = pkgs.split_whitespace().map(|p| format!("{p}@latest")).collect();
    tracing::info!(server = %config.id, packages = %pkgs, prefix = %home.display(), "lsp: installing language server");
    let t0 = std::time::Instant::now();
    let mut cmd = tokio::process::Command::new(&npm);
    cmd.args(["install", "--no-fund", "--no-audit", "--loglevel", "error"]).args(&specs).current_dir(&home).env("PATH", crate::acp::process::augmented_path()).env("npm_config_prefix", &home).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    let out = tokio::time::timeout(std::time::Duration::from_secs(600), cmd.output()).await.map_err(|_| CaretError::io("npm install timed out after 10 minutes"))?.map_err(|e| CaretError::io(format!("npm install failed to start: {e}")))?;
    for l in String::from_utf8_lossy(&out.stdout).lines().chain(String::from_utf8_lossy(&out.stderr).lines()) { if !l.trim().is_empty() { tracing::info!(server = %config.id, "lsp install: {l}"); } }
    if !out.status.success() { return Err(CaretError::io(format!("npm install {pkgs} failed (exit {}): {}", out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stderr).chars().take(800).collect::<String>()))); }
    let entry = entry_for(config, root.as_deref().map(Path::new), true);
    tracing::info!(server = %entry.config.id, installed = entry.installed, resolved = ?entry.resolved, ms = t0.elapsed().as_millis() as u64, "lsp: language server installed");
    if !entry.installed { return Err(CaretError::io(format!("npm install finished but `{}` is not in {}", entry.config.command, lsp_bin_dir().display()))); }
    Ok(entry)
}

#[tauri::command]
pub async fn lsp_start(app: AppHandle, window: tauri::Window, config_id: String, root: String, on_message: Channel<Value>) -> Result<LspServerStatus> {
    let settings = app.state::<SettingsState>().effective();
    if !settings.lsp.enabled { return Err(CaretError::config("Language servers are disabled in settings")); }
    let config = settings.lsp.servers.iter().find(|c| c.id == config_id).cloned().map(with_defaults).ok_or_else(|| CaretError::not_found(format!("LSP config {config_id}")))?;
    if !config.enabled { return Err(CaretError::config(format!("{} is disabled", config.name))); }
    let root = std::fs::canonicalize(&root).unwrap_or_else(|_| PathBuf::from(&root));
    let label = window.label().to_string();
    let key = server_key(&config.id, &root, &label);
    let st = app.state::<LspState>();
    if let Some(existing) = st.by_key.get(&key).and_then(|id| st.servers.get(id.value()).map(|s| s.clone())) {
        // A (re)starting client always needs a fresh `initialize` handshake, so restart rather than reuse.
        let eid = existing.id.clone(); drop(existing);
        stop_server(&app, &eid).await;
        st.servers.remove(&eid); st.by_key.remove(&key);
    }
    let srv = spawn_server(app.clone(), label, config, root, on_message).await?;
    Ok(srv.status())
}

#[tauri::command]
pub async fn lsp_stop(app: AppHandle, server_id: String) -> Result<()> { stop_server(&app, &server_id).await; Ok(()) }

#[tauri::command]
pub async fn lsp_send(st: State<'_, LspState>, server_id: String, message: Value) -> Result<()> {
    let srv = st.servers.get(&server_id).map(|s| s.clone()).ok_or_else(|| CaretError::not_found(format!("language server {server_id}")))?;
    srv.write(&message).await
}

/// The servers of the calling window's folder; empty for a window without a folder.
#[tauri::command(async)]
pub fn lsp_status(st: State<'_, LspState>, window: tauri::Window) -> Result<Vec<LspServerStatus>> { Ok(st.statuses_for_window(window.label())) }

/// Diagnostics of files under the calling window's folder; every file when the window has no folder (loose files).
#[tauri::command(async)]
pub fn lsp_diagnostics(store: State<'_, DiagnosticsStore>, lsp: State<'_, LspState>, ws: State<'_, WorkspaceState>, window: tauri::Window) -> Result<Vec<FileDiagnostics>> {
    let all = match ws.root_for(window.label()) { Some(r) => store.under_root(&r), None => store.all() };
    // Two windows on one folder each run their own servers over the same files: keep this window's servers' entries
    // plus anything that is not an LSP server (diagnostics the UI reported itself).
    let foreign: std::collections::HashSet<String> = lsp.servers.iter().filter(|e| e.value().window != window.label()).map(|e| e.key().clone()).collect();
    Ok(all.into_iter().filter(|d| !foreign.contains(&d.server_id)).collect())
}

#[tauri::command(async)]
pub fn lsp_diagnostics_for(store: State<'_, DiagnosticsStore>, path: String) -> Result<Vec<FileDiagnostics>> { Ok(store.for_path(&path)) }

/// UI-computed diagnostics (Monaco's TS worker) for a file; relayed to every window on the same folder.
#[tauri::command]
pub fn lsp_report_diagnostics(app: AppHandle, store: State<'_, DiagnosticsStore>, ws: State<'_, WorkspaceState>, window: tauri::Window, diags: FileDiagnostics) -> Result<()> {
    store.publish(diags.clone());
    match ws.root_for(window.label()) {
        Some(root) => events::emit_for_root(&app, &root, events::LSP_DIAGNOSTICS, diags),
        None => events::emit_to(&app, window.label(), events::LSP_DIAGNOSTICS, diags),
    }
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

    #[test]
    fn workspace_node_modules_wins_then_managed_prefix_then_path() {
        let d = tempfile::tempdir().unwrap(); let root = d.path();
        let bin = root.join("node_modules/.bin"); std::fs::create_dir_all(&bin).unwrap();
        let local = bin.join("typescript-language-server"); std::fs::write(&local, "#!/bin/sh\n").unwrap();
        assert_eq!(resolve_server_command("typescript-language-server", Some(root)).as_deref(), Some(local.as_path()), "a repo that pins its own server wins");
        assert!(resolve_server_command("definitely-not-a-real-server-xyz", Some(root)).is_none());
        assert!(resolve_server_command("", Some(root)).is_none());
        // an explicit path is taken literally
        assert_eq!(resolve_server_command(local.to_str().unwrap(), None).as_deref(), Some(local.as_path()));
        assert!(resolve_server_command("/nonexistent/dir/typescript-language-server", Some(root)).is_none());
        // the shell is always on PATH: the resolver falls through to it for bare names
        assert!(resolve_server_command("sh", Some(root)).is_some());
    }

    #[test]
    fn stale_saved_path_falls_back_to_the_default_command_and_keeps_npm_spec() {
        let mut c = crate::core::settings::default_lsp_servers().into_iter().find(|c| c.id == "typescript").unwrap();
        c.command = "/private/tmp/wiped-checkout/node_modules/.bin/typescript-language-server".into();
        c.npm = None; c.install_hint = None;
        let fixed = with_defaults(c);
        assert_eq!(fixed.command, "typescript-language-server");
        assert_eq!(fixed.args, vec!["--stdio".to_string()]);
        assert_eq!(fixed.npm.as_deref(), Some("typescript-language-server typescript"));
        assert!(fixed.install_hint.is_some());
        // a user-defined server with no built-in counterpart is left alone
        let custom = LspServerConfig { id: "mine".into(), command: "/nope/server".into(), ..Default::default() };
        assert_eq!(with_defaults(custom).command, "/nope/server");
        // an existing explicit path is respected
        let mut ok = crate::core::settings::default_lsp_servers().into_iter().find(|c| c.id == "typescript").unwrap();
        ok.command = "/bin/sh".into();
        assert_eq!(with_defaults(ok).command, "/bin/sh");
    }

    fn change(prev_root: Option<&str>, new_root: Option<&str>, last_for_prev_root: bool) -> WorkspaceChange {
        WorkspaceChange { label: "w2".into(), info: new_root.map(|r| crate::core::workspace::info_for(Path::new(r))), prev_root: prev_root.map(String::from), first_for_root: new_root.is_some(), last_for_prev_root }
    }
    fn servers() -> Vec<(String, PathBuf, String)> {
        vec![("ts-a".into(), PathBuf::from("/work/a"), "w2".into()), ("rust-a".into(), PathBuf::from("/work/a"), "w2".into()), ("ts-b".into(), PathBuf::from("/work/b"), "w2".into()), ("ts-a-main".into(), PathBuf::from("/work/a"), "main".into())]
    }

    #[test]
    fn stops_the_changing_windows_servers_that_are_not_on_its_new_folder() {
        // w2 switches from /work/a to /work/c: its a-servers stop, its b-server too; main's server on /work/a stays
        let mut ids = servers_to_stop(&change(Some("/work/a"), Some("/work/c"), true), servers()); ids.sort();
        assert_eq!(ids, vec!["rust-a".to_string(), "ts-a".to_string(), "ts-b".to_string()]);
        // w2 closes its folder (or is destroyed): everything w2 owns stops, nothing of main's
        let mut ids = servers_to_stop(&change(Some("/work/a"), None, true), servers()); ids.sort();
        assert_eq!(ids, vec!["rust-a".to_string(), "ts-a".to_string(), "ts-b".to_string()]);
        // the flags about other windows do not matter: ownership does
        let mut ids = servers_to_stop(&change(Some("/work/a"), None, false), servers()); ids.sort();
        assert_eq!(ids, vec!["rust-a".to_string(), "ts-a".to_string(), "ts-b".to_string()]);
    }

    #[test]
    fn keeps_the_servers_on_the_folder_the_window_now_shows_and_other_windows_servers() {
        // w2 (re)opens /work/a: its a-servers stay, only the stray b-server stops
        assert_eq!(servers_to_stop(&change(None, Some("/work/a"), false), servers()), vec!["ts-b".to_string()]);
        // a window with no servers
        let mut c = change(Some("/work/a"), None, true); c.label = "w9".into();
        assert!(servers_to_stop(&c, servers()).is_empty());
        // main leaving /work/a stops only main's server even though w2 still shows the same folder
        let mut c = change(Some("/work/a"), Some("/work/c"), false); c.label = "main".into();
        assert_eq!(servers_to_stop(&c, servers()), vec!["ts-a-main".to_string()]);
    }

    #[test]
    fn statuses_for_window_filters_by_owner() {
        let st = LspState::default();
        let mk = |id: &str, root: &str, window: &str| Arc::new(LspServer { id: id.into(), config: LspServerConfig { id: id.into(), name: id.into(), ..Default::default() }, root: PathBuf::from(root), window: window.into(), stdin: tokio::sync::Mutex::new(None), child: tokio::sync::Mutex::new(None), state: RwLock::new("running".into()), error: RwLock::new(None), pid: None, channel: RwLock::new(Channel::new(|_| Ok(()))), stderr_tail: Mutex::new(VecDeque::new()), internal_id: AtomicI64::new(1) });
        st.servers.insert("a1".into(), mk("a1", "/work/a", "main"));
        st.servers.insert("b1".into(), mk("b1", "/work/b", "w2"));
        st.servers.insert("a2".into(), mk("a2", "/work/a", "w2"));
        let main: Vec<String> = st.statuses_for_window("main").into_iter().map(|s| s.server_id).collect();
        assert_eq!(main, vec!["a1".to_string()], "the same folder in another window has its own server");
        let mut w2: Vec<String> = st.statuses_for_window("w2").into_iter().map(|s| s.server_id).collect(); w2.sort();
        assert_eq!(w2, vec!["a2".to_string(), "b1".to_string()]);
        assert!(st.statuses_for_window("w3").is_empty());
        let mut ids = st.server_ids_for_window("w2"); ids.sort();
        assert_eq!(ids, vec!["a2".to_string(), "b1".to_string()]);
        assert_eq!(st.statuses().len(), 3, "the unfiltered list still has every server");
        assert_eq!(server_key("typescript", Path::new("/work/a"), "w2"), "typescript|/work/a|w2");
    }

    #[test]
    fn entries_report_installable_only_for_npm_servers_with_npm_present() {
        let ts = crate::core::settings::default_lsp_servers().into_iter().find(|c| c.id == "typescript").unwrap();
        let rust = crate::core::settings::default_lsp_servers().into_iter().find(|c| c.id == "rust").unwrap();
        assert!(entry_for(ts.clone(), None, true).installable);
        assert!(!entry_for(ts, None, false).installable, "no npm, nothing to install with");
        assert!(!entry_for(rust, None, true).installable, "rust-analyzer is not on npm");
    }
}

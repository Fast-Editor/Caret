//! MCP connection manager over rmcp 3.x: stdio (child process) and streamable-HTTP servers.
//!
//! Two layers of servers: the user's (`~/.caret/mcp.json`, one set for the whole process) and one set per open
//! workspace root (`<root>/.caret/mcp.json`), started on the first window for that root and stopped when its last
//! window leaves (see docs/MULTI_WINDOW.md). Every lookup takes the root it is asked from: a window's folder, or an
//! agent session's `workspace_root`; `None` means "user servers only" (an empty window, a scratch thread).
use crate::core::ports::{self, McpCallResult, McpContent, McpPort, McpResourceDesc, McpToolDesc};
use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use crate::events;
use crate::mcp::config::{self, McpServerConfig, McpTransport};
use dashmap::DashMap;
use parking_lot::RwLock;
use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientConfig, ContentBlock, GetPromptRequestParams, Implementation, Prompt, ReadResourceRequestParams, Resource, ResourceContents, Tool};
use rmcp::service::{NotificationContext, RunningService};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStatus {
    pub id: String, pub name: String, pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub error: Option<String>,
    pub tool_count: usize, pub resource_count: usize, pub prompt_count: usize, pub transport: McpTransport, pub source: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInfo { pub server_id: String, pub name: String, pub qualified_name: String, #[serde(skip_serializing_if = "Option::is_none")] pub description: Option<String>, pub input_schema: Value, pub enabled: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourceInfo { pub server_id: String, pub uri: String, pub name: String, #[serde(skip_serializing_if = "Option::is_none")] pub description: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] pub mime_type: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPromptArg { pub name: String, #[serde(skip_serializing_if = "Option::is_none")] pub description: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] pub required: Option<bool> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPromptInfo { pub server_id: String, pub name: String, #[serde(skip_serializing_if = "Option::is_none")] pub description: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] pub arguments: Option<Vec<McpPromptArg>> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPromptMessage { pub role: String, pub text: String }

struct Handler { server_id: String, root: Option<PathBuf>, mgr: Weak<McpInner> }

impl ClientHandler for Handler {
    fn get_info(&self) -> ClientConfig { ClientConfig::new(ClientCapabilities::default(), Implementation::new("Caret", env!("CARGO_PKG_VERSION"))) }
    fn on_tool_list_changed(&self, _ctx: NotificationContext<RoleClient>) -> impl std::future::Future<Output = ()> + Send + '_ {
        async move { if let Some(m) = self.mgr.upgrade() { m.refresh_catalog(self.root.as_deref(), &self.server_id).await; } }
    }
    fn on_resource_list_changed(&self, _ctx: NotificationContext<RoleClient>) -> impl std::future::Future<Output = ()> + Send + '_ {
        async move { if let Some(m) = self.mgr.upgrade() { m.refresh_catalog(self.root.as_deref(), &self.server_id).await; } }
    }
    fn on_prompt_list_changed(&self, _ctx: NotificationContext<RoleClient>) -> impl std::future::Future<Output = ()> + Send + '_ {
        async move { if let Some(m) = self.mgr.upgrade() { m.refresh_catalog(self.root.as_deref(), &self.server_id).await; } }
    }
}

struct Conn {
    config: McpServerConfig,
    /// The workspace root this server belongs to; `None` for user-scope servers.
    root: Option<PathBuf>,
    state: RwLock<String>,
    error: RwLock<Option<String>>,
    running: tokio::sync::Mutex<Option<RunningService<RoleClient, Handler>>>,
    peer: RwLock<Option<rmcp::Peer<RoleClient>>>,
    tools: RwLock<Vec<Tool>>,
    resources: RwLock<Vec<Resource>>,
    prompts: RwLock<Vec<Prompt>>,
    attempts: std::sync::atomic::AtomicU32,
}

/// The servers of one layer: the user's (`root: None`) or one workspace root's.
struct ServerSet { root: Option<PathBuf>, conns: RwLock<HashMap<String, Arc<Conn>>> }

impl ServerSet {
    fn new(root: Option<PathBuf>) -> Arc<Self> { Arc::new(Self { root, conns: RwLock::new(HashMap::new()) }) }
    fn get(&self, id: &str) -> Option<Arc<Conn>> { self.conns.read().get(id).cloned() }
    fn all(&self) -> Vec<Arc<Conn>> { self.conns.read().values().cloned().collect() }
}

pub struct McpInner {
    app: AppHandle,
    user: Arc<ServerSet>,
    workspaces: DashMap<PathBuf, Arc<ServerSet>>,
    /// Keyed by `log_key(root, id)`, so two roots with a `project:fs` keep separate logs.
    logs: DashMap<String, VecDeque<String>>,
}

#[derive(Clone)]
pub struct McpManager { pub inner: Arc<McpInner> }

impl McpManager {
    pub fn new(app: AppHandle) -> Self {
        Self { inner: Arc::new(McpInner { app, user: ServerSet::new(None), workspaces: DashMap::new(), logs: DashMap::new() }) }
    }
    pub fn install(&self) {
        let me = self.inner.clone();
        events::on_workspace_change(&self.inner.app, move |_, change| {
            if change.last_for_prev_root {
                if let Some(prev) = change.prev_root.as_deref() {
                    let me = me.clone(); let prev = PathBuf::from(prev);
                    tauri::async_runtime::spawn(async move { me.close_root(&prev).await; });
                }
            }
            if change.first_for_root {
                if let Some(info) = &change.info {
                    let me = me.clone(); let root = PathBuf::from(&info.root);
                    tauri::async_runtime::spawn(async move {
                        let t0 = std::time::Instant::now();
                        me.open_root(&root).await;
                        tracing::info!(root = %root.display(), servers = me.workspaces.get(&root).map(|s| s.conns.read().len()).unwrap_or(0), ms = t0.elapsed().as_millis() as u64, "mcp workspace servers started");
                    });
                }
            }
        });
        ports::set_mcp(Arc::new(McpPortImpl(self.inner.clone())));
        // user-scope servers do not need a workspace
        let me = self.inner.clone();
        tauri::async_runtime::spawn(async move { me.reload_user().await; });
    }
}

fn augmented_path() -> String {
    let home = crate::paths::home_dir();
    let mut parts: Vec<String> = vec![];
    for p in ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"] { parts.push(p.into()); }
    for p in [".cargo/bin", ".local/bin", ".bun/bin", ".deno/bin", "go/bin", ".volta/bin"] { parts.push(home.join(p).to_string_lossy().to_string()); }
    if let Ok(rd) = std::fs::read_dir(home.join(".nvm/versions/node")) { let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect(); v.sort(); if let Some(last) = v.last() { parts.push(last.join("bin").to_string_lossy().to_string()); } }
    let existing = std::env::var("PATH").unwrap_or_default();
    let mut all: Vec<String> = existing.split(':').filter(|s| !s.is_empty()).map(String::from).collect();
    for p in parts { if !all.contains(&p) { all.push(p); } }
    all.join(":")
}

fn log_key(root: Option<&Path>, id: &str) -> String { match root { Some(r) => format!("{}|{id}", r.display()), None => id.to_string() } }

impl McpInner {
    fn settings_compat(&self) -> bool { self.app.state::<SettingsState>().effective().general.use_cursor_compat_dirs }
    fn configs_user(&self) -> Vec<McpServerConfig> { let mut v = config::load_user(self.settings_compat()); config::apply_state(&mut v); v }
    fn configs_workspace(&self, root: &Path) -> Vec<McpServerConfig> { let mut v = config::load_workspace(root, self.settings_compat()); config::apply_state(&mut v); v }
    /// Every server visible from `root` (the user's plus that workspace's), as the Settings UI lists them.
    pub fn configs(&self, root: Option<&Path>) -> Vec<McpServerConfig> { config::load_all(root, self.settings_compat()) }

    pub fn log(&self, root: Option<&Path>, id: &str, line: String) {
        let mut e = self.logs.entry(log_key(root, id)).or_default();
        e.push_back(format!("{} {}", chrono::Local::now().format("%H:%M:%S"), line));
        while e.len() > 500 { e.pop_front(); }
    }
    pub fn logs(&self, root: Option<&Path>, id: &str) -> Vec<String> { self.logs.get(&log_key(self.root_for_id(root, id), id)).map(|l| l.iter().cloned().collect()).unwrap_or_default() }
    fn log_conn(&self, conn: &Conn, line: String) { self.log(conn.root.as_deref(), &conn.config.id, line); }

    /// The root a server id is scoped to when asked from `root`: workspace ids belong to that root, user ids to none.
    fn root_for_id<'a>(&self, root: Option<&'a Path>, id: &str) -> Option<&'a Path> { if config::is_workspace_scope(config::scope_of(id)) { root } else { None } }
    /// The layers visible from `root`: the user's, then that workspace's when it is open.
    fn sets_for(&self, root: Option<&Path>) -> Vec<Arc<ServerSet>> {
        let mut v = vec![self.user.clone()];
        if let Some(ws) = root.and_then(|r| self.workspaces.get(r).map(|s| s.clone())) { v.push(ws); }
        v
    }
    fn set_of(&self, conn: &Conn) -> Option<Arc<ServerSet>> { match &conn.root { None => Some(self.user.clone()), Some(r) => self.workspaces.get(r).map(|s| s.clone()) } }
    /// Look a server up by id as seen from `root`.
    fn conn(&self, root: Option<&Path>, id: &str) -> Option<Arc<Conn>> {
        match self.root_for_id(root, id) { None => self.user.get(id), Some(r) => self.workspaces.get(r)?.get(id) }
    }
    /// `true` while `conn` is still the registered connection for its id (a reload or a closed root replaces it).
    fn is_live(&self, conn: &Arc<Conn>) -> bool { self.set_of(conn).and_then(|s| s.get(&conn.config.id)).map(|c| Arc::ptr_eq(&c, conn)).unwrap_or(false) }
    fn conns_for(&self, root: Option<&Path>) -> Vec<Arc<Conn>> { self.sets_for(root).iter().flat_map(|s| s.all()).collect() }

    pub fn statuses(&self, root: Option<&Path>) -> Vec<McpServerStatus> {
        let mut v: Vec<McpServerStatus> = self.conns_for(root).iter().map(|c| McpServerStatus {
            id: c.config.id.clone(), name: c.config.name.clone(), state: c.state.read().clone(), error: c.error.read().clone(),
            tool_count: c.tools.read().len(), resource_count: c.resources.read().len(), prompt_count: c.prompts.read().len(), transport: c.config.transport, source: c.config.source.clone(),
        }).collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }
    /// Tell the windows that see a layer that it changed: a workspace root's windows, or every window for the user layer.
    fn broadcast(&self, root: Option<&Path>) {
        match root {
            Some(r) => events::emit_for_root(&self.app, r, events::MCP_CHANGED, self.statuses(Some(r))),
            None => events::emit(&self.app, events::MCP_CHANGED, self.statuses(None)),
        }
    }
    fn broadcast_conn(&self, conn: &Conn) { self.broadcast(conn.root.as_deref()); }

    /// Start the workspace layer of `root` (first window on that folder). A no-op when it is already open.
    pub async fn open_root(self: &Arc<Self>, root: &Path) {
        if self.workspaces.contains_key(root) { return; }
        self.workspaces.insert(root.to_path_buf(), ServerSet::new(Some(root.to_path_buf())));
        self.reload_root(root).await;
    }
    /// Stop the workspace layer of `root` (its last window left) and drop its connections.
    pub async fn close_root(self: &Arc<Self>, root: &Path) {
        let Some((_, set)) = self.workspaces.remove(root) else { return };
        let conns = set.all();
        for conn in &conns {
            if let Some(mut r) = conn.running.lock().await.take() { let _ = tokio::time::timeout(Duration::from_secs(5), r.close()).await; }
            *conn.peer.write() = None;
            *conn.state.write() = "stopped".into();
        }
        tracing::info!(root = %root.display(), servers = conns.len(), "mcp workspace servers stopped: last window left the folder");
    }

    /// Reload the user layer: connect new/changed servers, drop removed ones.
    pub async fn reload_user(self: &Arc<Self>) { let configs = self.configs_user(); self.reconcile(&self.user.clone(), configs); }
    /// Reload the workspace layer of `root` (nothing when that root is not open).
    pub async fn reload_root(self: &Arc<Self>, root: &Path) {
        let Some(set) = self.workspaces.get(root).map(|s| s.clone()) else { return };
        let configs = self.configs_workspace(root);
        self.reconcile(&set, configs);
    }
    /// Reload whatever a config change of `scope` affects: the user layer, or every open workspace layer (the
    /// enable/disable state file is keyed by id, so `project:fs` toggles in every root that defines it).
    pub async fn reload_scope(self: &Arc<Self>, scope: &str) {
        if config::is_workspace_scope(scope) { for root in self.workspaces.iter().map(|e| e.key().clone()).collect::<Vec<_>>() { self.reload_root(&root).await; } } else { self.reload_user().await; }
    }

    fn reconcile(self: &Arc<Self>, set: &Arc<ServerSet>, configs: Vec<McpServerConfig>) {
        let enabled_mcp = self.app.state::<SettingsState>().effective().mcp.enabled;
        let mut to_connect = vec![];
        {
            let mut conns = set.conns.write();
            conns.retain(|id, _| configs.iter().any(|c| &c.id == id));
            for c in configs {
                let changed = conns.get(&c.id).map(|existing| serde_json::to_string(&existing.config).ok() != serde_json::to_string(&c).ok()).unwrap_or(true);
                if changed {
                    let conn = Arc::new(Conn { config: c.clone(), root: set.root.clone(), state: RwLock::new(if c.enabled && enabled_mcp { "connecting".into() } else { "disabled".into() }), error: RwLock::new(None), running: tokio::sync::Mutex::new(None), peer: RwLock::new(None), tools: RwLock::new(vec![]), resources: RwLock::new(vec![]), prompts: RwLock::new(vec![]), attempts: Default::default() });
                    conns.insert(c.id.clone(), conn.clone());
                    if c.enabled && enabled_mcp { to_connect.push(conn); }
                }
            }
        }
        self.broadcast(set.root.as_deref());
        for conn in to_connect { self.spawn_connect(conn); }
    }

    /// Spawn a connection attempt loop (up to 4 attempts with 1s/3s/9s backoff).
    fn spawn_connect(self: &Arc<Self>, conn: Arc<Conn>) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut attempt = 0u32;
            loop {
                if !me.is_live(&conn) { return; }
                if me.connect_once(&conn).await { return; }
                attempt += 1;
                if attempt > 3 { return; }
                tokio::time::sleep(Duration::from_secs(3u64.pow(attempt - 1))).await;
            }
        });
    }

    /// One connection attempt; returns true when connected.
    async fn connect_once(self: &Arc<Self>, conn: &Arc<Conn>) -> bool {
        *conn.state.write() = "connecting".into();
        *conn.error.write() = None;
        self.broadcast_conn(conn);
        match self.try_connect(conn).await {
            Ok(()) => {
                conn.attempts.store(0, std::sync::atomic::Ordering::SeqCst);
                *conn.state.write() = "connected".into();
                self.broadcast_conn(conn);
                self.refresh_catalog(conn.root.as_deref(), &conn.config.id).await;
                self.spawn_monitor(conn.clone());
                true
            }
            Err(e) => {
                self.log_conn(conn, format!("connect failed: {}", e.message));
                conn.attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                *conn.state.write() = "error".into(); *conn.error.write() = Some(e.message.clone());
                self.broadcast_conn(conn);
                false
            }
        }
    }

    async fn try_connect(&self, conn: &Arc<Conn>) -> Result<()> {
        let cfg = &conn.config;
        let handler = Handler { server_id: cfg.id.clone(), root: conn.root.clone(), mgr: Arc::downgrade(&self_arc(self)) };
        let running = match cfg.transport {
            McpTransport::Stdio => {
                let command = cfg.command.clone().ok_or_else(|| CaretError::config("stdio server needs a command"))?;
                let path = augmented_path();
                let cwd = cfg.cwd.clone().map(PathBuf::from).or_else(|| conn.root.clone()).unwrap_or_else(crate::paths::home_dir);
                let resolved = which::which_in(&command, Some(&path), &cwd).map_err(|_| CaretError::config(format!("command not found: {command} (PATH searched: {path})")))?;
                let mut cmd = tokio::process::Command::new(resolved);
                cmd.args(cfg.args.clone().unwrap_or_default()).current_dir(&cwd).env("PATH", &path).kill_on_drop(true);
                if let Some(env) = &cfg.env { cmd.envs(env); }
                let (transport, stderr) = TokioChildProcess::builder(cmd).stderr(std::process::Stdio::piped()).spawn().map_err(|e| CaretError::io(format!("spawn {command}: {e}")))?;
                if let Some(stderr) = stderr {
                    let id = cfg.id.clone(); let root = conn.root.clone(); let me = self_arc(self);
                    tauri::async_runtime::spawn(async move { use tokio::io::AsyncBufReadExt; let mut lines = tokio::io::BufReader::new(stderr).lines(); while let Ok(Some(l)) = lines.next_line().await { me.log(root.as_deref(), &id, l); } });
                }
                tokio::time::timeout(Duration::from_secs(60), handler.serve(transport)).await.map_err(|_| CaretError::new("timeout", "MCP initialize timed out"))?.map_err(|e| CaretError::provider(format!("MCP initialize failed: {e}")))?
            }
            McpTransport::Http | McpTransport::Sse => {
                let url = cfg.url.clone().ok_or_else(|| CaretError::config("remote server needs a url"))?;
                let mut tcfg = StreamableHttpClientTransportConfig::with_uri(url.clone());
                let mut hm: HashMap<reqwest::header::HeaderName, reqwest::header::HeaderValue> = HashMap::new();
                for (k, v) in cfg.headers.clone().unwrap_or_default() {
                    if k.eq_ignore_ascii_case("authorization") { tcfg = tcfg.auth_header(v.strip_prefix("Bearer ").unwrap_or(&v).to_string()); continue; }
                    if let (Ok(n), Ok(val)) = (reqwest::header::HeaderName::from_bytes(k.as_bytes()), reqwest::header::HeaderValue::from_str(&v)) { hm.insert(n, val); }
                }
                tcfg = tcfg.custom_headers(hm);
                let client = reqwest::Client::builder().user_agent(concat!("Caret/", env!("CARGO_PKG_VERSION"))).build()?;
                let transport = StreamableHttpClientTransport::with_client(client, tcfg);
                match tokio::time::timeout(Duration::from_secs(45), handler.serve(transport)).await {
                    Err(_) => return Err(CaretError::new("timeout", "MCP initialize timed out")),
                    Ok(Err(e)) => {
                        let msg = e.to_string();
                        if msg.contains("401") || msg.to_ascii_lowercase().contains("unauthorized") || msg.to_ascii_lowercase().contains("auth") { return Err(CaretError::provider(format!("Server requires authentication (OAuth is not supported yet; add an Authorization header): {msg}"))); }
                        if cfg.transport == McpTransport::Sse { return Err(CaretError::provider(format!("Legacy SSE transport is not supported; point to a streamable HTTP endpoint. ({msg})"))); }
                        return Err(CaretError::provider(format!("MCP connect failed: {msg}")));
                    }
                    Ok(Ok(r)) => r,
                }
            }
        };
        *conn.peer.write() = Some(running.peer().clone());
        *conn.running.lock().await = Some(running);
        Ok(())
    }

    fn spawn_monitor(self: &Arc<Self>, conn: Arc<Conn>) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if !me.is_live(&conn) { break; }
                let closed = conn.peer.read().as_ref().map(|p| p.is_transport_closed()).unwrap_or(true);
                if closed && *conn.state.read() == "connected" {
                    *conn.state.write() = "disconnected".into(); *conn.error.write() = Some("transport closed".into());
                    *conn.peer.write() = None; *conn.running.lock().await = None;
                    me.broadcast_conn(&conn); me.log_conn(&conn, "disconnected; reconnecting".into());
                    me.spawn_connect(conn.clone());
                    break;
                }
            }
        });
    }

    pub async fn refresh_catalog(&self, root: Option<&Path>, id: &str) {
        let Some(conn) = self.conn(root, id) else { return };
        let Some(peer) = conn.peer.read().clone() else { return };
        if let Ok(t) = tokio::time::timeout(Duration::from_secs(30), peer.list_all_tools()).await { match t { Ok(t) => *conn.tools.write() = t, Err(e) => self.log_conn(&conn, format!("tools/list: {e}")) } }
        if let Ok(Ok(r)) = tokio::time::timeout(Duration::from_secs(15), peer.list_all_resources()).await { *conn.resources.write() = r; }
        if let Ok(Ok(p)) = tokio::time::timeout(Duration::from_secs(15), peer.list_all_prompts()).await { *conn.prompts.write() = p; }
        self.broadcast_conn(&conn);
    }

    fn peer_for(&self, root: Option<&Path>, id: &str) -> Result<(Arc<Conn>, rmcp::Peer<RoleClient>)> {
        let conn = self.conn(root, id).ok_or_else(|| CaretError::not_found(format!("MCP server {id}")))?;
        let peer = conn.peer.read().clone().ok_or_else(|| CaretError::new("mcp_disconnected", format!("MCP server '{}' is not connected ({})", conn.config.name, conn.state.read())))?;
        Ok((conn, peer))
    }

    pub fn tools(&self, root: Option<&Path>) -> Vec<McpToolInfo> {
        let mut v = vec![];
        for c in self.conns_for(root) {
            let disabled = c.config.disabled_tools.clone().unwrap_or_default();
            for t in c.tools.read().iter() {
                v.push(McpToolInfo { server_id: c.config.id.clone(), name: t.name.to_string(), qualified_name: config::qualified_tool_name(&c.config.name, &t.name), description: t.description.as_ref().map(|d| d.to_string()), input_schema: Value::Object((*t.input_schema).clone()), enabled: !disabled.contains(&t.name.to_string()) });
            }
        }
        v.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        v
    }
    pub fn resources(&self, root: Option<&Path>, server_id: Option<&str>) -> Vec<McpResourceInfo> {
        self.conns_for(root).iter().filter(|c| server_id.map(|s| s == c.config.id).unwrap_or(true)).flat_map(|c| c.resources.read().iter().map(|r| McpResourceInfo { server_id: c.config.id.clone(), uri: r.uri.clone(), name: r.name.clone(), description: r.description.clone(), mime_type: r.mime_type.clone() }).collect::<Vec<_>>()).collect()
    }
    pub fn prompts(&self, root: Option<&Path>) -> Vec<McpPromptInfo> {
        self.conns_for(root).iter().flat_map(|c| c.prompts.read().iter().map(|p| McpPromptInfo { server_id: c.config.id.clone(), name: p.name.clone(), description: p.description.clone(), arguments: p.arguments.as_ref().map(|a| a.iter().map(|x| McpPromptArg { name: x.name.clone(), description: x.description.clone(), required: x.required }).collect()) }).collect::<Vec<_>>()).collect()
    }

    pub async fn call_tool(&self, root: Option<&Path>, server_id: &str, name: &str, args: Value) -> Result<McpCallResult> {
        let (conn, peer) = self.peer_for(root, server_id)?;
        if conn.config.disabled_tools.as_ref().map(|d| d.iter().any(|t| t == name)).unwrap_or(false) { return Err(CaretError::denied(format!("tool '{name}' is disabled"))); }
        let mut p = CallToolRequestParams::new(name.to_string());
        p.arguments = args.as_object().cloned();
        self.log_conn(&conn, format!("tools/call {name}"));
        let res = tokio::time::timeout(Duration::from_secs(60), peer.call_tool(p)).await.map_err(|_| CaretError::new("timeout", format!("MCP tool '{name}' timed out after 60s")))?.map_err(|e| CaretError::provider(format!("MCP tool '{name}' failed: {e}")))?;
        let mut content: Vec<McpContent> = res.content.iter().map(convert_block).collect();
        if content.is_empty() { if let Some(sc) = &res.structured_content { content.push(McpContent { r#type: "text".into(), text: Some(serde_json::to_string_pretty(sc).unwrap_or_default()), data: None, mime_type: Some("application/json".into()), uri: None }); } }
        Ok(McpCallResult { content, is_error: res.is_error.unwrap_or(false) })
    }
    pub async fn read_resource(&self, root: Option<&Path>, server_id: &str, uri: &str) -> Result<McpCallResult> {
        let (_c, peer) = self.peer_for(root, server_id)?;
        let res = tokio::time::timeout(Duration::from_secs(30), peer.read_resource(ReadResourceRequestParams::new(uri.to_string()))).await.map_err(|_| CaretError::new("timeout", "resources/read timed out"))?.map_err(|e| CaretError::provider(format!("resources/read failed: {e}")))?;
        Ok(McpCallResult { content: res.contents.iter().map(convert_resource).collect(), is_error: false })
    }
    pub async fn get_prompt(&self, root: Option<&Path>, server_id: &str, name: &str, args: HashMap<String, String>) -> Result<Vec<McpPromptMessage>> {
        let (_c, peer) = self.peer_for(root, server_id)?;
        let mut p = GetPromptRequestParams::new(name.to_string());
        if !args.is_empty() { p.arguments = Some(args.into_iter().map(|(k, v)| (k, Value::String(v))).collect()); }
        let res = tokio::time::timeout(Duration::from_secs(30), peer.get_prompt(p)).await.map_err(|_| CaretError::new("timeout", "prompts/get timed out"))?.map_err(|e| CaretError::provider(format!("prompts/get failed: {e}")))?;
        Ok(res.messages.iter().map(|m| McpPromptMessage { role: format!("{:?}", m.role).to_lowercase(), text: convert_block(&m.content).text.unwrap_or_default() }).collect())
    }

    pub async fn restart(self: &Arc<Self>, root: Option<&Path>, id: &str) -> Result<()> {
        let conn = self.conn(root, id).ok_or_else(|| CaretError::not_found(format!("MCP server {id}")))?;
        if let Some(mut r) = conn.running.lock().await.take() { let _ = tokio::time::timeout(Duration::from_secs(5), r.close()).await; }
        *conn.peer.write() = None;
        conn.attempts.store(0, std::sync::atomic::Ordering::SeqCst);
        if conn.config.enabled { self.spawn_connect(conn); } else { *conn.state.write() = "disabled".into(); self.broadcast_conn(&conn); }
        Ok(())
    }
}

/// `Arc<Self>` for methods that only have `&self` (the inner is always Arc-held by the manager).
fn self_arc(me: &McpInner) -> Arc<McpInner> {
    // Safety-free approach: recover the Arc through the managed state.
    me.app.state::<McpManager>().inner.clone()
}

fn convert_block(b: &ContentBlock) -> McpContent {
    match b {
        ContentBlock::Text(t) => McpContent { r#type: "text".into(), text: Some(t.text.clone()), data: None, mime_type: None, uri: None },
        ContentBlock::Image(i) => McpContent { r#type: "image".into(), text: None, data: Some(i.data.clone()), mime_type: Some(i.mime_type.clone()), uri: None },
        ContentBlock::Resource(r) => convert_resource(&r.resource),
        ContentBlock::ResourceLink(r) => McpContent { r#type: "resource".into(), text: Some(format!("{} ({})", r.name, r.uri)), data: None, mime_type: r.mime_type.clone(), uri: Some(r.uri.clone()) },
        other => McpContent { r#type: "text".into(), text: Some(serde_json::to_string(other).unwrap_or_default()), data: None, mime_type: None, uri: None },
    }
}
fn convert_resource(r: &ResourceContents) -> McpContent {
    match r {
        ResourceContents::TextResourceContents { uri, mime_type, text, .. } => McpContent { r#type: "resource".into(), text: Some(text.clone()), data: None, mime_type: mime_type.clone(), uri: Some(uri.clone()) },
        ResourceContents::BlobResourceContents { uri, mime_type, blob, .. } => McpContent { r#type: "resource".into(), text: None, data: Some(blob.clone()), mime_type: mime_type.clone(), uri: Some(uri.clone()) },
        #[allow(unreachable_patterns)]
        _ => McpContent { r#type: "resource".into(), text: None, data: None, mime_type: None, uri: None },
    }
}

pub struct McpPortImpl(pub Arc<McpInner>);

#[async_trait::async_trait]
impl McpPort for McpPortImpl {
    async fn list_tools(&self, root: Option<&Path>) -> Vec<McpToolDesc> {
        let names: HashMap<String, String> = self.0.conns_for(root).iter().map(|c| (c.config.id.clone(), c.config.name.clone())).collect();
        self.0.tools(root).into_iter().filter(|t| t.enabled).map(|t| McpToolDesc { server_name: names.get(&t.server_id).cloned().unwrap_or_default(), server_id: t.server_id, name: t.name, qualified_name: t.qualified_name, description: t.description.unwrap_or_default(), input_schema: t.input_schema, enabled: t.enabled }).collect()
    }
    async fn call_tool(&self, root: Option<&Path>, server_id: &str, name: &str, args: Value) -> Result<McpCallResult> { self.0.call_tool(root, server_id, name, args).await }
    async fn list_resources(&self, root: Option<&Path>) -> Vec<McpResourceDesc> { self.0.resources(root, None).into_iter().map(|r| McpResourceDesc { server_id: r.server_id, uri: r.uri, name: r.name, description: r.description, mime_type: r.mime_type }).collect() }
    async fn read_resource(&self, root: Option<&Path>, server_id: &str, uri: &str) -> Result<McpCallResult> { self.0.read_resource(root, server_id, uri).await }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[test]
    fn log_keys_separate_roots_for_workspace_servers() {
        assert_eq!(log_key(None, "user:github"), "user:github");
        assert_eq!(log_key(Some(Path::new("/work/a")), "project:fs"), "/work/a|project:fs");
        assert_ne!(log_key(Some(Path::new("/work/a")), "project:fs"), log_key(Some(Path::new("/work/b")), "project:fs"));
    }

    /// Live handshake against the reference "everything" server (needs network for npx).
    /// Run: CARET_LIVE_TEST=1 cargo test --lib mcp::manager::tests::live_everything_server -- --nocapture
    #[tokio::test]
    async fn live_everything_server() {
        if std::env::var("CARET_LIVE_TEST").ok().as_deref() != Some("1") { eprintln!("skipped (set CARET_LIVE_TEST=1)"); return; }
        let path = augmented_path();
        let Ok(npx) = which::which_in("npx", Some(&path), std::env::temp_dir()) else { eprintln!("npx not found; skipping"); return };
        let mut cmd = tokio::process::Command::new(npx);
        cmd.args(["-y", "@modelcontextprotocol/server-everything"]).env("PATH", &path).kill_on_drop(true);
        let (transport, _stderr) = TokioChildProcess::builder(cmd).stderr(Stdio::piped()).spawn().expect("spawn");
        let handler = Handler { server_id: "test".into(), root: None, mgr: Weak::new() };
        let running = tokio::time::timeout(Duration::from_secs(120), handler.serve(transport)).await.expect("timeout").expect("initialize");
        let tools = running.peer().list_all_tools().await.expect("tools");
        assert!(tools.iter().any(|t| t.name == "echo"), "echo tool missing: {:?}", tools.iter().map(|t| t.name.to_string()).collect::<Vec<_>>());
        let mut p = CallToolRequestParams::new("echo");
        p.arguments = serde_json::json!({"message": "hi caret"}).as_object().cloned();
        let res = running.peer().call_tool(p).await.expect("call");
        let text = res.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect::<Vec<_>>().join("");
        assert!(text.contains("hi caret"), "unexpected echo result: {text}");
        let prompts = running.peer().list_all_prompts().await.unwrap_or_default();
        let resources = running.peer().list_all_resources().await.unwrap_or_default();
        eprintln!("tools={} prompts={} resources={}", tools.len(), prompts.len(), resources.len());
        let mut running = running;
        let _ = tokio::time::timeout(Duration::from_secs(5), running.close()).await;
    }
}

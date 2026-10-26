//! ACP ↔ Caret mapping. One `SessionLink` per Caret chat session that is driven by an external agent.
//!
//! * Caret `SendMessageRequest` → `session/prompt` (text + attached context + images)
//! * `session/update` chunks → `TextDelta` / `ThinkingDelta` / `ToolStart` / `ToolUpdate` / `Todos` / `Usage`
//! * `session/request_permission` → `AgentManager::request_approval` (same approval UI as the built-in agent)
//! * `fs/write_text_file` and `diff` tool content → checkpoint snapshot + `PendingChange` (review bar / proof panel)
//! * `terminal/*` → child processes owned by Caret (output surfaces on the tool card)
use crate::acp::client::{conn_key, AgentConn};
use crate::acp::protocol::*;
use crate::acp::{registry, AcpAgentConfig, AcpAgentStatus, AcpAuthMethod, AcpCapabilities, AcpInner, AcpMode};
use crate::ai::agent::manager::{AgentManager, SessionHandle};
use crate::ai::agent::session::*;
use crate::error::{CaretError, Result};
use dashmap::DashMap;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;

pub struct SessionLink {
    pub caret_id: String,
    pub acp_id: String,
    pub agent_id: String,
    pub conn_key: String,
    pub message_id: Mutex<Option<String>>,
    pub turn_id: Mutex<Option<String>>,
    pub modes: Mutex<Option<SessionModeState>>,
    /// Session config options (model, effort, …) as last advertised by the agent.
    pub config_options: Mutex<Option<Vec<ConfigOption>>>,
    pub commands: Mutex<Vec<Value>>,
    /// tool_call_id → message id that holds the card.
    pub tool_msg: DashMap<String, String>,
    /// `path` → [(hash(after), change_id)] of changes recorded this turn, oldest first (dedupes fs/write + diff content).
    pub registered: DashMap<String, Vec<(u64, String)>>,
    /// Approval keys the user allowed for the rest of this session.
    pub allow_memory: DashMap<String, ()>,
    /// Diff blocks announced while a tool call was still pending/in progress (e.g. inside a permission request),
    /// recorded once the call completes: `tool_call_id` → [(path, oldText, newText)].
    pub pending_diffs: DashMap<String, Vec<(String, Option<String>, String)>>,
    pub cancel: Mutex<CancellationToken>,
    pub turn_started: Mutex<Option<Instant>>,
}

pub struct TermProc {
    pub id: String,
    pub caret_id: String,
    pub command_line: String,
    pub cwd: String,
    pub pid: Option<u32>,
    pub buf: Mutex<Vec<u8>>,
    pub truncated: AtomicBool,
    pub limit: usize,
    pub exit: Mutex<Option<(Option<i32>, Option<String>)>>,
    pub done: tokio::sync::Notify,
    pub finished: AtomicBool,
    pub tool: Mutex<Option<(String, String)>>,
}
impl TermProc {
    fn output(&self) -> String { String::from_utf8_lossy(&self.buf.lock()).to_string() }
    fn append(&self, bytes: &[u8]) {
        let mut b = self.buf.lock();
        b.extend_from_slice(bytes);
        if b.len() > self.limit { let cut = b.len() - self.limit; b.drain(..cut); self.truncated.store(true, Ordering::SeqCst); }
    }
}

pub struct Bridge { pub inner: Arc<AcpInner>, pub mgr: Arc<AgentManager>, pub agent_id: String }

fn hash_str(s: &str) -> u64 { use std::hash::{Hash, Hasher}; let mut h = std::collections::hash_map::DefaultHasher::new(); s.hash(&mut h); h.finish() }

pub fn unified_diff(path: &str, before: &str, after: &str) -> (String, u32, u32) {
    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::from_lines(before, after);
    let (mut add, mut del) = (0u32, 0u32);
    for c in diff.iter_all_changes() { match c.tag() { ChangeTag::Insert => add += 1, ChangeTag::Delete => del += 1, _ => {} } }
    (diff.unified_diff().context_radius(3).header(&format!("a/{path}"), &format!("b/{path}")).to_string(), add, del)
}

fn display_path(root: Option<&Path>, p: &Path) -> String { match root { Some(r) => p.strip_prefix(r).map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|_| p.to_string_lossy().to_string()), None => p.to_string_lossy().to_string() } }

fn map_tool_status(s: &str) -> ToolStatus { match s { "pending" => ToolStatus::Pending, "in_progress" => ToolStatus::Running, "completed" => ToolStatus::Done, "failed" => ToolStatus::Error, _ => ToolStatus::Running } }
fn map_kind_display(kind: Option<&str>, title: Option<&str>) -> (String, String) {
    let name = kind.unwrap_or("other").to_string();
    let display = title.filter(|t| !t.trim().is_empty()).map(|t| t.to_string()).unwrap_or_else(|| match name.as_str() { "read" => "Read", "edit" => "Edit", "delete" => "Delete", "move" => "Move", "search" => "Search", "execute" => "Run command", "think" => "Think", "fetch" => "Fetch", "switch_mode" => "Switch mode", _ => "Tool" }.to_string());
    (format!("acp_{name}"), display)
}

pub fn status_for(inner: &AcpInner, cfg: &AcpAgentConfig, root: Option<&Path>) -> AcpAgentStatus {
    let installed = crate::acp::process::is_installed(cfg);
    let key = root.map(|r| conn_key(&cfg.id, r));
    let live = key.as_ref().and_then(|k| inner.conns.get(k)).filter(|c| c.alive()).map(|c| status_from_conn(&c));
    if let Some(s) = live { return s; }
    let mut st = inner.probes.get(&cfg.id).map(|p| p.clone()).unwrap_or(AcpAgentStatus { id: cfg.id.clone(), name: cfg.name.clone(), installed, state: "idle".into(), version: None, protocol_version: None, error: None, auth_required: None, auth_methods: None, capabilities: None, modes: None, current_mode: None, config_options: None });
    st.installed = installed; st.name = cfg.name.clone();
    if !cfg.enabled { st.state = "idle".into(); }
    if !installed { st.state = "unsupported".into(); st.error = Some(cfg.install_hint.clone().map(|h| format!("Not installed. {h}")).unwrap_or_else(|| format!("`{}` not found on PATH", cfg.command))); }
    else if st.state == "ready" { st.state = "idle".into(); }
    st
}
pub fn status_from_conn(c: &AgentConn) -> AcpAgentStatus {
    let caps = &c.init.agent_capabilities;
    AcpAgentStatus { id: c.agent_id.clone(), name: c.init.agent_info.as_ref().and_then(|a| a.title.clone()).unwrap_or_else(|| c.config.name.clone()), installed: true, state: "ready".into(), version: c.init.agent_info.as_ref().and_then(|a| a.version.clone()), protocol_version: Some(c.init.protocol_version), error: None, auth_required: None, auth_methods: if c.init.auth_methods.is_empty() { None } else { Some(c.init.auth_methods.iter().map(|m| AcpAuthMethod { id: m.id.clone(), name: m.name.clone(), description: m.description.clone() }).collect()) }, capabilities: Some(AcpCapabilities { load_session: Some(caps.load_session), prompt_images: Some(caps.prompt_capabilities.image), mcp: Some(caps.mcp_capabilities.http || caps.mcp_capabilities.sse || true) }), modes: None, current_mode: None, config_options: None }
}

fn make_log(inner: Arc<AcpInner>, agent_id: String) -> Arc<dyn Fn(String) + Send + Sync> {
    Arc::new(move |line: String| { tracing::debug!(agent=%agent_id, "{line}"); inner.log(&agent_id, line); })
}

/// Connect (or reuse) the agent process for this workspace.
pub async fn ensure_conn(inner: &Arc<AcpInner>, mgr: &Arc<AgentManager>, agent_id: &str) -> Result<Arc<AgentConn>> {
    let settings = mgr.settings();
    if !settings.acp.enabled { return Err(CaretError::config("External agents are disabled in Settings → Agents")); }
    let cfg = inner.overrides.get(agent_id).map(|c| c.clone()).or_else(|| registry::find_config(&settings, agent_id)).ok_or_else(|| CaretError::not_found(format!("agent {agent_id}")))?;
    if !cfg.enabled { return Err(CaretError::config(format!("{} is disabled in Settings → Agents", cfg.name))); }
    let root = mgr.workspace_root().unwrap_or_else(crate::paths::home_dir);
    let key = conn_key(agent_id, &root);
    if let Some(c) = inner.conns.get(&key) { if c.alive() { return Ok(c.clone()); } }
    inner.conns.remove(&key);
    inner.probes.insert(agent_id.to_string(), AcpAgentStatus { id: agent_id.into(), name: cfg.name.clone(), installed: true, state: "starting".into(), version: None, protocol_version: None, error: None, auth_required: None, auth_methods: None, capabilities: None, modes: None, current_mode: None, config_options: None });
    let handler: Arc<dyn IncomingHandler> = Arc::new(Bridge { inner: inner.clone(), mgr: mgr.clone(), agent_id: agent_id.to_string() });
    match AgentConn::connect(cfg.clone(), root, handler, make_log(inner.clone(), agent_id.to_string())).await {
        Ok(conn) => { inner.conns.insert(key, conn.clone()); inner.probes.insert(agent_id.to_string(), status_from_conn(&conn)); Ok(conn) }
        Err(e) => { inner.probes.insert(agent_id.to_string(), AcpAgentStatus { id: agent_id.into(), name: cfg.name.clone(), installed: crate::acp::process::is_installed(&cfg), state: "error".into(), version: None, protocol_version: None, error: Some(e.message.clone()), auth_required: None, auth_methods: None, capabilities: None, modes: None, current_mode: None, config_options: None }); Err(e) }
    }
}

fn mcp_servers(app: Option<&AppHandle>, caps: &McpCapabilities) -> Vec<Value> {
    let Some(app) = app else { return vec![] };
    let Some(m) = app.try_state::<crate::mcp::McpManager>() else { return vec![] };
    m.inner.configs().into_iter().filter(|c| c.enabled).filter_map(|c| {
        let env: Vec<Value> = c.env.clone().unwrap_or_default().into_iter().map(|(k, v)| json!({ "name": k, "value": v })).collect();
        match c.transport {
            crate::mcp::config::McpTransport::Stdio => c.command.clone().map(|cmd| json!({ "name": c.name, "command": cmd, "args": c.args.clone().unwrap_or_default(), "env": env })),
            crate::mcp::config::McpTransport::Http if caps.http => c.url.clone().map(|u| json!({ "type": "http", "name": c.name, "url": u, "headers": c.headers.clone().unwrap_or_default().into_iter().map(|(k, v)| json!({ "name": k, "value": v })).collect::<Vec<_>>() })),
            crate::mcp::config::McpTransport::Sse if caps.sse => c.url.clone().map(|u| json!({ "type": "sse", "name": c.name, "url": u, "headers": c.headers.clone().unwrap_or_default().into_iter().map(|(k, v)| json!({ "name": k, "value": v })).collect::<Vec<_>>() })),
            _ => None,
        }
    }).collect()
}

fn modes_to_status(st: &mut AcpAgentStatus, modes: &Option<SessionModeState>) {
    if let Some(m) = modes { st.modes = Some(m.available_modes.iter().map(|x| AcpMode { id: x.id.clone(), name: x.name.clone(), description: x.description.clone() }).collect()); st.current_mode = Some(m.current_mode_id.clone()); }
}

/// Connection status plus this session's modes and config options (what the composer's pickers render).
pub fn status_for_link(conn: &AgentConn, link: &SessionLink) -> AcpAgentStatus {
    let mut st = status_from_conn(conn); modes_to_status(&mut st, &link.modes.lock());
    st.config_options = link.config_options.lock().clone();
    st
}

fn emit_agent_status(inner: &AcpInner, mgr: &AgentManager, link: &SessionLink, conn: &AgentConn) {
    let st = status_for_link(conn, link);
    inner.probes.insert(link.agent_id.clone(), st.clone());
    mgr.emit(AgentEvent::AgentStatus { session_id: link.caret_id.clone(), agent_id: link.agent_id.clone(), status: st });
}

/// Create (or reuse) the ACP session bound to this Caret session. Returns `(link, newly_created)`.
pub async fn ensure_session(inner: &Arc<AcpInner>, mgr: &Arc<AgentManager>, app: Option<&AppHandle>, h: &Arc<SessionHandle>, conn: &Arc<AgentConn>) -> Result<(Arc<SessionLink>, bool)> {
    let caret_id = h.id();
    if let Some(l) = inner.links.get(&caret_id) { if l.conn_key == conn.key && conn.alive() { return Ok((l.clone(), false)); } }
    let cwd = h.session.lock().cwd_path();
    let settings = mgr.settings();
    let mcp = if settings.acp.pass_mcp_servers { mcp_servers(app, &conn.init.agent_capabilities.mcp_capabilities) } else { vec![] };
    let res = match conn.new_session(&cwd, mcp).await {
        Ok(r) => r,
        Err(e) if e.code == RpcError::AUTH_REQUIRED => {
            let methods: Vec<String> = conn.init.auth_methods.iter().map(|m| format!("{} ({})", m.name, m.id)).collect();
            let mut st = status_from_conn(conn); st.auth_required = Some(true); st.state = "error".into(); st.error = Some("Authentication required".into());
            inner.probes.insert(conn.agent_id.clone(), st.clone());
            mgr.emit(AgentEvent::AgentStatus { session_id: caret_id.clone(), agent_id: conn.agent_id.clone(), status: st });
            return Err(CaretError::denied(format!("{} needs authentication. Open Settings → Agents and sign in{}.", conn.config.name, if methods.is_empty() { String::new() } else { format!(" ({})", methods.join(", ")) })));
        }
        Err(e) => return Err(CaretError::provider(format!("{}: session/new failed: {}", conn.config.name, e.message))),
    };
    let link = Arc::new(SessionLink { caret_id: caret_id.clone(), acp_id: res.session_id.clone(), agent_id: conn.agent_id.clone(), conn_key: conn.key.clone(), message_id: Mutex::new(None), turn_id: Mutex::new(None), modes: Mutex::new(res.modes.clone()), config_options: Mutex::new(res.config_options.clone()), commands: Mutex::new(vec![]), tool_msg: DashMap::new(), registered: DashMap::new(), allow_memory: DashMap::new(), pending_diffs: DashMap::new(), cancel: Mutex::new(CancellationToken::new()), turn_started: Mutex::new(None) });
    inner.links.insert(caret_id.clone(), link.clone());
    inner.by_acp.insert(res.session_id.clone(), caret_id);
    // Apply the options the user last picked for this agent (model, effort, …) to the fresh session.
    if let Some(defaults) = settings.acp.session_defaults.get(&conn.agent_id) {
        for (cid, val) in defaults {
            let applicable = link.config_options.lock().as_ref().map(|o| o.iter().any(|x| x.id == *cid && x.current_value != *val)).unwrap_or(false);
            if !applicable { continue; }
            match conn.set_config_option(&res.session_id, cid, val).await {
                Ok(Some(opts)) => { *link.config_options.lock() = Some(opts); }
                Ok(None) => { if let Some(opts) = link.config_options.lock().as_mut() { if let Some(o) = opts.iter_mut().find(|o| o.id == *cid) { o.current_value = val.clone(); } } }
                Err(e) => inner.log(&conn.agent_id, format!("[acp] could not apply remembered option {cid}={val}: {e:?}")),
            }
        }
    }
    emit_agent_status(inner, mgr, &link, conn);
    // Replay updates (available commands, mode) the agent sent before the link existed.
    if let Some((_, early)) = inner.pending_updates.remove(&res.session_id) {
        let b = Bridge { inner: inner.clone(), mgr: mgr.clone(), agent_id: conn.agent_id.clone() };
        for params in early { b.on_notification("session/update", params).await; }
    }
    Ok((link, true))
}

/// Summarised prior transcript for a fresh ACP session bound to a Caret session that already has history.
fn history_block(s: &Session) -> Option<String> {
    let mut out = String::new();
    for m in s.messages.iter().rev().take(12).collect::<Vec<_>>().into_iter().rev() {
        let role = match m.role { MessageRole::User => "User", MessageRole::Assistant => "Assistant", MessageRole::System => continue };
        let text: String = m.parts.iter().filter_map(|p| match p { MessagePart::Text { text } => Some(text.as_str()), MessagePart::Tool { call } => Some(call.display_name.as_str()), _ => None }).collect::<Vec<_>>().join("\n");
        if text.trim().is_empty() { continue; }
        let t: String = text.chars().take(1200).collect();
        out.push_str(&format!("{role}: {t}\n\n"));
        if out.len() > 7000 { break; }
    }
    if out.trim().is_empty() { None } else { Some(format!("<previous_conversation note=\"earlier turns of this chat, for continuity; the editor restarted\">\n{out}</previous_conversation>")) }
}

/// Queue-aware entry point used by `chat_send` for sessions driven by an ACP agent.
pub async fn submit(inner: Arc<AcpInner>, mgr: Arc<AgentManager>, app: Option<AppHandle>, req: SendMessageRequest) -> Result<()> {
    let h = mgr.handle(&req.session_id)?;
    if h.is_running() {
        let q = QueuedMessage { id: new_id(), text: req.text.clone(), context: req.context.clone(), images: req.images.clone(), mode: Some(req.mode.clone()), request: Some(req) };
        let queued = { let mut s = h.session.lock(); s.queued.push(q); s.queued.clone() };
        mgr.emit(AgentEvent::Queue { session_id: h.id(), queued });
        mgr.persist(&h);
        return Ok(());
    }
    h.running.store(true, Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        let mut next = Some(req);
        while let Some(r) = next.take() {
            let cancel = h.reset_token();
            if let Err(e) = run_turn(&inner, &mgr, app.as_ref(), &h, r, cancel).await { tracing::warn!(error=%e, "acp turn failed"); }
            let q = { let mut s = h.session.lock(); if s.queued.is_empty() { None } else { Some(s.queued.remove(0)) } };
            if let Some(q) = q {
                mgr.emit(AgentEvent::Queue { session_id: h.id(), queued: h.session.lock().queued.clone() });
                next = Some(q.request.unwrap_or(SendMessageRequest { session_id: h.id(), text: q.text, context: q.context, images: q.images, mode: q.mode.unwrap_or_else(|| h.session.lock().mode.clone()), ..Default::default() }));
            }
        }
        h.running.store(false, Ordering::SeqCst);
        mgr.set_status(&h, SessionStatus::Idle);
        mgr.persist_now(&h);
    });
    Ok(())
}

pub fn stop(inner: &AcpInner, session_id: &str) {
    if let Some(l) = inner.links.get(session_id) { if let Some(c) = inner.conns.get(&l.conn_key) { c.cancel(&l.acp_id); } }
}

fn end_message(mgr: &AgentManager, h: &SessionHandle, message_id: &str, status: MessageStatus, error: Option<String>, started: Instant) {
    let sid = { let mut s = h.session.lock(); if let Some(m) = s.message_mut(message_id) { m.status = Some(status); m.duration_ms = Some(started.elapsed().as_millis() as u64); if let Some(e) = &error { if !m.parts.iter().any(|p| matches!(p, MessagePart::Error { .. })) { m.parts.push(MessagePart::Error { message: e.clone() }); } }
        // close out tool cards that never completed
        for p in &mut m.parts { if let MessagePart::Tool { call } = p { if matches!(call.status, ToolStatus::Pending | ToolStatus::Running | ToolStatus::AwaitingApproval) { call.status = if status == MessageStatus::Cancelled { ToolStatus::Cancelled } else { ToolStatus::Done }; call.finished_at = Some(now_ms()); } } }
    } s.touch(); s.id.clone() };
    mgr.emit(AgentEvent::MessageEnd { session_id: sid, message_id: message_id.to_string(), status, error, duration_ms: started.elapsed().as_millis() as u64 });
    mgr.persist(h);
}

/// One prompt turn against the external agent.
pub async fn run_turn(inner: &Arc<AcpInner>, mgr: &Arc<AgentManager>, app: Option<&AppHandle>, h: &Arc<SessionHandle>, req: SendMessageRequest, cancel: CancellationToken) -> Result<()> {
    let session_id = h.id();
    let agent_id = { let s = h.session.lock(); s.agent_id.clone().unwrap_or_default() };
    mgr.set_status(h, SessionStatus::Running);
    let started = Instant::now();

    // 1. connection + session (may spawn the agent)
    let conn = match ensure_conn(inner, mgr, &agent_id).await { Ok(c) => c, Err(e) => { mgr.emit(AgentEvent::Error { session_id: session_id.clone(), message: e.message.clone() }); mgr.set_status(h, SessionStatus::Error); return Err(e); } };
    let (link, fresh) = match ensure_session(inner, mgr, app, h, &conn).await { Ok(x) => x, Err(e) => { mgr.emit(AgentEvent::Error { session_id: session_id.clone(), message: e.message.clone() }); mgr.set_status(h, SessionStatus::Error); return Err(e); } };
    *link.cancel.lock() = cancel.clone();

    // 2. user message + checkpoint (mirrors the built-in runner so restore/edit/regenerate behave the same)
    let settings = mgr.settings();
    let root = h.session.lock().root();
    let cwd = h.session.lock().cwd_path();
    let files = mgr.file_list(&cwd).await;
    let env = crate::ai::agent::context::ContextEnv { app, root: root.clone(), cwd: cwd.clone(), settings: &settings, ws_key: crate::core::sessions::workspace_key(root.as_deref()), store: &mgr.store, files };
    let mut items = req.context.clone();
    if let Some(sel) = &req.cursor_selection { if !items.iter().any(|i| i.kind == "selection" || i.kind == "code") { items.push(ContextItem { id: "selection".into(), kind: "selection".into(), label: format!("{}:{}-{}", sel.path.rsplit('/').next().unwrap_or(&sel.path), sel.range.start_line, sel.range.end_line), path: Some(sel.path.clone()), range: Some(sel.range.clone()), data: Some(sel.text.clone()), ..Default::default() }); } }
    let built = crate::ai::agent::context::build(&env, &items).await;
    let turn_id = new_id().chars().take(8).collect::<String>();
    let mut user_msg = ChatMessage::new(MessageRole::User);
    user_msg.turn_id = Some(turn_id.clone()); user_msg.mode = Some(req.mode.clone()); user_msg.checkpoint_id = Some(turn_id.clone());
    if !items.is_empty() { user_msg.parts.push(MessagePart::Context { items: items.clone() }); }
    for img in &req.images { user_msg.parts.push(MessagePart::Image { image: img.clone() }); }
    user_msg.parts.push(MessagePart::Text { text: req.text.clone() });
    let user_msg_id = user_msg.id.clone();
    let checkpoint = Checkpoint { id: turn_id.clone(), session_id: session_id.clone(), message_id: user_msg_id.clone(), created_at: now_ms(), files: vec![], label: req.text.chars().take(60).collect() };
    let history = { let mut s = h.session.lock(); let hist = if fresh { history_block(&s) } else { None }; s.checkpoints.push(checkpoint); s.messages.push(user_msg.clone()); s.current_turn = Some(turn_id.clone()); s.touch(); hist };
    mgr.emit(AgentEvent::MessageStart { session_id: session_id.clone(), message: user_msg });
    mgr.emit(AgentEvent::Checkpoint { session_id: session_id.clone(), checkpoint_id: turn_id.clone(), message_id: user_msg_id.clone() });
    mgr.persist(h);

    // 3. prompt blocks
    let caps = &conn.init.agent_capabilities.prompt_capabilities;
    let mut blocks: Vec<Value> = vec![];
    if let Some(hist) = history { blocks.push(json!({ "type": "text", "text": hist })); }
    if !built.text.trim().is_empty() {
        if caps.embedded_context { blocks.push(json!({ "type": "resource", "resource": { "uri": "caret://attached-context", "mimeType": "text/plain", "text": format!("<attached_context>\n{}</attached_context>", built.text) } })); }
        else { blocks.push(json!({ "type": "text", "text": format!("<attached_context>\n{}</attached_context>", built.text) })); }
    }
    for it in items.iter().filter(|i| i.kind == "file" && i.data.is_none()) { if let Some(p) = &it.path { let abs = crate::paths::resolve(root.as_deref(), p); blocks.push(json!({ "type": "resource_link", "uri": format!("file://{}", abs.display()), "name": it.label })); } }
    if caps.image { for (m, d) in &built.images { blocks.push(json!({ "type": "image", "mimeType": m, "data": d })); } for img in &req.images { blocks.push(json!({ "type": "image", "mimeType": img.mime, "data": img.data_base64 })); } }
    else if !req.images.is_empty() { blocks.push(json!({ "type": "text", "text": format!("({} image attachment(s) omitted: this agent does not accept images)", req.images.len()) })); }
    if let Some(rc) = &req.review_comments { if !rc.is_empty() { let lines: Vec<String> = rc.iter().map(|c| format!("- {}:{}{} ({}): {}", c.path, c.line, c.end_line.map(|e| format!("-{e}")).unwrap_or_default(), c.side, c.text)).collect(); blocks.push(json!({ "type": "text", "text": format!("<review_comments>\n{}\n</review_comments>", lines.join("\n")) })); } }
    blocks.push(json!({ "type": "text", "text": if req.text.trim().is_empty() { "(see attachments)".to_string() } else { req.text.clone() } }));

    // 4. assistant message shell
    let mut amsg = ChatMessage::new(MessageRole::Assistant);
    amsg.status = Some(MessageStatus::Streaming); amsg.model_id = Some(format!("acp:{agent_id}")); amsg.mode = Some(req.mode.clone()); amsg.turn_id = Some(turn_id.clone());
    let mid = amsg.id.clone();
    { let mut s = h.session.lock(); s.messages.push(amsg.clone()); }
    *link.message_id.lock() = Some(mid.clone());
    *link.turn_id.lock() = Some(turn_id.clone());
    *link.turn_started.lock() = Some(started);
    link.registered.clear();
    mgr.emit(AgentEvent::MessageStart { session_id: session_id.clone(), message: amsg });

    // 5. the turn
    let prompt_fut = conn.prompt(&link.acp_id, blocks);
    tokio::pin!(prompt_fut);
    let result = tokio::select! {
        r = &mut prompt_fut => r,
        _ = cancel.cancelled() => {
            conn.cancel(&link.acp_id);
            match tokio::time::timeout(Duration::from_secs(15), &mut prompt_fut).await { Ok(r) => r, Err(_) => Ok(PromptResult { stop_reason: "cancelled".into() }) }
        }
    };
    *link.message_id.lock() = None;
    *link.turn_id.lock() = None;

    let (status, err) = match result {
        Ok(r) => match r.stop_reason.as_str() {
            "end_turn" | "max_tokens" | "max_turn_requests" => { if r.stop_reason != "end_turn" { (MessageStatus::Done, Some(format!("Stopped early: {}", r.stop_reason.replace('_', " ")))) } else { (MessageStatus::Done, None) } }
            "cancelled" => (MessageStatus::Cancelled, None),
            "refusal" => (MessageStatus::Error, Some("The agent refused to continue with this request.".into())),
            other => (MessageStatus::Done, Some(format!("Agent stopped: {other}"))),
        },
        Err(e) if cancel.is_cancelled() => { let _ = e; (MessageStatus::Cancelled, None) }
        Err(e) => (MessageStatus::Error, Some(format!("{}: {}", conn.config.name, e.message))),
    };
    // 6. finish
    let empty = { let s = h.session.lock(); s.messages.iter().find(|m| m.id == mid).map(|m| m.parts.is_empty()).unwrap_or(false) };
    if empty && status == MessageStatus::Done && err.is_none() { let mut s = h.session.lock(); if let Some(m) = s.message_mut(&mid) { m.parts.push(MessagePart::Text { text: "(no response)".into() }); } }
    end_message(mgr, h, &mid, status, err.clone(), started);
    if status == MessageStatus::Error { mgr.emit(AgentEvent::Error { session_id: session_id.clone(), message: err.clone().unwrap_or_default() }); }
    // title: external agents have no cheap title model; derive from the first prompt
    let title = { let mut s = h.session.lock(); if !s.title_generated && !req.text.trim().is_empty() { let t: String = req.text.trim().lines().next().unwrap_or("").chars().take(60).collect(); s.title = t.clone(); s.title_generated = true; Some(t) } else { None } };
    if let Some(t) = title { mgr.emit(AgentEvent::Title { session_id: session_id.clone(), title: t }); }
    mgr.persist(h);
    // Verified Diff: same auto-run hook as the built-in runner when this turn changed files.
    if !link.registered.is_empty() && settings.verify.auto_run_after_turn && !cancel.is_cancelled() && status != MessageStatus::Error {
        if let Some(app) = app { let app2 = app.clone(); let sid = session_id.clone(); let tid = turn_id.clone(); tauri::async_runtime::spawn(async move { crate::verify::auto_run_after_turn(app2, sid, tid).await; }); }
    }
    if !conn.alive() { inner.conns.remove(&conn.key); mgr.emit(AgentEvent::Error { session_id, message: format!("{} exited; it will be restarted on the next message.", conn.config.name) }); }
    Ok(())
}

// ───────────────────────── incoming (agent → client) ─────────────────────────

impl Bridge {
    fn link_for(&self, acp_sid: &str) -> Option<(Arc<SessionLink>, Arc<SessionHandle>)> {
        let caret_id = self.inner.by_acp.get(acp_sid)?.clone();
        let link = self.inner.links.get(&caret_id)?.clone();
        let h = self.mgr.get_loaded(&caret_id)?;
        Some((link, h))
    }
    fn current_message(&self, link: &SessionLink, h: &SessionHandle) -> String {
        if let Some(m) = link.message_id.lock().clone() { return m; }
        // Agent spoke outside a prompt (e.g. after cancel): open a fresh assistant message.
        let mut m = ChatMessage::new(MessageRole::Assistant); m.status = Some(MessageStatus::Streaming); m.model_id = Some(format!("acp:{}", link.agent_id));
        let id = m.id.clone();
        { let mut s = h.session.lock(); s.messages.push(m.clone()); }
        *link.message_id.lock() = Some(id.clone());
        self.mgr.emit(AgentEvent::MessageStart { session_id: link.caret_id.clone(), message: m });
        id
    }
    fn append_text(&self, link: &SessionLink, h: &SessionHandle, text: &str, thinking: bool) {
        if text.is_empty() { return; }
        let mid = self.current_message(link, h);
        { let mut s = h.session.lock(); if let Some(m) = s.message_mut(&mid) {
            match (m.parts.last_mut(), thinking) {
                (Some(MessagePart::Text { text: t }), false) => t.push_str(text),
                (Some(MessagePart::Thinking { text: t, .. }), true) => t.push_str(text),
                (_, false) => m.parts.push(MessagePart::Text { text: text.to_string() }),
                (_, true) => m.parts.push(MessagePart::Thinking { text: text.to_string(), duration_ms: None, redacted: None }),
            }
        } }
        if thinking { self.mgr.emit(AgentEvent::ThinkingDelta { session_id: link.caret_id.clone(), message_id: mid, text: text.to_string() }); }
        else { self.mgr.emit(AgentEvent::TextDelta { session_id: link.caret_id.clone(), message_id: mid, text: text.to_string() }); }
    }
    fn content_text(v: &Value) -> Option<String> {
        match v.get("type").and_then(|t| t.as_str()) {
            Some("text") => v.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()),
            Some("resource_link") => Some(format!("[{}]({})", v.get("name").and_then(|t| t.as_str()).unwrap_or("resource"), v.get("uri").and_then(|t| t.as_str()).unwrap_or(""))),
            Some("resource") => v.pointer("/resource/text").and_then(|t| t.as_str()).map(|s| s.to_string()),
            Some("image") => Some("(image)".into()),
            _ => None,
        }
    }

    /// Apply a `tool_call` / `tool_call_update` to the session: create or patch the card, record diffs, link terminals.
    fn apply_tool(&self, link: &SessionLink, h: &SessionHandle, u: ToolCallUpdate, create: bool) {
        let mid = link.tool_msg.get(&u.tool_call_id).map(|m| m.clone()).unwrap_or_else(|| self.current_message(link, h));
        link.tool_msg.insert(u.tool_call_id.clone(), mid.clone());
        let mut diffs: Vec<(String, Option<String>, String)> = vec![];
        let mut terminals: Vec<String> = vec![];
        let mut out_text = String::new();
        if let Some(content) = &u.content {
            for c in content {
                match c.get("type").and_then(|t| t.as_str()) {
                    Some("diff") => { if let Some(p) = c.get("path").and_then(|p| p.as_str()) { diffs.push((p.to_string(), c.get("oldText").and_then(|t| t.as_str()).map(|s| s.to_string()), c.get("newText").and_then(|t| t.as_str()).unwrap_or("").to_string())); } }
                    Some("terminal") => { if let Some(t) = c.get("terminalId").and_then(|t| t.as_str()) { terminals.push(t.to_string()); } }
                    Some("content") => { if let Some(t) = c.get("content").and_then(Self::content_text) { if !out_text.is_empty() { out_text.push('\n'); } out_text.push_str(&t); } }
                    _ => { if let Some(t) = Self::content_text(c) { if !out_text.is_empty() { out_text.push('\n'); } out_text.push_str(&t); } }
                }
            }
        }
        // Diffs are only *recorded* once the call has completed (before that the file may not be written yet, e.g.
        // a permission request carrying the proposed edit); until then they are shown as a preview on the card.
        let (mut changes, mut change_diff) = (vec![], None);
        match u.status.as_deref() {
            Some("completed") => {
                let to_record = if diffs.is_empty() { link.pending_diffs.remove(&u.tool_call_id).map(|(_, v)| v).unwrap_or_default() } else { link.pending_diffs.remove(&u.tool_call_id); diffs.clone() };
                for (path, old, new) in &to_record { let (sum, diff) = self.record_diff_block(link, h, path, old.as_deref(), new, &u.tool_call_id); change_diff = Some(diff); changes.push(sum); }
            }
            Some("failed") => { link.pending_diffs.remove(&u.tool_call_id); }
            _ => {
                if !diffs.is_empty() {
                    let root = h.session.lock().root();
                    change_diff = diffs.first().map(|(p, o, n)| unified_diff(&display_path(root.as_deref(), &crate::paths::resolve(root.as_deref(), p)), o.as_deref().unwrap_or(""), n).0);
                    link.pending_diffs.insert(u.tool_call_id.clone(), diffs.clone());
                }
            }
        }
        let term_outputs: Vec<(String, String, Option<i32>)> = terminals.iter().filter_map(|tid| self.inner.terminals.get(tid).map(|t| { *t.tool.lock() = Some((mid.clone(), u.tool_call_id.clone())); (t.command_line.clone(), if t.finished.load(Ordering::SeqCst) { t.output() } else { String::new() }, t.exit.lock().as_ref().and_then(|e| e.0)) })).collect();

        let (sid, rec) = {
            let mut s = h.session.lock();
            let sid = s.id.clone();
            let existing = s.find_tool_mut(&mid, &u.tool_call_id).is_some();
            if !existing {
                let (name, display) = map_kind_display(u.kind.as_deref(), u.title.as_deref());
                let mut rec = ToolCallRecord::new(&u.tool_call_id, &name, &display);
                rec.started_at = Some(now_ms());
                rec.status = u.status.as_deref().map(map_tool_status).unwrap_or(ToolStatus::Running);
                if let Some(m) = s.message_mut(&mid) { m.parts.push(MessagePart::Tool { call: rec }); }
            }
            let rec = s.find_tool_mut(&mid, &u.tool_call_id).map(|r| {
                if let Some(t) = &u.title { if !t.trim().is_empty() { r.display_name = t.clone(); } }
                if let Some(k) = &u.kind { r.raw_kind = Some(k.clone()); let (n, _) = map_kind_display(Some(k), None); r.name = n; }
                if let Some(st) = &u.status { r.status = map_tool_status(st); if matches!(r.status, ToolStatus::Done | ToolStatus::Error) { r.finished_at = Some(now_ms()); } }
                if let Some(i) = &u.raw_input { r.input = i.clone(); if r.command.is_none() { if let Some(c) = i.get("command").and_then(|c| c.as_str()) { r.command = Some(c.to_string()); } } }
                if let Some(l) = &u.locations { r.locations = Some(l.iter().map(|x| crate::ai::agent::session::ToolLocation { path: x.path.clone(), line: x.line }).collect()); }
                if !out_text.is_empty() { let truncated = out_text.len() > 60_000; r.output = Some(out_text.chars().take(60_000).collect()); if truncated { r.output_truncated = Some(true); } }
                else if let Some(o) = &u.raw_output { if r.output.is_none() { let t = match o { Value::String(s) => s.clone(), v => v.to_string() }; r.output = Some(t.chars().take(60_000).collect()); } }
                if r.status == ToolStatus::Error && r.error.is_none() { r.error = r.output.clone().filter(|o| o.len() < 2000); }
                if !changes.is_empty() { let mut fc = r.file_changes.take().unwrap_or_default(); for c in &changes { if !fc.iter().any(|x| x.change_id == c.change_id) { fc.push(c.clone()); } } r.file_changes = Some(fc); }
                if let Some(d) = &change_diff { r.diff = Some(d.clone()); }
                for (cmd, out, code) in &term_outputs { r.command = Some(cmd.clone()); if !out.is_empty() { r.output = Some(out.clone()); } if code.is_some() { r.exit_code = *code; } }
                r.clone()
            });
            s.touch();
            (sid, rec)
        };
        if let Some(call) = rec {
            if create && !link.tool_msg.contains_key(&format!("started:{}", u.tool_call_id)) { link.tool_msg.insert(format!("started:{}", u.tool_call_id), mid.clone()); self.mgr.emit(AgentEvent::ToolStart { session_id: sid, message_id: mid, call }); }
            else { self.mgr.emit(AgentEvent::ToolUpdate { session_id: sid, message_id: mid, call }); }
        }
    }

    /// Snapshot + PendingChange for a file the agent changed (already applied on disk by the agent or by us).
    /// The same edit often arrives twice (`fs/write_text_file` and a `diff` block on the tool call): the second
    /// report is folded into the existing change (and tags it with the tool call id) instead of duplicating it.
    fn record_change(&self, link: &SessionLink, h: &SessionHandle, path: &str, before: Option<String>, after: &str, tool_call_id: Option<&str>) -> FileChangeSummary {
        let root = h.session.lock().root();
        let abs = crate::paths::resolve(root.as_deref(), path);
        let key = abs.to_string_lossy().to_string();
        let hv = hash_str(after);
        if let Some(existing) = link.registered.get(&key).and_then(|v| v.iter().find(|e| e.0 == hv).map(|e| e.1.clone())) {
            let mut s = h.session.lock();
            if let Some(c) = s.changes.iter_mut().find(|c| c.change_id == existing) { if c.tool_call_id.is_none() { c.tool_call_id = tool_call_id.map(|t| t.to_string()); } return c.summary(); }
        }
        let turn_id = link.turn_id.lock().clone().unwrap_or_default();
        let before = before.or_else(|| std::fs::read_to_string(&abs).ok().filter(|b| b != after));
        self.mgr.snapshot_before_write(&link.caret_id, &turn_id, &abs, before.as_deref());
        let rel = display_path(root.as_deref(), &abs);
        let (diff, add, del) = unified_diff(&rel, before.as_deref().unwrap_or(""), after);
        let kind = if before.is_none() { ChangeKind::Create } else { ChangeKind::Modify };
        let change_id = new_id();
        link.registered.entry(key).or_default().push((hv, change_id.clone()));
        let change = PendingChange { change_id, session_id: link.caret_id.clone(), path: rel, kind, before, after: Some(after.to_string()), diff, status: ChangeStatus::Pending, turn_id: Some(turn_id), tool_call_id: tool_call_id.map(|s| s.to_string()), additions: add, deletions: del, old_path: None };
        self.mgr.add_change(&link.caret_id, change)
    }

    /// A `diff` content block on a tool call. Adapters differ: some send whole-file `oldText`/`newText`, Claude Code's
    /// sends the edit's old/new *snippets*. If the file was already written through `fs/write_text_file` this turn we
    /// fold the block into that (authoritative, whole-file) change; otherwise we reconstruct whole-file before/after
    /// from the current file contents so accept/reject never writes a snippet over a file.
    fn record_diff_block(&self, link: &SessionLink, h: &SessionHandle, path: &str, old_text: Option<&str>, new_text: &str, tool_call_id: &str) -> (FileChangeSummary, String) {
        let root = h.session.lock().root();
        let abs = crate::paths::resolve(root.as_deref(), path);
        let key = abs.to_string_lossy().to_string();
        if let Some(entries) = link.registered.get(&key).map(|v| v.clone()) {
            let mut s = h.session.lock();
            for (_, cid) in entries.iter().rev() {
                if let Some(c) = s.changes.iter_mut().find(|c| &c.change_id == cid) {
                    if c.after.as_deref().map(|a| new_text.is_empty() || a.contains(new_text)).unwrap_or(true) {
                        if c.tool_call_id.is_none() { c.tool_call_id = Some(tool_call_id.to_string()); }
                        return (c.summary(), c.diff.clone());
                    }
                }
            }
        }
        let current = std::fs::read_to_string(&abs).ok();
        let old_full = old_text.map(|o| o.to_string()).filter(|o| !o.is_empty());
        let (before, after): (Option<String>, String) = match current {
            Some(cur) if cur == new_text => (old_full, cur),
            Some(cur) if !new_text.is_empty() && cur.contains(new_text) => (Some(cur.replacen(new_text, old_text.unwrap_or(""), 1)), cur),
            _ => (old_full, new_text.to_string()),
        };
        let sum = self.record_change(link, h, &key, before.clone(), &after, Some(tool_call_id));
        let diff = h.session.lock().changes.iter().find(|c| c.change_id == sum.change_id).map(|c| c.diff.clone()).unwrap_or_else(|| unified_diff(&sum.path, before.as_deref().unwrap_or(""), &after).0);
        (sum, diff)
    }

    async fn on_permission(&self, p: PermissionRequest) -> std::result::Result<Value, RpcError> {
        let (link, h) = self.link_for(&p.session_id).ok_or_else(|| RpcError::invalid_params("unknown sessionId"))?;
        let tc: ToolCallUpdate = serde_json::from_value(p.tool_call.clone()).unwrap_or_default();
        if !tc.tool_call_id.is_empty() { self.apply_tool(&link, &h, tc.clone(), true); }
        let mid = link.tool_msg.get(&tc.tool_call_id).map(|m| m.clone()).unwrap_or_else(|| self.current_message(&link, &h));
        let kind_s = tc.kind.clone().unwrap_or_else(|| "other".into());
        let title = tc.title.clone().unwrap_or_else(|| map_kind_display(Some(&kind_s), None).1);
        let command = tc.raw_input.as_ref().and_then(|i| i.get("command").and_then(|c| c.as_str()).map(|s| s.to_string()));
        let mem_key = if kind_s == "execute" { format!("execute:{}", command.clone().unwrap_or_else(|| title.clone())) } else { format!("{kind_s}:*") };
        let pick = |kind: &str| p.options.iter().find(|o| o.kind == kind).map(|o| o.option_id.clone());
        let allow_once = pick("allow_once").or_else(|| pick("allow_always")).or_else(|| p.options.first().map(|o| o.option_id.clone()));
        if link.allow_memory.contains_key(&mem_key) { if let Some(id) = allow_once.clone() { return Ok(json!({ "outcome": { "outcome": "selected", "optionId": id } })); } }

        let mut options = vec![];
        if pick("allow_once").is_some() || allow_once.is_some() { options.push(ApprovalOption::Allow); options.push(ApprovalOption::AllowSession); }
        if pick("allow_always").is_some() { options.push(ApprovalOption::AllowAlways); }
        if pick("reject_once").is_some() { options.push(ApprovalOption::Deny); }
        if pick("reject_always").is_some() { options.push(ApprovalOption::DenyAlways); }
        if !options.iter().any(|o| matches!(o, ApprovalOption::Deny | ApprovalOption::DenyAlways)) { options.push(ApprovalOption::Deny); }
        let akind = match kind_s.as_str() { "execute" => ApprovalKind::Command, "edit" | "move" => ApprovalKind::FileEdit, "delete" => ApprovalKind::FileDelete, "fetch" => ApprovalKind::Web, "switch_mode" | "think" | "read" | "search" | "other" => ApprovalKind::Mcp, _ => ApprovalKind::Mcp };
        let detail = {
            let mut d = String::new();
            if let Some(l) = &tc.locations { let paths: Vec<String> = l.iter().map(|x| x.path.clone()).collect(); if !paths.is_empty() { d.push_str(&paths.join(", ")); } }
            if d.is_empty() { if let Some(i) = &tc.raw_input { let t = i.to_string(); d = t.chars().take(600).collect(); } }
            if d.is_empty() { d = format!("{} wants to run: {}", link.agent_id, title); }
            d
        };
        let req = ApprovalRequest { approval_id: new_id(), session_id: link.caret_id.clone(), tool_call_id: tc.tool_call_id.clone(), kind: akind, title: title.clone(), detail, command: command.clone(), cwd: Some(h.session.lock().cwd_path().to_string_lossy().to_string()), options, allow_always_pattern: None, classification: None, risk_tier: None, sandboxed: Some(false), agent_id: Some(link.agent_id.clone()) };
        let cancel = link.cancel.lock().clone();
        let res = self.mgr.request_approval(&h, &mid, req, &cancel).await;
        let outcome = match res {
            Err(_) => json!({ "outcome": { "outcome": "cancelled" } }),
            Ok(opt) => {
                let chosen = match opt {
                    ApprovalOption::Allow => allow_once.clone(),
                    ApprovalOption::AllowSession => { link.allow_memory.insert(mem_key.clone(), ()); allow_once.clone() }
                    ApprovalOption::AllowAlways => pick("allow_always").or_else(|| allow_once.clone()),
                    ApprovalOption::Deny => pick("reject_once").or_else(|| pick("reject_always")),
                    ApprovalOption::DenyAlways => pick("reject_always").or_else(|| pick("reject_once")),
                };
                let denied = matches!(opt, ApprovalOption::Deny | ApprovalOption::DenyAlways);
                { let (sid, rec) = { let mut s = h.session.lock(); let sid = s.id.clone(); let r = s.find_tool_mut(&mid, &tc.tool_call_id).map(|r| { r.status = if denied { ToolStatus::Rejected } else { ToolStatus::Running }; r.clone() }); (sid, r) }; if let Some(call) = rec { self.mgr.emit(AgentEvent::ToolUpdate { session_id: sid, message_id: mid.clone(), call }); } }
                match chosen { Some(id) => json!({ "outcome": { "outcome": "selected", "optionId": id } }), None => json!({ "outcome": { "outcome": "cancelled" } }) }
            }
        };
        Ok(outcome)
    }

    async fn on_fs_read(&self, p: FsReadParams) -> std::result::Result<Value, RpcError> {
        let (_, h) = self.link_for(&p.session_id).ok_or_else(|| RpcError::invalid_params("unknown sessionId"))?;
        let root = h.session.lock().root();
        let abs = crate::paths::resolve(root.as_deref(), &p.path);
        let text = tokio::fs::read_to_string(&abs).await.map_err(|e| RpcError::internal(format!("read {}: {e}", abs.display())))?;
        let content = match (p.line, p.limit) {
            (None, None) => text,
            (line, limit) => { let start = line.unwrap_or(1).saturating_sub(1) as usize; let it = text.lines().skip(start); let v: Vec<&str> = match limit { Some(n) => it.take(n as usize).collect(), None => it.collect() }; v.join("\n") }
        };
        Ok(json!({ "content": content }))
    }

    async fn on_fs_write(&self, p: FsWriteParams) -> std::result::Result<Value, RpcError> {
        let (link, h) = self.link_for(&p.session_id).ok_or_else(|| RpcError::invalid_params("unknown sessionId"))?;
        let root = h.session.lock().root();
        let abs = crate::paths::resolve(root.as_deref(), &p.path);
        let before = tokio::fs::read_to_string(&abs).await.ok();
        if let Some(parent) = abs.parent() { let _ = tokio::fs::create_dir_all(parent).await; }
        tokio::fs::write(&abs, &p.content).await.map_err(|e| RpcError::internal(format!("write {}: {e}", abs.display())))?;
        if before.as_deref() != Some(p.content.as_str()) { self.record_change(&link, &h, &abs.to_string_lossy(), before, &p.content, None); }
        if let Some(app) = self.mgr.app_opt() { let _ = app.emit("caret://fs/changed", json!({ "path": abs.to_string_lossy() })); }
        Ok(json!({}))
    }

    async fn on_terminal_create(&self, p: TerminalCreateParams) -> std::result::Result<Value, RpcError> {
        use tokio::io::AsyncReadExt;
        let (link, h) = self.link_for(&p.session_id).ok_or_else(|| RpcError::invalid_params("unknown sessionId"))?;
        let root = h.session.lock().root();
        let cwd = p.cwd.as_deref().map(|c| crate::paths::resolve(root.as_deref(), c)).unwrap_or_else(|| h.session.lock().cwd_path());
        let command_line = std::iter::once(p.command.clone()).chain(p.args.iter().cloned()).collect::<Vec<_>>().join(" ");
        let mut cmd = tokio::process::Command::new(&p.command);
        cmd.args(&p.args).current_dir(&cwd).env("PATH", crate::acp::process::augmented_path()).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(false);
        for e in &p.env { cmd.env(&e.name, &e.value); }
        #[cfg(unix)] { cmd.process_group(0); }
        let mut child = cmd.spawn().map_err(|e| RpcError::internal(format!("spawn `{command_line}`: {e}")))?;
        let id = format!("term-{}", new_id());
        let term = Arc::new(TermProc { id: id.clone(), caret_id: link.caret_id.clone(), command_line: command_line.clone(), cwd: cwd.to_string_lossy().to_string(), pid: child.id(), buf: Mutex::new(vec![]), truncated: AtomicBool::new(false), limit: p.output_byte_limit.unwrap_or(1 << 20) as usize, exit: Mutex::new(None), done: tokio::sync::Notify::new(), finished: AtomicBool::new(false), tool: Mutex::new(None) });
        self.inner.terminals.insert(id.clone(), term.clone());
        self.inner.log(&link.agent_id, format!("[terminal] {id}: {command_line} (cwd {})", cwd.display()));
        let mut out = child.stdout.take(); let mut err = child.stderr.take();
        let (t1, t2, t3) = (term.clone(), term.clone(), term.clone());
        let o = tokio::spawn(async move { if let Some(mut o) = out.take() { let mut b = [0u8; 8192]; while let Ok(n) = o.read(&mut b).await { if n == 0 { break; } t1.append(&b[..n]); } } });
        let e = tokio::spawn(async move { if let Some(mut e) = err.take() { let mut b = [0u8; 8192]; while let Ok(n) = e.read(&mut b).await { if n == 0 { break; } t2.append(&b[..n]); } } });
        let mgr = self.mgr.clone(); let inner = self.inner.clone();
        tokio::spawn(async move {
            let status = child.wait().await.ok();
            let _ = tokio::join!(o, e);
            let code = status.as_ref().and_then(|s| s.code());
            #[cfg(unix)] let signal = { use std::os::unix::process::ExitStatusExt; status.as_ref().and_then(|s| s.signal()).map(|s| format!("SIG{s}")) };
            #[cfg(not(unix))] let signal: Option<String> = None;
            *t3.exit.lock() = Some((code, signal));
            t3.finished.store(true, Ordering::SeqCst);
            t3.done.notify_waiters();
            // surface output on the linked tool card
            if let Some((mid, tid)) = t3.tool.lock().clone() { if let Some(h) = mgr.get_loaded(&t3.caret_id) { let (sid, rec) = { let mut s = h.session.lock(); let sid = s.id.clone(); let r = s.find_tool_mut(&mid, &tid).map(|r| { r.command = Some(t3.command_line.clone()); r.output = Some(t3.output().chars().take(60_000).collect()); r.exit_code = code; r.clone() }); (sid, r) }; if let Some(call) = rec { mgr.emit(AgentEvent::ToolUpdate { session_id: sid, message_id: mid, call }); } } }
            let _ = inner;
        });
        Ok(json!({ "terminalId": id }))
    }

    fn term(&self, p: &TerminalRef) -> std::result::Result<Arc<TermProc>, RpcError> { self.inner.terminals.get(&p.terminal_id).map(|t| t.clone()).ok_or_else(|| RpcError::invalid_params(format!("unknown terminalId {}", p.terminal_id))) }
}

#[async_trait::async_trait]
impl IncomingHandler for Bridge {
    async fn on_request(&self, method: &str, params: Value) -> std::result::Result<Value, RpcError> {
        match method {
            "session/request_permission" => self.on_permission(serde_json::from_value(params).map_err(|e| RpcError::invalid_params(e.to_string()))?).await,
            "fs/read_text_file" => self.on_fs_read(serde_json::from_value(params).map_err(|e| RpcError::invalid_params(e.to_string()))?).await,
            "fs/write_text_file" => self.on_fs_write(serde_json::from_value(params).map_err(|e| RpcError::invalid_params(e.to_string()))?).await,
            "terminal/create" => self.on_terminal_create(serde_json::from_value(params).map_err(|e| RpcError::invalid_params(e.to_string()))?).await,
            "terminal/output" => { let p: TerminalRef = serde_json::from_value(params).map_err(|e| RpcError::invalid_params(e.to_string()))?; let t = self.term(&p)?; let exit = t.exit.lock().clone().map(|(c, s)| json!({ "exitCode": c, "signal": s })); Ok(json!({ "output": t.output(), "truncated": t.truncated.load(Ordering::SeqCst), "exitStatus": exit })) }
            "terminal/wait_for_exit" => { let p: TerminalRef = serde_json::from_value(params).map_err(|e| RpcError::invalid_params(e.to_string()))?; let t = self.term(&p)?; if !t.finished.load(Ordering::SeqCst) { let n = t.done.notified(); if !t.finished.load(Ordering::SeqCst) { n.await; } } let (c, s) = t.exit.lock().clone().unwrap_or((None, None)); Ok(json!({ "exitCode": c, "signal": s })) }
            "terminal/kill" => { let p: TerminalRef = serde_json::from_value(params).map_err(|e| RpcError::invalid_params(e.to_string()))?; let t = self.term(&p)?; if let Some(pid) = t.pid { crate::acp::process::kill_pid(pid).await; } Ok(json!({})) }
            "terminal/release" => { let p: TerminalRef = serde_json::from_value(params).map_err(|e| RpcError::invalid_params(e.to_string()))?; if let Some((_, t)) = self.inner.terminals.remove(&p.terminal_id) { if !t.finished.load(Ordering::SeqCst) { if let Some(pid) = t.pid { crate::acp::process::kill_pid(pid).await; } } } Ok(json!({})) }
            _ => Err(RpcError::method_not_found(method)),
        }
    }

    async fn on_notification(&self, method: &str, params: Value) {
        if method != "session/update" { self.inner.log(&self.agent_id, format!("[acp] notification {method} ignored")); return; }
        let Some(sid) = params.get("sessionId").and_then(|s| s.as_str()) else { return };
        let Some((link, h)) = self.link_for(sid) else {
            let mut e = self.inner.pending_updates.entry(sid.to_string()).or_default();
            if e.len() < 64 { e.push(params.clone()); } else { self.inner.log(&self.agent_id, format!("[acp] dropping early update for unlinked session {sid}")); }
            return;
        };
        let Some(update) = params.get("update") else { return };
        let kind = update.get("sessionUpdate").and_then(|k| k.as_str()).unwrap_or("");
        match kind {
            "agent_message_chunk" => { if let Some(c) = update.get("content") { if let Some(t) = Self::content_text(c) { self.append_text(&link, &h, &t, false); } else if c.get("type").and_then(|t| t.as_str()) == Some("image") { let mid = self.current_message(&link, &h); let img = ImageAttachment { id: new_id(), name: "image".into(), mime: c.get("mimeType").and_then(|m| m.as_str()).unwrap_or("image/png").into(), data_base64: c.get("data").and_then(|d| d.as_str()).unwrap_or("").into(), width: None, height: None }; let mut s = h.session.lock(); if let Some(m) = s.message_mut(&mid) { m.parts.push(MessagePart::Image { image: img }); } } } }
            "agent_thought_chunk" => { if let Some(t) = update.get("content").and_then(Self::content_text) { self.append_text(&link, &h, &t, true); } }
            "user_message_chunk" => {}
            "tool_call" => { let mut u: ToolCallUpdate = serde_json::from_value(update.clone()).unwrap_or_default(); if u.status.is_none() { u.status = Some("pending".into()); } self.apply_tool(&link, &h, u, true); }
            "tool_call_update" => { let u: ToolCallUpdate = serde_json::from_value(update.clone()).unwrap_or_default(); self.apply_tool(&link, &h, u, false); }
            "plan" => {
                let entries: Vec<PlanEntry> = update.get("entries").cloned().and_then(|e| serde_json::from_value(e).ok()).unwrap_or_default();
                let todos: Vec<TodoItem> = entries.iter().enumerate().map(|(i, e)| TodoItem { id: format!("plan-{i}"), content: e.content.clone(), status: match e.status.as_deref() { Some("in_progress") => "in_progress", Some("completed") => "completed", _ => "pending" }.into() }).collect();
                self.mgr.update_todos(&link.caret_id, todos, false);
            }
            "available_commands_update" => { *link.commands.lock() = update.get("availableCommands").and_then(|c| c.as_array()).cloned().unwrap_or_default(); }
            "current_mode_update" => {
                if let Some(m) = update.get("currentModeId").and_then(|m| m.as_str()) { if let Some(ms) = link.modes.lock().as_mut() { ms.current_mode_id = m.to_string(); } }
                if let Some(c) = self.inner.conns.get(&link.conn_key) { emit_agent_status(&self.inner, &self.mgr, &link, &c); }
            }
            "session_info_update" => { if let Some(t) = update.get("title").and_then(|t| t.as_str()) { let changed = { let mut s = h.session.lock(); if !s.title_generated || s.title != t { s.title = t.to_string(); s.title_generated = true; true } else { false } }; if changed { self.mgr.emit(AgentEvent::Title { session_id: link.caret_id.clone(), title: t.to_string() }); } } }
            "usage_update" => {
                let used = update.get("used").and_then(|u| u.as_u64()).or_else(|| update.get("inputTokens").and_then(|u| u.as_u64()));
                let cost = update.pointer("/cost/amount").and_then(|c| c.as_f64());
                let (usage, ctx, session_usage) = { let mut s = h.session.lock(); if let Some(u) = used { s.context_tokens = u; } if let Some(c) = cost { s.usage.cost_usd = Some(c); } let mut u = Usage::default(); u.cost_usd = cost; (u, s.context_tokens, s.usage.clone()) };
                let mid = link.message_id.lock().clone().unwrap_or_default();
                self.mgr.emit(AgentEvent::Usage { session_id: link.caret_id.clone(), message_id: mid, usage, context_tokens: ctx, session_usage });
            }
            "notice" => { if let Some(t) = update.get("content").and_then(Self::content_text).or_else(|| update.get("message").and_then(|m| m.as_str()).map(|s| s.to_string())) { self.append_text(&link, &h, &format!("\n\n> {t}\n\n"), false); } }
            "config_option_update" => {
                if let Some(c) = update.get("configOptions").cloned().and_then(|c| serde_json::from_value::<Vec<ConfigOption>>(c).ok()) { *link.config_options.lock() = Some(c); }
                if let Some(c) = self.inner.conns.get(&link.conn_key) { emit_agent_status(&self.inner, &self.mgr, &link, &c); }
            }
            "compaction_update" | "compaction_summary_chunk" => { self.inner.log(&self.agent_id, format!("[acp] {kind}")); }
            other => { self.inner.log(&self.agent_id, format!("[acp] unhandled session update `{other}`")); }
        }
    }
}

#[allow(dead_code)]
fn _assert_send() { fn f<T: Send + Sync>() {} f::<Bridge>(); f::<SessionLink>(); f::<TermProc>(); }

pub fn root_of(mgr: &AgentManager) -> PathBuf { mgr.workspace_root().unwrap_or_else(crate::paths::home_dir) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_tool_status_and_kind() {
        assert_eq!(map_tool_status("completed"), ToolStatus::Done); assert_eq!(map_tool_status("failed"), ToolStatus::Error); assert_eq!(map_tool_status("pending"), ToolStatus::Pending);
        assert_eq!(map_kind_display(Some("execute"), None), ("acp_execute".into(), "Run command".into()));
        assert_eq!(map_kind_display(Some("edit"), Some("Edit src/a.ts")).1, "Edit src/a.ts");
    }
    #[test]
    fn unified_diff_counts() { let (d, a, r) = unified_diff("x.txt", "a\nb\n", "a\nc\nd\n"); assert!(d.contains("+c")); assert_eq!(a, 2); assert_eq!(r, 1); }
}

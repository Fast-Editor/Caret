//! Orchestrator tool surface: the MCP tools another agent (Claude Code in a terminal, Codex, Cursor, another Caret)
//! calls to spawn and steer Caret threads. Every tool is a thin wrapper over the same `AgentManager` / ACP bridge
//! entry points the chat UI uses, so a thread driven from here looks identical in the app: same approvals, same
//! review bar, same checkpoints.
//!
//! Threads are Caret chat sessions; `thread_id` is the session id. A thread is either driven by the built-in agent
//! (`agent_id = "caret"`, any configured provider/model) or bound to an external ACP agent exactly like
//! `chat_create_session` binds one.
use crate::acp::{self, AcpInner};
use crate::ai::agent::manager::{AgentManager, SessionHandle};
use crate::ai::agent::session::*;
use crate::core::settings::Settings;
use crate::error::{CaretError, Result};
use dashmap::DashMap;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::AppHandle;

pub const BUILTIN_AGENT: &str = "caret";
const DEFAULT_TIMEOUT_S: u64 = 600;
const MAX_TIMEOUT_S: u64 = 6 * 3600;
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const READ_LIMIT_MAX: usize = 500;

/// Everything a tool call needs. Production: the Tauri-managed manager / ACP state; tests: headless instances.
pub struct Ctx {
    pub mgr: Arc<AgentManager>,
    pub acp: Arc<AcpInner>,
    pub app: Option<AppHandle>,
    pub pending: Arc<Pending>,
}

impl Ctx {
    /// Build a context and start the pending-item tracker on the manager's event broadcast.
    pub fn new(mgr: Arc<AgentManager>, acp: Arc<AcpInner>, app: Option<AppHandle>) -> Arc<Self> {
        let pending = Arc::new(Pending::default());
        let (rx, p) = (mgr.subscribe_events(), pending.clone());
        tauri::async_runtime::spawn(Pending::track(p, rx));
        Arc::new(Self { mgr, acp, app, pending })
    }
    fn settings(&self) -> Arc<Settings> { self.mgr.settings() }
}

// ───────────────────────── pending approvals / questions ─────────────────────────

/// Approvals and questions currently waiting for an answer, learned from the event stream. Entries clear when the
/// owning session leaves the awaiting state (resolution, cancellation, error) or is deleted. The session itself is
/// also scanned (`scan_session`) so items raised before the tracker started are still visible.
#[derive(Default)]
pub struct Pending {
    approvals: DashMap<String, ApprovalRequest>,
    questions: DashMap<String, (String, AgentQuestion)>,
}

impl Pending {
    pub fn observe(&self, ev: &AgentEvent) {
        match ev {
            AgentEvent::ApprovalRequest { request, .. } => { self.approvals.insert(request.approval_id.clone(), request.clone()); }
            AgentEvent::Question { session_id, question, .. } => { self.questions.insert(question.question_id.clone(), (session_id.clone(), question.clone())); }
            AgentEvent::SessionStatus { session_id, status } if !matches!(status, SessionStatus::AwaitingApproval | SessionStatus::AwaitingInput) => self.clear_session(session_id),
            AgentEvent::SessionDeleted { session_id } => self.clear_session(session_id),
            _ => {}
        }
    }
    pub fn clear_session(&self, session_id: &str) {
        self.approvals.retain(|_, r| r.session_id != session_id);
        self.questions.retain(|_, (s, _)| s != session_id);
    }
    pub fn remove_approval(&self, id: &str) { self.approvals.remove(id); }
    pub fn remove_question(&self, id: &str) { self.questions.remove(id); }
    pub fn approvals_for(&self, session_id: Option<&str>) -> Vec<ApprovalRequest> {
        self.approvals.iter().filter(|e| session_id.map(|s| e.value().session_id == s).unwrap_or(true)).map(|e| e.value().clone()).collect()
    }
    pub fn questions_for(&self, session_id: Option<&str>) -> Vec<(String, AgentQuestion)> {
        self.questions.iter().filter(|e| session_id.map(|s| e.value().0 == s).unwrap_or(true)).map(|e| e.value().clone()).collect()
    }
    pub fn len(&self) -> usize { self.approvals.len() + self.questions.len() }
    pub fn is_empty(&self) -> bool { self.len() == 0 }
    async fn track(p: Arc<Pending>, mut rx: tokio::sync::broadcast::Receiver<AgentEvent>) {
        loop {
            match rx.recv().await {
                Ok(ev) => p.observe(&ev),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => tracing::debug!(skipped = n, "orchestrator pending tracker lagged"),
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}

fn approval_json(r: &ApprovalRequest) -> Value {
    json!({
        "approval_id": r.approval_id, "thread_id": r.session_id, "kind": r.kind, "title": r.title, "detail": r.detail,
        "command": r.command, "cwd": r.cwd, "options": r.options, "risk_tier": r.risk_tier, "agent_id": r.agent_id,
    })
}
fn question_json(session_id: &str, q: &AgentQuestion) -> Value {
    json!({ "question_id": q.question_id, "thread_id": session_id, "text": q.question, "options": q.options, "allow_free_text": q.allow_free_text })
}

/// Pending items visible in the session itself: tool calls parked in `AwaitingApproval` and unanswered questions.
fn scan_session(s: &Session) -> (Vec<ApprovalRequest>, Vec<(String, AgentQuestion)>) {
    let mut approvals = vec![]; let mut questions = vec![];
    for m in &s.messages {
        for p in &m.parts {
            match p {
                MessagePart::Tool { call } if call.status == ToolStatus::AwaitingApproval => { if let Some(a) = &call.approval { approvals.push(a.clone()); } }
                MessagePart::Question { question, answer: None } if s.status == SessionStatus::AwaitingInput => questions.push((s.id.clone(), question.clone())),
                _ => {}
            }
        }
    }
    (approvals, questions)
}

/// Approvals and questions for one session, merged from the tracker and the session scan (deduplicated by id).
fn pending_for(ctx: &Ctx, s: &Session) -> (Vec<Value>, Vec<Value>) {
    let (mut approvals, mut questions) = scan_session(s);
    for a in ctx.pending.approvals_for(Some(&s.id)) { if !approvals.iter().any(|x| x.approval_id == a.approval_id) { approvals.push(a); } }
    for q in ctx.pending.questions_for(Some(&s.id)) { if !questions.iter().any(|x| x.1.question_id == q.1.question_id) { questions.push(q); } }
    (approvals.iter().map(approval_json).collect(), questions.iter().map(|(sid, q)| question_json(sid, q)).collect())
}

// ───────────────────────── tool catalogue ─────────────────────────

fn spec(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({ "name": name, "description": description, "inputSchema": { "type": "object", "properties": properties, "required": required, "additionalProperties": false } })
}

/// MCP `tools/list` payload.
pub fn specs() -> Vec<Value> {
    let thread = json!({"type": "string", "description": "Thread id returned by create_thread or list_threads."});
    let timeout = json!({"type": "integer", "minimum": 1, "maximum": MAX_TIMEOUT_S, "description": format!("Seconds to wait for the thread to settle (default {DEFAULT_TIMEOUT_S}).")});
    vec![
        spec("list_agents", "List the agents that can drive a thread: the built-in `caret` agent (runs on any configured provider/model) plus enabled external ACP agents (Claude Code, Codex, Gemini CLI, ...) with install and sign-in state.", json!({}), &[]),
        spec("list_threads", "List Caret threads (chat sessions) in the open workspace with status, agent, model and last update.", json!({
            "include_archived": {"type": "boolean", "description": "Include archived threads (default false)."},
            "limit": {"type": "integer", "minimum": 1, "maximum": 500, "description": "Maximum number of threads to return (default 50)."}
        }), &[]),
        spec("create_thread", "Create a new thread. Omit agent_id to use the built-in Caret agent; pass an ACP agent id from list_agents (for example `codex`) to have that agent drive the thread. Returns the thread_id to use with send.", json!({
            "title": {"type": "string", "description": "Thread title shown in Caret."},
            "agent_id": {"type": "string", "description": "`caret` (default) or an enabled ACP agent id."},
            "model_id": {"type": "string", "description": "Caret model id for the built-in agent (see Settings > Models); omit for the default or Auto."},
            "mode": {"type": "string", "description": "Agent mode: agent (default), ask, plan, debug or a custom mode id."},
            "use_worktree": {"type": "boolean", "description": "Run the thread in its own git worktree and branch (default false)."}
        }), &[]),
        spec("send", "Send a user message to a thread. With wait=true (default) blocks until the thread settles (idle, error, awaiting approval or awaiting an answer) or the timeout passes, then returns the assistant's reply, files changed, usage and any pending approval or question. If the thread is already running the message is queued.", json!({
            "thread_id": thread,
            "text": {"type": "string", "description": "The message to send."},
            "wait": {"type": "boolean", "description": "Block until the thread settles (default true)."},
            "timeout_s": timeout,
            "model_id": {"type": "string", "description": "Override the model for this turn (built-in agent only)."},
            "mode": {"type": "string", "description": "Override the mode for this turn."}
        }), &["thread_id", "text"]),
        spec("wait", "Wait until a running thread settles and return the same result shape as send. Use after send with wait=false, after approve/answer, or when a previous wait timed out.", json!({ "thread_id": thread, "timeout_s": timeout }), &["thread_id"]),
        spec("read_thread", "Read a thread's messages (user, assistant, system) with text, tool calls and timestamps, plus the total count for paging.", json!({
            "thread_id": thread,
            "offset": {"type": "integer", "minimum": 0, "description": "Index of the first message to return (default 0)."},
            "limit": {"type": "integer", "minimum": 1, "maximum": READ_LIMIT_MAX, "description": "Maximum messages to return (default 50)."}
        }), &["thread_id"]),
        spec("stop", "Cancel the running turn of a thread (queued messages are dropped with it).", json!({ "thread_id": thread }), &["thread_id"]),
        spec("pending", "List approvals (commands, file edits, MCP calls) and questions currently waiting on a human or orchestrator, for one thread or all threads.", json!({ "thread_id": {"type": "string", "description": "Restrict to one thread; omit for all threads."} }), &[]),
        spec("approve", "Resolve a pending approval. `allow` runs it once, `allow-session` also skips further prompts for the same command prefix in this thread, `deny` rejects it (the agent is told and continues).", json!({
            "approval_id": {"type": "string"},
            "option": {"type": "string", "enum": ["allow", "allow-session", "deny"]}
        }), &["approval_id", "option"]),
        spec("answer", "Answer a question the agent asked (see pending).", json!({ "question_id": {"type": "string"}, "answer": {"type": "string"} }), &["question_id", "answer"]),
        spec("delete_thread", "Delete a thread, its checkpoints and its worktree (if any).", json!({ "thread_id": thread }), &["thread_id"]),
    ]
}

pub fn tool_names() -> Vec<String> { specs().iter().filter_map(|t| t["name"].as_str().map(String::from)).collect() }

// ───────────────────────── argument helpers ─────────────────────────

fn arg_str(args: &Value, key: &str) -> Option<String> { args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from) }
fn need_str(args: &Value, key: &str) -> Result<String> { arg_str(args, key).ok_or_else(|| CaretError::invalid(format!("`{key}` is required"))) }
fn arg_bool(args: &Value, key: &str, default: bool) -> bool { args.get(key).and_then(Value::as_bool).unwrap_or(default) }
fn arg_u64(args: &Value, key: &str, default: u64) -> u64 { args.get(key).and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f.max(0.0) as u64))).unwrap_or(default) }
fn timeout_arg(args: &Value) -> Duration { Duration::from_secs(arg_u64(args, "timeout_s", DEFAULT_TIMEOUT_S).clamp(1, MAX_TIMEOUT_S)) }
fn status_str(s: SessionStatus) -> String { serde_json::to_value(s).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_else(|| "idle".into()) }

// ───────────────────────── dispatch ─────────────────────────

/// Run one tool. Errors become `isError` results at the transport layer.
pub async fn call(ctx: &Arc<Ctx>, name: &str, args: &Value) -> Result<Value> {
    let args = if args.is_null() { &Value::Object(Default::default()) } else { args };
    if !args.is_object() { return Err(CaretError::invalid("tool arguments must be an object")); }
    match name {
        "list_agents" => list_agents(ctx),
        "list_threads" => list_threads(ctx, args),
        "create_thread" => create_thread(ctx, args).await,
        "send" => send(ctx, args).await,
        "wait" => wait(ctx, args).await,
        "read_thread" => read_thread(ctx, args),
        "stop" => stop(ctx, args),
        "pending" => pending(ctx, args),
        "approve" => approve(ctx, args).await,
        "answer" => answer(ctx, args).await,
        "delete_thread" => delete_thread(ctx, args).await,
        other => Err(CaretError::not_found(format!("tool `{other}`"))),
    }
}

fn list_agents(ctx: &Ctx) -> Result<Value> {
    let settings = ctx.settings();
    let root = ctx.mgr.workspace_root();
    let mut agents = vec![json!({
        "id": BUILTIN_AGENT, "name": "Caret (built-in)", "kind": "builtin", "installed": true, "state": "ready",
        "default_model": settings.default_model().map(|m| m.id.clone()),
        "models": settings.models.iter().filter(|m| m.enabled).map(|m| m.id.clone()).collect::<Vec<_>>(),
    })];
    if settings.acp.enabled {
        for c in acp::registry::all_configs(&settings).into_iter().filter(|c| c.enabled) {
            let st = acp::bridge::status_for(&ctx.acp, &c, root.as_deref());
            agents.push(json!({ "id": st.id, "name": st.name, "kind": "acp", "installed": st.installed, "state": st.state, "version": st.version, "auth_required": st.auth_required, "error": st.error, "description": c.description }));
        }
    }
    Ok(json!({ "agents": agents, "default_agent_id": settings.acp.default_agent_id, "workspace": root.map(|r| r.to_string_lossy().to_string()) }))
}

fn thread_summary(s: &SessionSummary) -> Value {
    json!({
        "id": s.id, "title": s.title, "status": status_str(s.status), "agent": s.agent_id.clone().unwrap_or_else(|| BUILTIN_AGENT.into()),
        "model": s.model_id, "mode": s.mode, "created_at": s.created_at, "updated_at": s.updated_at, "workspace": s.workspace_root,
        "message_count": s.message_count, "pending_change_count": s.pending_change_count, "worktree_path": s.worktree_path,
        "archived": s.archived.unwrap_or(false), "last_message_preview": s.last_message_preview,
    })
}

fn list_threads(ctx: &Ctx, args: &Value) -> Result<Value> {
    let include_archived = arg_bool(args, "include_archived", false);
    let limit = arg_u64(args, "limit", 50).clamp(1, 500) as usize;
    let list = ctx.mgr.list_sessions(include_archived);
    Ok(json!({ "threads": list.iter().take(limit).map(thread_summary).collect::<Vec<_>>(), "total": list.len() }))
}

async fn create_thread(ctx: &Arc<Ctx>, args: &Value) -> Result<Value> {
    let settings = ctx.settings();
    let agent = arg_str(args, "agent_id").unwrap_or_else(|| settings.acp.default_agent_id.clone());
    let agent = if agent.is_empty() { BUILTIN_AGENT.to_string() } else { agent };
    if agent != BUILTIN_AGENT {
        if !settings.acp.enabled { return Err(CaretError::invalid("external agents are disabled (Settings > Agents)")); }
        if acp::registry::find_config(&settings, &agent).filter(|c| c.enabled).is_none() { return Err(CaretError::not_found(format!("agent `{agent}` (see list_agents)"))); }
    }
    if let Some(m) = arg_str(args, "model_id") { if m != "auto" && settings.model(&m).is_none() { return Err(CaretError::not_found(format!("model `{m}` (see list_agents for configured models)"))); } }
    let h = ctx.mgr.create_session(arg_str(args, "mode"), arg_str(args, "model_id"), arg_str(args, "title"), arg_bool(args, "use_worktree", false), None).await?;
    if agent != BUILTIN_AGENT {
        // Same binding as `chat_create_session`: the agent id lives on the session; the UI reloads it.
        { let mut s = h.session.lock(); s.agent_id = Some(agent.clone()); }
        ctx.mgr.persist(&h);
        ctx.mgr.emit(AgentEvent::SessionReset { session_id: h.id() });
    }
    let s = h.session.lock();
    Ok(json!({ "thread_id": s.id, "title": s.title, "agent": agent, "model": s.model_id, "mode": s.mode, "workspace": s.workspace_root, "worktree_path": s.worktree_path, "branch": s.branch }))
}

/// Same routing as `chat_send`: ACP-bound sessions go through the ACP bridge, everything else through the manager.
async fn submit(ctx: &Arc<Ctx>, agent: Option<String>, req: SendMessageRequest) -> Result<()> {
    match agent.filter(|a| a != BUILTIN_AGENT) {
        Some(_) => acp::bridge::submit(ctx.acp.clone(), ctx.mgr.clone(), ctx.app.clone(), req).await,
        None => ctx.mgr.submit(req).await,
    }
}

async fn send(ctx: &Arc<Ctx>, args: &Value) -> Result<Value> {
    let thread_id = need_str(args, "thread_id")?;
    let text = need_str(args, "text")?;
    let h = ctx.mgr.handle(&thread_id)?;
    let (mode, agent, since_msgs, since_changes) = { let s = h.session.lock(); (arg_str(args, "mode").unwrap_or_else(|| s.mode.clone()), s.agent_id.clone(), s.messages.len(), s.changes.len()) };
    // Subscribe before submitting so the settle transition cannot be missed.
    let rx = ctx.mgr.subscribe_events();
    let req = SendMessageRequest { session_id: thread_id.clone(), text, mode, model_id: arg_str(args, "model_id"), ..Default::default() };
    submit(ctx, agent, req).await?;
    if !arg_bool(args, "wait", true) {
        let s = h.session.lock();
        return Ok(json!({ "thread_id": thread_id, "status": if h.is_running() { "running".to_string() } else { status_str(s.status) }, "queued": s.queued.len(), "messages_added": s.messages.len().saturating_sub(since_msgs) }));
    }
    wait_turn(ctx, &h, rx, timeout_arg(args), since_msgs, since_changes).await
}

async fn wait(ctx: &Arc<Ctx>, args: &Value) -> Result<Value> {
    let h = ctx.mgr.handle(&need_str(args, "thread_id")?)?;
    let rx = ctx.mgr.subscribe_events();
    let (since_msgs, since_changes) = { let s = h.session.lock(); turn_anchor(&s) };
    wait_turn(ctx, &h, rx, timeout_arg(args), since_msgs, since_changes).await
}

/// Start of the current turn: index just past the last user message and the first change recorded for that turn.
fn turn_anchor(s: &Session) -> (usize, usize) {
    let Some(ui) = s.messages.iter().rposition(|m| m.role == MessageRole::User) else { return (0, 0) };
    let turn = s.messages[ui].turn_id.clone();
    let ci = turn.as_ref().and_then(|t| s.changes.iter().position(|c| c.turn_id.as_ref() == Some(t))).unwrap_or(s.changes.len());
    (ui + 1, ci)
}

/// `Some(status)` once the thread no longer needs to be waited on.
fn settled(h: &SessionHandle) -> Option<SessionStatus> {
    let s = h.session.lock();
    match s.status {
        SessionStatus::AwaitingApproval | SessionStatus::AwaitingInput => Some(s.status),
        // `running` is cleared right before the final `Idle` status lands; never report a finished thread as running.
        st if !h.is_running() => Some(if st == SessionStatus::Running || st == SessionStatus::Summarizing { SessionStatus::Idle } else { st }),
        _ => None,
    }
}

async fn wait_turn(ctx: &Arc<Ctx>, h: &Arc<SessionHandle>, mut rx: tokio::sync::broadcast::Receiver<AgentEvent>, timeout: Duration, since_msgs: usize, since_changes: usize) -> Result<Value> {
    let deadline = Instant::now() + timeout;
    let sid = h.id();
    loop {
        if let Some(status) = settled(h) { return Ok(turn_result(ctx, h, status, since_msgs, since_changes, false)); }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() { return Ok(turn_result(ctx, h, SessionStatus::Running, since_msgs, since_changes, true)); }
        tokio::select! {
            ev = rx.recv() => match ev {
                Ok(AgentEvent::SessionDeleted { session_id }) if session_id == sid => return Err(CaretError::not_found(format!("thread {sid}"))),
                Ok(_) => {} // any event re-checks `settled`; status/message-end events are what we are waiting for
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => { tokio::time::sleep(remaining.min(POLL_INTERVAL)).await; }
            },
            _ = tokio::time::sleep(remaining.min(POLL_INTERVAL)) => {}
        }
    }
}

fn change_json(c: &PendingChange) -> Value {
    json!({ "change_id": c.change_id, "path": c.path, "kind": c.kind, "status": c.status, "additions": c.additions, "deletions": c.deletions, "old_path": c.old_path })
}

/// Result shape shared by `send` and `wait`.
fn turn_result(ctx: &Ctx, h: &SessionHandle, status: SessionStatus, since_msgs: usize, since_changes: usize, timed_out: bool) -> Value {
    let s = h.session.lock();
    let last_assistant = s.messages.iter().rev().find(|m| m.role == MessageRole::Assistant);
    let assistant_text = last_assistant.map(|m| m.text()).unwrap_or_default();
    let error = last_assistant.and_then(|m| m.parts.iter().find_map(|p| if let MessagePart::Error { message } = p { Some(message.clone()) } else { None }));
    let (approvals, questions) = pending_for(ctx, &s);
    let files_changed: Vec<Value> = s.changes.iter().skip(since_changes.min(s.changes.len())).map(change_json).collect();
    json!({
        "thread_id": s.id,
        "status": if timed_out { "running".to_string() } else { status_str(status) },
        "timed_out": timed_out,
        "assistant_text": assistant_text,
        "assistant_message_id": last_assistant.map(|m| m.id.clone()),
        "messages_added": s.messages.len().saturating_sub(since_msgs),
        "pending_approval": approvals.first().cloned(),
        "pending_question": questions.first().cloned(),
        "files_changed": files_changed,
        "pending_changes": s.pending_summaries().len(),
        "usage": { "turn": last_assistant.and_then(|m| m.usage.clone()), "thread": s.usage, "context_tokens": s.context_tokens },
        "error": error,
        "queued": s.queued.len(),
        "worktree_path": s.worktree_path,
    })
}

fn tool_call_json(c: &ToolCallRecord) -> Value {
    let summary = c.summary.clone().or_else(|| c.command.clone()).or_else(|| c.input_text.clone()).unwrap_or_else(|| { let t = c.input.to_string(); if t.chars().count() > 200 { t.chars().take(200).collect::<String>() + "..." } else { t } });
    json!({ "id": c.id, "name": c.name, "status": c.status, "summary": summary, "error": c.error, "files": c.file_changes.as_ref().map(|f| f.iter().map(|x| x.path.clone()).collect::<Vec<_>>()) })
}

fn read_thread(ctx: &Ctx, args: &Value) -> Result<Value> {
    let h = ctx.mgr.handle(&need_str(args, "thread_id")?)?;
    let offset = arg_u64(args, "offset", 0) as usize;
    let limit = arg_u64(args, "limit", 50).clamp(1, READ_LIMIT_MAX as u64) as usize;
    let s = h.session.lock();
    let total = s.messages.len();
    let messages: Vec<Value> = s.messages.iter().skip(offset.min(total)).take(limit).map(|m| {
        let tool_calls: Vec<Value> = m.parts.iter().filter_map(|p| if let MessagePart::Tool { call } = p { Some(tool_call_json(call)) } else { None }).collect();
        let errors: Vec<String> = m.parts.iter().filter_map(|p| if let MessagePart::Error { message } = p { Some(message.clone()) } else { None }).collect();
        json!({ "id": m.id, "role": m.role, "text": m.text(), "tool_calls": tool_calls, "errors": errors, "created_at": m.created_at, "status": m.status, "model_id": m.model_id })
    }).collect();
    let (approvals, questions) = pending_for(ctx, &s);
    Ok(json!({
        "thread_id": s.id, "title": s.title, "status": if h.is_running() && s.status == SessionStatus::Idle { "running".to_string() } else { status_str(s.status) },
        "agent": s.agent_id.clone().unwrap_or_else(|| BUILTIN_AGENT.into()), "model": s.model_id, "mode": s.mode,
        "total": total, "offset": offset, "messages": messages, "pending_changes": s.pending_summaries().len(), "todos": s.todos,
        "pending_approval": approvals.first().cloned(), "pending_question": questions.first().cloned(), "worktree_path": s.worktree_path, "branch": s.branch,
    }))
}

fn stop(ctx: &Ctx, args: &Value) -> Result<Value> {
    let id = need_str(args, "thread_id")?;
    let h = ctx.mgr.handle(&id)?;
    acp::bridge::stop(&ctx.acp, &id);
    ctx.mgr.stop(&id);
    let s = h.session.lock();
    Ok(json!({ "ok": true, "thread_id": id, "was_running": h.is_running(), "status": status_str(s.status) }))
}

fn pending(ctx: &Ctx, args: &Value) -> Result<Value> {
    let mut approvals = vec![]; let mut questions = vec![];
    let ids: Vec<String> = match arg_str(args, "thread_id") {
        Some(id) => vec![id],
        None => {
            let mut ids: Vec<String> = ctx.mgr.list_sessions(true).into_iter().filter(|s| matches!(s.status, SessionStatus::AwaitingApproval | SessionStatus::AwaitingInput)).map(|s| s.id).collect();
            for a in ctx.pending.approvals_for(None) { if !ids.contains(&a.session_id) { ids.push(a.session_id); } }
            for (sid, _) in ctx.pending.questions_for(None) { if !ids.contains(&sid) { ids.push(sid); } }
            ids
        }
    };
    for id in ids {
        let Ok(h) = ctx.mgr.handle(&id) else { continue };
        let s = h.session.lock();
        let (a, q) = pending_for(ctx, &s);
        approvals.extend(a); questions.extend(q);
    }
    Ok(json!({ "approvals": approvals, "questions": questions }))
}

fn parse_option(raw: &str) -> Result<ApprovalOption> {
    serde_json::from_value(Value::String(raw.trim().to_ascii_lowercase().replace('_', "-"))).map_err(|_| CaretError::invalid(format!("unknown option `{raw}`; use allow, allow-session or deny")))
}

async fn approve(ctx: &Arc<Ctx>, args: &Value) -> Result<Value> {
    let id = need_str(args, "approval_id")?;
    let option = parse_option(&need_str(args, "option")?)?;
    let sid = ctx.pending.approvals_for(None).into_iter().find(|a| a.approval_id == id).map(|a| a.session_id)
        .or_else(|| session_with_item(ctx, |s| scan_session(s).0.iter().any(|a| a.approval_id == id)));
    ctx.mgr.resolve_approval(&id, option)?;
    ctx.pending.remove_approval(&id);
    if let Some(sid) = sid { settle_after_resolution(ctx, &sid, |s| scan_session(s).0.iter().any(|a| a.approval_id == id)).await; }
    Ok(json!({ "ok": true, "approval_id": id, "option": option }))
}

/// Id of the session whose messages satisfy `pred` (used when the tracker has not seen the item).
fn session_with_item(ctx: &Ctx, pred: impl Fn(&Session) -> bool) -> Option<String> {
    ctx.mgr.list_sessions(true).into_iter().map(|s| s.id).find(|id| ctx.mgr.handle(id).map(|h| pred(&h.session.lock())).unwrap_or(false))
}

/// After an approval or answer is resolved the runner needs a moment to leave the awaiting state. Wait (bounded) until
/// the item is gone from the session, so a `wait` issued right afterwards does not see the stale status and return early.
async fn settle_after_resolution(ctx: &Arc<Ctx>, sid: &str, still_pending: impl Fn(&Session) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        let Ok(h) = ctx.mgr.handle(sid) else { return };
        let waiting = { let s = h.session.lock(); matches!(s.status, SessionStatus::AwaitingApproval | SessionStatus::AwaitingInput) && still_pending(&s) };
        if !waiting { return; }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn answer(ctx: &Arc<Ctx>, args: &Value) -> Result<Value> {
    let id = need_str(args, "question_id")?;
    let answer = need_str(args, "answer")?;
    let sid = session_with_item(ctx, |s| scan_session(s).1.iter().any(|(_, q)| q.question_id == id));
    ctx.mgr.resolve_question(&id, answer)?;
    ctx.pending.remove_question(&id);
    if let Some(sid) = sid { settle_after_resolution(ctx, &sid, |s| scan_session(s).1.iter().any(|(_, q)| q.question_id == id)).await; }
    Ok(json!({ "ok": true, "question_id": id }))
}

async fn delete_thread(ctx: &Ctx, args: &Value) -> Result<Value> {
    let id = need_str(args, "thread_id")?;
    ctx.mgr.delete_session(&id).await?;
    ctx.pending.clear_session(&id);
    Ok(json!({ "ok": true, "thread_id": id }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval(id: &str, sid: &str) -> ApprovalRequest {
        ApprovalRequest { approval_id: id.into(), session_id: sid.into(), tool_call_id: "t".into(), kind: ApprovalKind::Command, title: "Run".into(), detail: "echo".into(), command: Some("echo hi".into()), cwd: None, options: vec![ApprovalOption::Allow, ApprovalOption::Deny], allow_always_pattern: None, classification: None, risk_tier: None, sandboxed: None, agent_id: None, permission_mode: None }
    }

    #[test]
    fn pending_tracks_and_clears_by_session() {
        let p = Pending::default();
        p.observe(&AgentEvent::SessionStatus { session_id: "s1".into(), status: SessionStatus::AwaitingApproval });
        p.observe(&AgentEvent::ApprovalRequest { session_id: "s1".into(), request: approval("a1", "s1") });
        p.observe(&AgentEvent::Question { session_id: "s2".into(), message_id: "m".into(), question: AgentQuestion { question_id: "q1".into(), question: "color?".into(), options: None, allow_free_text: true } });
        assert_eq!(p.approvals_for(None).len(), 1); assert_eq!(p.questions_for(Some("s2")).len(), 1); assert_eq!(p.len(), 2);
        // other sessions' status changes do not touch s1
        p.observe(&AgentEvent::SessionStatus { session_id: "s2".into(), status: SessionStatus::Running });
        assert_eq!(p.approvals_for(Some("s1")).len(), 1); assert!(p.questions_for(None).is_empty());
        // resolution puts the session back to running: cleared
        p.observe(&AgentEvent::SessionStatus { session_id: "s1".into(), status: SessionStatus::Running });
        assert!(p.is_empty());
        p.observe(&AgentEvent::ApprovalRequest { session_id: "s3".into(), request: approval("a3", "s3") });
        p.observe(&AgentEvent::SessionDeleted { session_id: "s3".into() });
        assert!(p.is_empty());
    }

    #[test]
    fn option_parsing_accepts_kebab_and_snake() {
        assert_eq!(parse_option("allow").unwrap(), ApprovalOption::Allow);
        assert_eq!(parse_option("allow-session").unwrap(), ApprovalOption::AllowSession);
        assert_eq!(parse_option("ALLOW_SESSION").unwrap(), ApprovalOption::AllowSession);
        assert_eq!(parse_option("deny").unwrap(), ApprovalOption::Deny);
        assert!(parse_option("maybe").is_err());
    }

    #[test]
    fn specs_are_well_formed_and_unique() {
        let names = tool_names();
        let mut dedup = names.clone(); dedup.sort(); dedup.dedup();
        assert_eq!(names.len(), dedup.len());
        for expected in ["list_agents", "list_threads", "create_thread", "send", "wait", "read_thread", "stop", "pending", "approve", "answer", "delete_thread"] { assert!(names.contains(&expected.to_string()), "{expected}"); }
        for t in specs() {
            assert!(t["description"].as_str().map(|d| d.len() > 20).unwrap_or(false));
            assert_eq!(t["inputSchema"]["type"], "object");
            let props = t["inputSchema"]["properties"].as_object().unwrap();
            for r in t["inputSchema"]["required"].as_array().unwrap() { assert!(props.contains_key(r.as_str().unwrap()), "{} requires {r}", t["name"]); }
        }
    }

    #[test]
    fn turn_anchor_points_past_last_user_message() {
        let mut s = Session::default();
        let mut u = ChatMessage::new(MessageRole::User); u.turn_id = Some("t1".into()); s.messages.push(u);
        s.messages.push(ChatMessage::new(MessageRole::Assistant));
        let mut u2 = ChatMessage::new(MessageRole::User); u2.turn_id = Some("t2".into()); s.messages.push(u2);
        s.changes.push(PendingChange { change_id: "c1".into(), session_id: s.id.clone(), path: "a".into(), kind: ChangeKind::Modify, before: None, after: None, diff: String::new(), status: ChangeStatus::Pending, turn_id: Some("t1".into()), tool_call_id: None, additions: 1, deletions: 0, old_path: None });
        s.changes.push(PendingChange { change_id: "c2".into(), session_id: s.id.clone(), path: "b".into(), kind: ChangeKind::Create, before: None, after: None, diff: String::new(), status: ChangeStatus::Pending, turn_id: Some("t2".into()), tool_call_id: None, additions: 1, deletions: 0, old_path: None });
        assert_eq!(turn_anchor(&s), (3, 1));
        assert_eq!(turn_anchor(&Session::default()), (0, 0));
    }
}

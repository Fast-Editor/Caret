//! Tauri commands for the chat/agent surface (`api.chat.*` in src/lib/api.ts).
use crate::ai::agent::context::{self, ContextEnv};
use crate::ai::agent::manager::AgentManager;
use crate::ai::agent::modes;
use crate::ai::agent::session::*;
use crate::ai::agent::{plans, runner};
use crate::core::sessions::SessionStore;
use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use serde_json::Value;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

type Mgr<'a> = State<'a, Arc<AgentManager>>;

#[tauri::command]
pub fn chat_subscribe(mgr: Mgr<'_>, on_event: Channel<AgentEvent>) -> Result<()> { mgr.subscribe(on_event); Ok(()) }

#[tauri::command(async)]
pub fn chat_list_sessions(mgr: Mgr<'_>, include_archived: Option<bool>) -> Result<Vec<SessionSummary>> { Ok(mgr.list_sessions(include_archived.unwrap_or(false))) }

#[tauri::command]
pub async fn chat_create_session(app: AppHandle, mgr: Mgr<'_>, mode: Option<String>, model_id: Option<String>, title: Option<String>, use_worktree: Option<bool>, agent_id: Option<String>) -> Result<ChatSessionView> {
    let h = mgr.create_session(mode, model_id, title, use_worktree.unwrap_or(false), None).await?;
    // External (ACP) agent binding: explicit choice wins, else the configured default agent.
    let default_agent = app.state::<SettingsState>().effective().acp.default_agent_id.clone();
    let agent = agent_id.filter(|a| !a.is_empty()).unwrap_or(default_agent);
    if agent != "caret" { { let mut s = h.session.lock(); s.agent_id = Some(agent); } mgr.persist(&h); mgr.emit(AgentEvent::SessionReset { session_id: h.id() }); }
    let v = h.session.lock().view(); Ok(v)
}

#[tauri::command(async)]
pub fn chat_get_session(mgr: Mgr<'_>, session_id: String) -> Result<ChatSessionView> { let h = mgr.handle(&session_id)?; let v = h.session.lock().view(); Ok(v) }

#[tauri::command]
pub async fn chat_delete_session(mgr: Mgr<'_>, session_id: String) -> Result<()> { mgr.delete_session(&session_id).await }

#[tauri::command]
pub fn chat_rename_session(mgr: Mgr<'_>, session_id: String, title: String) -> Result<()> {
    let h = mgr.handle(&session_id)?;
    { let mut s = h.session.lock(); s.title = title.clone(); s.title_generated = true; s.touch(); }
    mgr.emit(AgentEvent::Title { session_id, title }); mgr.persist_now(&h); Ok(())
}

#[tauri::command]
pub fn chat_duplicate_session(mgr: Mgr<'_>, session_id: String) -> Result<ChatSessionView> {
    let h = mgr.handle(&session_id)?;
    let mut s = h.session.lock().clone();
    s.id = new_id(); s.title = format!("{} (copy)", s.title); s.created_at = now_ms(); s.updated_at = now_ms(); s.status = SessionStatus::Idle; s.queued.clear(); s.worktree_path = None; s.branch = None; s.cwd = s.workspace_root.clone(); s.checkpoints.clear(); s.transcript_marks.clear(); s.changes.clear(); s.pinned = false;
    let nh = crate::ai::agent::manager::SessionHandle::new(s);
    let v = nh.session.lock().view();
    mgr.persist_now(&nh);
    mgr.emit(AgentEvent::SessionCreated { session: nh.session.lock().summary_view() });
    Ok(v)
}

#[tauri::command]
pub fn chat_pin_session(mgr: Mgr<'_>, session_id: String, pinned: bool) -> Result<()> { let h = mgr.handle(&session_id)?; { h.session.lock().pinned = pinned; } mgr.persist_now(&h); Ok(()) }
#[tauri::command]
pub fn chat_archive_session(mgr: Mgr<'_>, session_id: String, archived: bool) -> Result<()> { let h = mgr.handle(&session_id)?; { h.session.lock().archived = archived; } mgr.persist_now(&h); Ok(()) }

#[tauri::command(async)]
pub fn chat_export_session(mgr: Mgr<'_>, session_id: String, format: String) -> Result<String> {
    let h = mgr.handle(&session_id)?; let s = h.session.lock();
    Ok(match format.as_str() { "json" => serde_json::to_string_pretty(&s.view())?, _ => SessionStore::export_markdown(&s) })
}

#[tauri::command]
pub async fn chat_send(app: AppHandle, mgr: Mgr<'_>, req: SendMessageRequest) -> Result<()> {
    if req.session_id.is_empty() { return Err(CaretError::invalid("sessionId is required")); }
    // Route to an external ACP agent when the session (or this request) is bound to one.
    let h = mgr.handle(&req.session_id)?;
    let agent = {
        let mut s = h.session.lock();
        if let Some(a) = req.agent_id.as_deref().filter(|a| !a.is_empty()) { let next = if a == "caret" { None } else { Some(a.to_string()) }; if s.agent_id != next { s.agent_id = next; } }
        s.agent_id.clone()
    };
    if let Some(a) = agent.filter(|a| a != "caret") {
        let _ = a;
        let st = app.state::<crate::acp::AcpState>();
        return crate::acp::bridge::submit(st.inner.clone(), mgr.inner().clone(), Some(app.clone()), req).await;
    }
    mgr.inner().submit(req).await
}

#[tauri::command]
pub fn chat_stop(app: AppHandle, mgr: Mgr<'_>, session_id: String) -> Result<()> { crate::acp::bridge::stop(&app.state::<crate::acp::AcpState>().inner, &session_id); mgr.stop(&session_id); Ok(()) }

#[tauri::command]
pub fn chat_approve(mgr: Mgr<'_>, approval_id: String, option: ApprovalOption) -> Result<()> {
    tracing::info!(approval_id = %approval_id, option = ?option, "approval resolved by UI");
    mgr.resolve_approval(&approval_id, option)
}

#[tauri::command]
pub fn chat_answer_question(mgr: Mgr<'_>, question_id: String, answer: String) -> Result<()> { mgr.resolve_question(&question_id, answer) }

#[tauri::command]
pub fn chat_queue_remove(mgr: Mgr<'_>, session_id: String, queued_id: String) -> Result<()> {
    let h = mgr.handle(&session_id)?;
    let q = { let mut s = h.session.lock(); s.queued.retain(|q| q.id != queued_id); s.queued.clone() };
    mgr.emit(AgentEvent::Queue { session_id, queued: q }); mgr.persist(&h); Ok(())
}

#[tauri::command]
pub async fn chat_queue_send_now(mgr: Mgr<'_>, session_id: String, queued_id: String) -> Result<()> {
    let h = mgr.handle(&session_id)?;
    let item = { let mut s = h.session.lock(); let idx = s.queued.iter().position(|q| q.id == queued_id).ok_or_else(|| CaretError::not_found("queued message"))?; let it = s.queued.remove(idx); s.queued.insert(0, it.clone()); it };
    let q = h.session.lock().queued.clone();
    mgr.emit(AgentEvent::Queue { session_id: session_id.clone(), queued: q });
    if h.is_running() { mgr.stop(&session_id); } // current turn cancels; the queue drains next
    else { let req = item.request.unwrap_or(SendMessageRequest { session_id, text: item.text, context: item.context, images: item.images, mode: item.mode.unwrap_or_default(), ..Default::default() }); { h.session.lock().queued.retain(|q| q.id != queued_id); } mgr.inner().submit(req).await?; }
    Ok(())
}

#[tauri::command]
pub fn chat_accept_changes(mgr: Mgr<'_>, session_id: String, change_ids: Option<Vec<String>>) -> Result<()> { mgr.accept_changes(&session_id, change_ids) }
#[tauri::command]
pub fn chat_reject_changes(mgr: Mgr<'_>, session_id: String, change_ids: Option<Vec<String>>) -> Result<()> { mgr.reject_changes(&session_id, change_ids) }

#[tauri::command(async)]
pub fn chat_pending_changes(mgr: Mgr<'_>, session_id: String) -> Result<Vec<PendingChange>> {
    let h = mgr.handle(&session_id)?; let s = h.session.lock();
    Ok(s.changes.iter().filter(|c| c.status == ChangeStatus::Pending).cloned().collect())
}

#[tauri::command]
pub fn chat_change_diff(mgr: Mgr<'_>, change_id: String) -> Result<PendingChange> { mgr.find_change(&change_id).ok_or_else(|| CaretError::not_found(format!("change {change_id}"))) }

#[tauri::command(async)]
pub fn chat_checkpoints(mgr: Mgr<'_>, session_id: String) -> Result<Vec<Checkpoint>> { let h = mgr.handle(&session_id)?; let v = h.session.lock().checkpoints.clone(); Ok(v) }

#[tauri::command]
pub fn chat_restore_checkpoint(mgr: Mgr<'_>, session_id: String, checkpoint_id: String) -> Result<Vec<String>> { mgr.restore_checkpoint(&session_id, &checkpoint_id) }

#[tauri::command]
pub fn chat_set_todos(mgr: Mgr<'_>, session_id: String, todos: Vec<TodoItem>) -> Result<()> { mgr.handle(&session_id)?; mgr.update_todos(&session_id, todos, false); Ok(()) }

#[tauri::command]
pub async fn chat_summarize(app: AppHandle, mgr: Mgr<'_>, session_id: String) -> Result<()> {
    let h = mgr.handle(&session_id)?;
    if h.is_running() { return Err(CaretError::invalid("Wait for the agent to finish before summarizing")); }
    let settings = mgr.settings();
    let model_id = h.session.lock().model_id.clone();
    let resolved = crate::ai::llm::resolve_model(&settings, &app.state::<SettingsState>(), model_id.as_deref())?;
    runner::compact_now(&mgr, &h, &resolved, &h.token()).await?;
    mgr.set_status(&h, SessionStatus::Idle);
    Ok(())
}

#[tauri::command]
pub fn chat_set_mode(mgr: Mgr<'_>, session_id: String, mode: String) -> Result<()> { let h = mgr.handle(&session_id)?; { h.session.lock().mode = mode; } mgr.persist(&h); Ok(()) }
#[tauri::command]
pub fn chat_set_model(mgr: Mgr<'_>, session_id: String, model_id: String) -> Result<()> { let h = mgr.handle(&session_id)?; { h.session.lock().model_id = if model_id.is_empty() || model_id == "auto" { None } else { Some(model_id) }; } mgr.persist(&h); Ok(()) }

/// Truncate the session back to (and including) a message and return the user text of that turn, if any.
fn truncate_to(h: &crate::ai::agent::manager::SessionHandle, message_id: &str, include: bool) -> Result<Option<(String, Vec<ContextItem>, Vec<ImageAttachment>, String)>> {
    let mut s = h.session.lock();
    let idx = s.messages.iter().position(|m| m.id == message_id).ok_or_else(|| CaretError::not_found("message"))?;
    // Find the user message that started this turn.
    let turn_id = s.messages[idx].turn_id.clone();
    let user_idx = s.messages[..=idx].iter().rposition(|m| m.role == MessageRole::User && m.turn_id == turn_id).unwrap_or(idx);
    let um = s.messages[user_idx].clone();
    let cut = if include { user_idx } else { idx };
    let text = um.text();
    let ctx: Vec<ContextItem> = um.parts.iter().find_map(|p| if let MessagePart::Context { items } = p { Some(items.clone()) } else { None }).unwrap_or_default();
    let images: Vec<ImageAttachment> = um.parts.iter().filter_map(|p| if let MessagePart::Image { image } = p { Some(image.clone()) } else { None }).collect();
    let mode = um.mode.clone().unwrap_or_else(|| s.mode.clone());
    if let Some(t) = &turn_id { if let Some((_, len)) = s.transcript_marks.iter().find(|(x, _)| x == t).cloned() { s.transcript.truncate(len); } s.transcript_marks.retain(|(x, _)| x != t); s.checkpoints.retain(|c| &c.id != t); s.changes.retain(|c| c.turn_id.as_ref() != Some(t)); }
    s.messages.truncate(cut);
    s.context_tokens = s.estimate_context_tokens();
    let _ = cut;
    Ok(Some((text, ctx, images, mode)))
}

#[tauri::command]
pub async fn chat_regenerate(mgr: Mgr<'_>, session_id: String, message_id: String) -> Result<()> {
    let h = mgr.handle(&session_id)?;
    if h.is_running() { return Err(CaretError::invalid("Stop the agent first")); }
    let Some((text, context, images, mode)) = truncate_to(&h, &message_id, true)? else { return Ok(()) };
    mgr.persist_now(&h); mgr.emit(AgentEvent::SessionReset { session_id: session_id.clone() });
    mgr.inner().submit(SendMessageRequest { session_id, text, context, images, mode, ..Default::default() }).await
}

#[tauri::command]
pub async fn chat_edit_message(mgr: Mgr<'_>, session_id: String, message_id: String, text: String) -> Result<()> {
    let h = mgr.handle(&session_id)?;
    if h.is_running() { return Err(CaretError::invalid("Stop the agent first")); }
    let Some((_, context, images, mode)) = truncate_to(&h, &message_id, true)? else { return Ok(()) };
    mgr.persist_now(&h); mgr.emit(AgentEvent::SessionReset { session_id: session_id.clone() });
    mgr.inner().submit(SendMessageRequest { session_id, text, context, images, mode, ..Default::default() }).await
}

#[tauri::command(async)]
pub fn chat_list_modes(mgr: Mgr<'_>) -> Result<Vec<ModeView>> {
    let settings = mgr.settings();
    Ok(modes::all_modes(&settings).into_iter().map(|m| ModeView { id: m.id, name: m.name, description: m.description, icon: m.icon, builtin: m.builtin, tools: match m.tools { modes::ToolFilter::All => Value::String("all".into()), modes::ToolFilter::Readonly => Value::String("readonly".into()), modes::ToolFilter::Only { names } => Value::Array(names.into_iter().map(Value::String).collect()) } }).collect())
}

fn env_for<'a>(app: &'a AppHandle, mgr: &'a AgentManager, settings: &'a crate::core::settings::Settings, files: Arc<Vec<String>>) -> ContextEnv<'a> {
    let root = mgr.workspace_root();
    let cwd = root.clone().unwrap_or_else(crate::paths::home_dir);
    ContextEnv { app: Some(app), root: root.clone(), cwd, settings, ws_key: crate::core::sessions::workspace_key(root.as_deref()), store: &mgr.store, files }
}

#[tauri::command]
pub async fn chat_context_preview(app: AppHandle, mgr: Mgr<'_>, items: Vec<ContextItem>) -> Result<ContextPreview> {
    let settings = mgr.settings();
    let root = mgr.workspace_root();
    let files = match &root { Some(r) => mgr.file_list(r).await, None => Arc::new(vec![]) }; // no folder open → nothing to enumerate
    let env = env_for(&app, &mgr, &settings, files);
    Ok(context::preview(&env, &items).await)
}

#[tauri::command]
pub async fn chat_mention_candidates(app: AppHandle, mgr: Mgr<'_>, kind: String, query: String, limit: Option<usize>) -> Result<Vec<ContextItem>> {
    let settings = mgr.settings();
    let root = mgr.workspace_root();
    let files = match &root { Some(r) => mgr.file_list(r).await, None => Arc::new(vec![]) };
    let env = env_for(&app, &mgr, &settings, files);
    Ok(context::mention_candidates(&env, &kind, &query, limit.unwrap_or(30)).await)
}

#[tauri::command]
pub async fn chat_tool_catalog(mgr: Mgr<'_>) -> Result<Vec<ToolCatalogEntry>> {
    let mut v = mgr.registry.catalog();
    for t in crate::ai::agent::tools::dynamic::browser_tools() { v.push(ToolCatalogEntry { name: t.name().into(), display_name: t.display_name().into(), description: t.description(), group: t.group().into(), readonly: false }); }
    for t in crate::ai::agent::tools::dynamic::mcp_tools().await { v.push(ToolCatalogEntry { name: t.name().into(), display_name: t.display_name().into(), description: t.description(), group: "mcp".into(), readonly: false }); }
    Ok(v)
}

/// Used by `plans_build`: start an agent session that implements a plan file. Returns the session id.
pub async fn start_plan_build(app: &AppHandle, plan_path: &str, session_id: Option<String>, model_id: Option<String>, use_worktree: bool) -> Result<String> {
    let mgr = app.state::<Arc<AgentManager>>().inner().clone();
    let content = plans::read(plan_path).unwrap_or_default();
    let title = content.lines().find(|l| l.starts_with("# ")).map(|l| l.trim_start_matches("# ").to_string()).unwrap_or_else(|| "Build plan".into());
    let h = match session_id { Some(id) => mgr.handle(&id)?, None => mgr.create_session(Some("agent".into()), model_id, Some(format!("Build: {title}")), use_worktree, None).await? };
    let _ = plans::set_status(plan_path, "building");
    let sid = h.id();
    mgr.submit(SendMessageRequest { session_id: sid.clone(), text: format!("Implement the following plan completely. Work through the steps in order, verify as you go, and report what was done.\n\nPlan file: {plan_path}\n\n{content}"), mode: "agent".into(), context: vec![ContextItem { id: format!("plan:{plan_path}"), kind: "plan".into(), label: title, path: Some(plan_path.to_string()), ..Default::default() }], ..Default::default() }).await?;
    Ok(sid)
}

//! The agentic loop: prompt → stream → tool calls (policy/approvals/hooks) → results → repeat.
use crate::ai::agent::context::{self, ContextEnv};
use crate::ai::agent::hooks::{self, HookEvent, HookPermission};
use crate::ai::agent::manager::{AgentManager, SessionHandle};
use crate::ai::agent::modes::{self, ModeSpec};
use crate::ai::agent::policy::{self, Decision, PolicyInput, ToolEffect};
use crate::ai::agent::prompt::{self, PromptContext};
use crate::ai::agent::session::*;
use crate::ai::agent::tools::{self, cap_output, Tool, ToolContext, ToolOutput};
use crate::ai::agent::{commands as slash, summarize};
use crate::ai::llm::{self, ContentBlock, Effort, LlmEvent, LlmRequest, Message as LlmMessage, ResolvedModel, Role, StopReason, SystemBlock, ThinkingConfig, ToolResultContent};
use crate::core::diagnostics::DiagnosticsStore;
use crate::core::settings::{Settings, SettingsState};
use crate::error::{CaretError, Result};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::Manager;
use tokio_util::sync::CancellationToken;

fn resolve(mgr: &AgentManager, settings: &Settings, model_id: Option<&str>) -> Result<ResolvedModel> {
    match mgr.app_opt() { Some(app) => { let secrets = app.state::<SettingsState>(); llm::resolve_model(settings, &secrets, model_id) } None => llm::resolve_model(settings, &SettingsState::load(), model_id) }
}

fn lint_summary(app: &Option<tauri::AppHandle>, paths: Option<&[String]>, max: usize) -> String {
    match app { Some(a) => a.state::<DiagnosticsStore>().summary(paths, max), None => String::new() }
}

fn update_tool<F: FnOnce(&mut ToolCallRecord)>(mgr: &AgentManager, h: &SessionHandle, message_id: &str, call_id: &str, f: F) {
    let (sid, rec) = { let mut s = h.session.lock(); let sid = s.id.clone(); match s.find_tool_mut(message_id, call_id) { Some(r) => { f(r); (sid, Some(r.clone())) } None => (sid, None) } };
    if let Some(call) = rec { mgr.emit(AgentEvent::ToolUpdate { session_id: sid, message_id: message_id.to_string(), call }); }
}

fn end_message(mgr: &AgentManager, h: &SessionHandle, message_id: &str, status: MessageStatus, error: Option<String>, started: std::time::Instant) {
    let sid = { let mut s = h.session.lock(); if let Some(m) = s.message_mut(message_id) { m.status = Some(status); m.duration_ms = Some(started.elapsed().as_millis() as u64); if let Some(e) = &error { if !m.parts.iter().any(|p| matches!(p, MessagePart::Error { .. })) { m.parts.push(MessagePart::Error { message: e.clone() }); } } } s.touch(); s.id.clone() };
    mgr.emit(AgentEvent::MessageEnd { session_id: sid, message_id: message_id.to_string(), status, error, duration_ms: started.elapsed().as_millis() as u64 });
    mgr.persist(h);
}

fn volatile_block(settings: &Settings, s: &Session, model: &str, req: &SendMessageRequest) -> SystemBlock {
    let mut t = String::from("<environment>\n");
    t.push_str(&format!("Date: {}\n", chrono::Local::now().format("%Y-%m-%d (%A)")));
    t.push_str(&format!("OS: {} ({}); shell: {}\n", std::env::consts::OS, std::env::consts::ARCH, tools::shell_tools::user_shell(settings)));
    if !s.workspace_root.is_empty() { t.push_str(&format!("Workspace root: {}\n", s.workspace_root)); }
    if s.worktree_path.is_some() { t.push_str(&format!("You are working in an isolated git worktree: {} (branch {}). Paths resolve relative to it.\n", s.cwd, s.branch.clone().unwrap_or_default())); }
    t.push_str(&format!("Model: {model}\n"));
    if let Some(a) = &req.active_file { t.push_str(&format!("User's active editor file: {a}\n")); }
    if let Some(o) = &req.open_files { if !o.is_empty() { t.push_str(&format!("Open editor tabs: {}\n", o.iter().take(12).cloned().collect::<Vec<_>>().join(", "))); } }
    t.push_str("</environment>\n");
    if !s.todos.is_empty() { t.push_str("<todos>\n"); for td in &s.todos { t.push_str(&format!("- [{}] {} (id: {})\n", td.status, td.content, td.id)); } t.push_str("</todos>\n"); }
    SystemBlock { text: t, cache: false }
}

fn session_allow_pattern(kind: ApprovalKind, pattern: Option<&str>) -> Vec<String> {
    match kind {
        ApprovalKind::Command => pattern.map(|p| vec![format!("cmd:{p}")]).unwrap_or_default(),
        ApprovalKind::Mcp => pattern.map(|p| vec![format!("mcp:{p}")]).unwrap_or_default(),
        ApprovalKind::FileEdit => vec!["edits".into(), "dotfiles".into()],
        ApprovalKind::FileDelete => vec!["delete".into(), "edits".into()],
        ApprovalKind::OutsideWorkspace => vec!["outside-workspace".into()],
        ApprovalKind::Browser => vec!["browser-eval".into()],
        ApprovalKind::Web => vec!["web".into()],
        ApprovalKind::Subagent => vec![],
    }
}

fn persist_always(mgr: &AgentManager, kind: ApprovalKind, pattern: Option<&str>, deny: bool) {
    let Some(app) = mgr.app_opt() else { return };
    let st = app.state::<SettingsState>();
    let eff = st.effective();
    let patch = match (kind, pattern) {
        (ApprovalKind::Command, Some(p)) => { if deny { let mut l = eff.agent.command_denylist.clone(); if !l.contains(&p.to_string()) { l.push(p.to_string()); } json!({"agent": {"commandDenylist": l}}) } else { let mut l = eff.agent.command_allowlist.clone(); if !l.contains(&p.to_string()) { l.push(p.to_string()); } json!({"agent": {"commandAllowlist": l}}) } }
        (ApprovalKind::Mcp, Some(p)) if !deny => { let mut l = eff.agent.auto_run_mcp_tools.clone(); if !l.contains(&p.to_string()) { l.push(p.to_string()); } json!({"agent": {"autoRunMcpTools": l}}) }
        _ => return,
    };
    if let Ok(s) = st.patch(&patch, "user") { crate::events::emit(&app, crate::events::SETTINGS_CHANGED, s); }
}

pub(crate) struct ToolRun { pub id: String, pub name: String, pub input: Value, pub tool: Option<Arc<dyn Tool>> }

pub(crate) async fn execute_one(mgr: Arc<AgentManager>, h: Arc<SessionHandle>, ctx: ToolContext, run: ToolRun) -> (String, ToolOutput) {
    let Some(tool) = run.tool.clone() else {
        update_tool(&mgr, &h, &ctx.message_id, &run.id, |r| { r.status = ToolStatus::Error; r.error = Some("unknown tool".into()); r.finished_at = Some(now_ms()); });
        return (run.id, ToolOutput::error(format!("Unknown tool `{}`. Available tools are listed in your tool definitions.", run.name)));
    };
    let effect = tool.effect(&run.input);
    // hooks for shell commands
    if let ToolEffect::Shell(cmd) = &effect {
        let hk = hooks::run(ctx.root.as_deref(), HookEvent::BeforeShellExecution, json!({ "command": cmd, "cwd": ctx.cwd.to_string_lossy(), "session_id": ctx.session_id })).await;
        if hk.permission == HookPermission::Deny { update_tool(&mgr, &h, &ctx.message_id, &run.id, |r| { r.status = ToolStatus::Rejected; r.error = Some("blocked by hook".into()); r.finished_at = Some(now_ms()); }); return (run.id, ToolOutput::error(format!("Command blocked by a project hook{}", hk.user_message.map(|m| format!(": {m}")).unwrap_or_default()))); }
    }
    // Deterministic risk classification for shell commands (shown on the card whether or not approval is needed).
    let shell_class = if let ToolEffect::Shell(c) = &effect { Some(crate::sandbox::classify_with(c, Some(&ctx.cwd), ctx.root.as_deref(), &ctx.settings.agent.command_allowlist, &ctx.settings.agent.command_denylist)) } else { None };
    let decision = { let s = h.session.lock(); policy::decide(&PolicyInput { settings: &ctx.settings, session: &s, tool_name: tool.name(), mode_auto_run: ctx.mode.auto_run }, &effect) };
    match decision {
        Decision::Deny(reason) => { update_tool(&mgr, &h, &ctx.message_id, &run.id, |r| { r.status = ToolStatus::Rejected; r.error = Some(reason.clone()); r.finished_at = Some(now_ms()); }); return (run.id, ToolOutput::error(format!("Denied by policy: {reason}"))); }
        Decision::NeedsApproval { kind, title, detail, allow_always_pattern, classification } => {
            let options = match kind { ApprovalKind::Command => vec![ApprovalOption::Allow, ApprovalOption::AllowSession, ApprovalOption::AllowAlways, ApprovalOption::Deny, ApprovalOption::DenyAlways], ApprovalKind::Mcp => vec![ApprovalOption::Allow, ApprovalOption::AllowSession, ApprovalOption::AllowAlways, ApprovalOption::Deny], _ => vec![ApprovalOption::Allow, ApprovalOption::AllowSession, ApprovalOption::Deny] };
            let (command, cwd) = if let ToolEffect::Shell(c) = &effect { (Some(c.clone()), Some(ctx.cwd.to_string_lossy().to_string())) } else { (None, None) };
            let req = ApprovalRequest { approval_id: new_id(), session_id: ctx.session_id.clone(), tool_call_id: run.id.clone(), kind, title, detail, command, cwd, options, allow_always_pattern: allow_always_pattern.clone(), classification: classification.clone().or_else(|| shell_class.clone()), risk_tier: classification.as_ref().or(shell_class.as_ref()).map(|c| c.risk_tier()), sandboxed: if matches!(effect, ToolEffect::Shell(_)) { Some(crate::sandbox::will_sandbox(&ctx.settings, ctx.root.as_deref())) } else { None }, agent_id: None };
            match mgr.request_approval(&h, &ctx.message_id, req, &ctx.cancel).await {
                Ok(ApprovalOption::Allow) => {}
                Ok(ApprovalOption::AllowSession) => { let pats = session_allow_pattern(kind, allow_always_pattern.as_deref()); let mut s = h.session.lock(); for p in pats { if !s.session_allow.contains(&p) { s.session_allow.push(p); } } }
                Ok(ApprovalOption::AllowAlways) => { persist_always(&mgr, kind, allow_always_pattern.as_deref(), false); let pats = session_allow_pattern(kind, allow_always_pattern.as_deref()); let mut s = h.session.lock(); for p in pats { if !s.session_allow.contains(&p) { s.session_allow.push(p); } } }
                Ok(opt @ (ApprovalOption::Deny | ApprovalOption::DenyAlways)) => {
                    if opt == ApprovalOption::DenyAlways { persist_always(&mgr, kind, allow_always_pattern.as_deref(), true); }
                    update_tool(&mgr, &h, &ctx.message_id, &run.id, |r| { r.status = ToolStatus::Rejected; r.error = Some("denied by user".into()); r.finished_at = Some(now_ms()); r.approval = None; });
                    return (run.id, ToolOutput::error("The user denied this action. Do not retry it; ask the user how to proceed or choose another approach."));
                }
                Err(_) => { update_tool(&mgr, &h, &ctx.message_id, &run.id, |r| { r.status = ToolStatus::Cancelled; r.finished_at = Some(now_ms()); r.approval = None; }); return (run.id, ToolOutput::error("Cancelled by user")); }
            }
            // Approving a changed MCP tool re-pins its current description.
            if let ToolEffect::Mcp { server_id, tool } = &effect { crate::sandbox::mcp_pins::acknowledge(server_id, tool); }
        }
        Decision::Allow => {}
    }
    update_tool(&mgr, &h, &ctx.message_id, &run.id, |r| { r.status = ToolStatus::Running; r.approval = None; r.started_at = Some(now_ms()); r.classification = shell_class.clone(); });
    let out = tokio::select! {
        r = tool.run(&ctx, run.input.clone()) => match r { Ok(o) => o, Err(e) => ToolOutput::error(format!("Tool error: {}", e.message)) },
        _ = ctx.cancel.cancelled() => ToolOutput::error("Cancelled by user"),
    };
    let max = ctx.max_output();
    let (capped, truncated) = cap_output(&out.text, max);
    let cancelled = ctx.cancel.is_cancelled();
    update_tool(&mgr, &h, &ctx.message_id, &run.id, |r| {
        r.status = if cancelled { ToolStatus::Cancelled } else if out.is_error { ToolStatus::Error } else { ToolStatus::Done };
        r.output = Some(capped.clone()); r.output_truncated = Some(truncated); r.finished_at = Some(now_ms());
        if out.is_error { r.error = Some(capped.lines().next().unwrap_or("error").chars().take(300).collect()); }
        if !out.file_changes.is_empty() { r.file_changes = Some(out.file_changes.clone()); }
        r.command = out.command.clone(); r.exit_code = out.exit_code; r.diff = out.diff.clone(); r.subagent_session_id = out.subagent_session_id.clone(); r.summary = out.summary.clone().or_else(|| tool.label(&run.input));
        if out.is_background { r.is_background = Some(true); }
        if out.command.is_some() { r.sandboxed = Some(out.sandboxed); }
    });
    (run.id, ToolOutput { text: capped, ..out })
}

/// Run one user turn to completion (may span many LLM round-trips). Returns the final assistant text.
pub async fn run_turn(mgr: Arc<AgentManager>, h: Arc<SessionHandle>, mut req: SendMessageRequest, mode_override: Option<ModeSpec>, cancel: CancellationToken) -> Result<String> {
    let app = mgr.app_opt();
    let settings = mgr.settings();
    let (session_id, root, cwd, is_subagent) = { let s = h.session.lock(); (s.id.clone(), s.root(), s.cwd_path(), s.is_subagent) };
    let mode: Arc<ModeSpec> = Arc::new(match &mode_override { Some(m) => m.clone(), None => { let id = if req.mode.is_empty() { h.session.lock().mode.clone() } else { req.mode.clone() }; { let mut s = h.session.lock(); s.mode = id.clone(); } modes::resolve(&settings, &id) } });
    if let Some(m) = &req.model_id { if !m.is_empty() { h.session.lock().model_id = Some(m.clone()); } }
    let model_pref = { let s = h.session.lock(); s.model_id.clone() }.or_else(|| mode.model_id.clone());
    let resolved = match resolve(&mgr, &settings, model_pref.as_deref()) {
        Ok(r) => r,
        Err(e) => { mgr.emit(AgentEvent::Error { session_id: session_id.clone(), message: e.message.clone() }); mgr.set_status(&h, SessionStatus::Error); return Err(e); }
    };
    mgr.set_status(&h, SessionStatus::Running);

    // Hooks: beforeSubmitPrompt
    let hk = hooks::run(root.as_deref(), HookEvent::BeforeSubmitPrompt, json!({ "prompt": req.text, "session_id": session_id, "mode": mode.id, "attachments": req.context.iter().map(|c| c.label.clone()).collect::<Vec<_>>() })).await;
    if hk.permission == HookPermission::Deny { let msg = hk.user_message.unwrap_or_else(|| "Prompt blocked by a project hook".into()); mgr.emit(AgentEvent::Error { session_id: session_id.clone(), message: msg.clone() }); return Err(CaretError::denied(msg)); }
    if let Some(Value::Object(o)) = &hk.replaced_input { if let Some(p) = o.get("prompt").and_then(|v| v.as_str()) { req.text = p.to_string(); } }
    // Slash commands
    let display_text = req.text.clone();
    if req.text.trim_start().starts_with('/') {
        let t = req.text.trim_start(); let (name, args) = t[1..].split_once(char::is_whitespace).map(|(a, b)| (a.to_string(), b.trim().to_string())).unwrap_or((t[1..].to_string(), String::new()));
        if name == "summarize" { if let Err(e) = compact_now(&mgr, &h, &resolved, &cancel).await { tracing::warn!(error=%e, "summarize failed"); } return Ok(String::new()); }
        if let Some(expanded) = slash::expand(root.as_deref(), &name, &args) { req.text = expanded; }
    }

    // Context
    let files = mgr.file_list(&cwd).await;
    let env = ContextEnv { app: app.as_ref(), root: root.clone(), cwd: cwd.clone(), settings: &settings, ws_key: crate::core::sessions::workspace_key(root.as_deref()), store: &mgr.store, files };
    let mut items = req.context.clone();
    if let Some(sel) = &req.cursor_selection { if !items.iter().any(|i| i.kind == "selection" || i.kind == "code") { items.push(ContextItem { id: "selection".into(), kind: "selection".into(), label: format!("{}:{}-{}", sel.path.rsplit('/').next().unwrap_or(&sel.path), sel.range.start_line, sel.range.end_line), path: Some(sel.path.clone()), range: Some(sel.range.clone()), data: Some(sel.text.clone()), ..Default::default() }); } }
    let built = context::build(&env, &items).await;
    let mentioned_rule_ids = built.rule_ids.clone();

    // User message + checkpoint
    let turn_id = short_id();
    let mut user_msg = ChatMessage::new(MessageRole::User);
    user_msg.turn_id = Some(turn_id.clone()); user_msg.mode = Some(mode.id.clone()); user_msg.checkpoint_id = Some(turn_id.clone());
    if !items.is_empty() { user_msg.parts.push(MessagePart::Context { items: items.clone() }); }
    for img in &req.images { user_msg.parts.push(MessagePart::Image { image: img.clone() }); }
    user_msg.parts.push(MessagePart::Text { text: display_text.clone() });
    let user_msg_id = user_msg.id.clone();
    let checkpoint = Checkpoint { id: turn_id.clone(), session_id: session_id.clone(), message_id: user_msg_id.clone(), created_at: now_ms(), files: vec![], label: display_text.chars().take(60).collect() };
    {
        let mut s = h.session.lock();
        s.ensure_transcript_model(&resolved.model.model);
        let tlen = s.transcript.len();
        s.transcript_marks.push((turn_id.clone(), tlen));
        s.checkpoints.push(checkpoint);
        s.messages.push(user_msg.clone());
        s.current_turn = Some(turn_id.clone());
        let mut content = vec![];
        if !built.text.trim().is_empty() { content.push(ContentBlock::Text { text: format!("<attached_context>\n{}</attached_context>", built.text) }); }
        for (m, d) in &built.images { content.push(ContentBlock::Image { media_type: m.clone(), data: d.clone() }); }
        for img in &req.images { content.push(ContentBlock::Image { media_type: img.mime.clone(), data: img.data_base64.clone() }); }
        content.push(ContentBlock::Text { text: if req.text.trim().is_empty() { "(see attachments)".into() } else { req.text.clone() } });
        s.transcript.push(LlmMessage { role: Role::User, content });
        s.touch();
    }
    mgr.emit(AgentEvent::MessageStart { session_id: session_id.clone(), message: user_msg });
    mgr.emit(AgentEvent::Checkpoint { session_id: session_id.clone(), checkpoint_id: turn_id.clone(), message_id: user_msg_id.clone() });
    mgr.persist(&h);

    // Tools
    let active = tools::active_tools(&mgr.registry, &mode, &settings, is_subagent).await;
    let tool_map: HashMap<String, Arc<dyn Tool>> = active.iter().map(|t| (t.name().to_string(), t.clone())).collect();
    let tool_defs = tools::to_defs(&active);
    let tool_names: Vec<String> = active.iter().map(|t| t.name().to_string()).collect();
    let mut edited_files: HashSet<String> = HashSet::new();
    let lint_before: HashSet<String> = lint_summary(&app, None, 500).lines().map(String::from).collect();
    let mut lint_rounds = 0u32; let mut stop_hook_rounds = 0u32;
    let thinking_pref = req.thinking.clone().unwrap_or_else(|| settings.agent.thinking.clone());
    let effort = Effort::parse(req.effort.as_deref().unwrap_or(&settings.agent.effort));
    let max_tokens = if settings.agent.large_context_mode { resolved.model.max_output.min(64_000) } else { resolved.model.max_output.min(32_000) }.max(4096);
    let mut final_text = String::new();
    let mut iterations = 0u32;
    let max_iter = settings.agent.max_iterations.max(4);

    'outer: loop {
        if cancel.is_cancelled() { break; }
        if iterations >= max_iter { let m = ChatMessage::new(MessageRole::Assistant); let mid = m.id.clone(); { h.session.lock().messages.push(m.clone()); } mgr.emit(AgentEvent::MessageStart { session_id: session_id.clone(), message: m }); end_message(&mgr, &h, &mid, MessageStatus::Error, Some(format!("Stopped after {max_iter} tool iterations. Send a message to continue.")), std::time::Instant::now()); break; }
        iterations += 1;

        // Compaction
        let ctx_tokens = { let s = h.session.lock(); s.estimate_context_tokens() };
        if ctx_tokens as f32 > settings.agent.summarize_threshold_ratio * resolved.model.context_window as f32 && ctx_tokens > 20_000 {
            if let Err(e) = compact_now(&mgr, &h, &resolved, &cancel).await { tracing::warn!(error=%e, "auto compaction failed"); }
        }

        // System prompt
        let (open_files, transcript, todos_nonempty) = { let s = h.session.lock(); (req.open_files.clone().unwrap_or_default(), s.transcript.clone(), !s.todos.is_empty()) };
        let _ = todos_nonempty;
        let mut system = prompt::build_system_prompt(&PromptContext { root: root.as_deref(), mode: &mode, settings: &settings, active_file: req.active_file.as_deref(), open_files: &open_files, mentioned_rule_ids: &mentioned_rule_ids, os: std::env::consts::OS, shell: &tools::shell_tools::user_shell(&settings), model_name: &resolved.model.display_name, tool_names: &tool_names, is_subagent, worktree: { let s = h.session.lock(); s.worktree_path.clone() }.as_deref().map(std::path::Path::new) });
        { let s = h.session.lock(); system.push(volatile_block(&settings, &s, &resolved.model.display_name, &req)); }

        let mut llm_req = LlmRequest::new(resolved.model.clone());
        llm_req.system = system; llm_req.messages = transcript; llm_req.tools = tool_defs.clone(); llm_req.max_tokens = max_tokens;
        llm_req.thinking = ThinkingConfig { enabled: resolved.model.supports_thinking && thinking_pref != "off", effort, summarized: true };
        llm_req.parallel_tool_calls = settings.agent.parallel_tool_calls; llm_req.purpose = "agent".into();

        // Assistant message
        let mut amsg = ChatMessage::new(MessageRole::Assistant);
        amsg.status = Some(MessageStatus::Streaming); amsg.model_id = Some(resolved.model.id.clone()); amsg.mode = Some(mode.id.clone()); amsg.turn_id = Some(turn_id.clone());
        let message_id = amsg.id.clone();
        { h.session.lock().messages.push(amsg.clone()); }
        mgr.emit(AgentEvent::MessageStart { session_id: session_id.clone(), message: amsg });
        let started = std::time::Instant::now();

        let provider = match llm::provider_for(&resolved) { Ok(p) => p, Err(e) => { end_message(&mgr, &h, &message_id, MessageStatus::Error, Some(e.message.clone()), started); mgr.set_status(&h, SessionStatus::Error); return Err(e); } };
        let (mgr2, h2, sid2, mid2, tool_map2) = (mgr.clone(), h.clone(), session_id.clone(), message_id.clone(), tool_map.clone());
        let mut slots: HashMap<u32, usize> = HashMap::new();
        let mut call_ids: HashMap<u32, String> = HashMap::new();
        let on_event = move |ev: &LlmEvent| {
            match ev {
                LlmEvent::TextDelta { index, text } => {
                    let mut s = h2.session.lock();
                    if let Some(m) = s.message_mut(&mid2) {
                        let pos = *slots.entry(*index).or_insert_with(|| { m.parts.push(MessagePart::Text { text: String::new() }); m.parts.len() - 1 });
                        if let Some(MessagePart::Text { text: t }) = m.parts.get_mut(pos) { t.push_str(text); }
                    }
                    drop(s);
                    mgr2.emit(AgentEvent::TextDelta { session_id: sid2.clone(), message_id: mid2.clone(), text: text.clone() });
                }
                LlmEvent::ThinkingDelta { index, text } => {
                    let mut s = h2.session.lock();
                    if let Some(m) = s.message_mut(&mid2) {
                        let pos = *slots.entry(*index).or_insert_with(|| { m.parts.push(MessagePart::Thinking { text: String::new(), duration_ms: None, redacted: None }); m.parts.len() - 1 });
                        if let Some(MessagePart::Thinking { text: t, .. }) = m.parts.get_mut(pos) { t.push_str(text); }
                    }
                    drop(s);
                    if !text.is_empty() { mgr2.emit(AgentEvent::ThinkingDelta { session_id: sid2.clone(), message_id: mid2.clone(), text: text.clone() }); }
                }
                LlmEvent::RedactedThinking { index, .. } => { let mut s = h2.session.lock(); if let Some(m) = s.message_mut(&mid2) { m.parts.push(MessagePart::Thinking { text: String::new(), duration_ms: None, redacted: Some(true) }); slots.insert(*index, m.parts.len() - 1); } }
                LlmEvent::ToolUseStart { index, id, name } => {
                    let display = tool_map2.get(name).map(|t| t.display_name().to_string()).unwrap_or_else(|| name.clone());
                    let rec = ToolCallRecord::new(id, name, &display);
                    { let mut s = h2.session.lock(); if let Some(m) = s.message_mut(&mid2) { m.parts.push(MessagePart::Tool { call: rec.clone() }); slots.insert(*index, m.parts.len() - 1); } }
                    call_ids.insert(*index, id.clone());
                    mgr2.emit(AgentEvent::ToolStart { session_id: sid2.clone(), message_id: mid2.clone(), call: rec });
                }
                LlmEvent::ToolInputDelta { index, partial_json } => {
                    if let Some(cid) = call_ids.get(index) {
                        { let mut s = h2.session.lock(); if let Some(r) = s.find_tool_mut(&mid2, cid) { let t = r.input_text.get_or_insert_with(String::new); if t.len() < 200_000 { t.push_str(partial_json); } } }
                        mgr2.emit(AgentEvent::ToolInputDelta { session_id: sid2.clone(), message_id: mid2.clone(), tool_call_id: cid.clone(), text: partial_json.clone() });
                    }
                }
                _ => {}
            }
        };
        let turn = match llm::run_stream(provider, &llm_req, cancel.clone(), on_event).await {
            Ok(t) => t,
            Err(e) => {
                if cancel.is_cancelled() || e.code == "cancelled" { end_message(&mgr, &h, &message_id, MessageStatus::Cancelled, None, started); break 'outer; }
                end_message(&mgr, &h, &message_id, MessageStatus::Error, Some(e.message.clone()), started);
                mgr.set_status(&h, SessionStatus::Error);
                return Err(e);
            }
        };
        // usage
        let cost = turn.usage.cost_usd(&resolved.model);
        let usage = Usage::from_delta(&turn.usage, cost);
        let (session_usage, ctx_tokens) = { let mut s = h.session.lock(); s.usage.add(&usage); if let Some(m) = s.message_mut(&message_id) { m.usage = Some(usage.clone()); } s.context_tokens = turn.usage.input_tokens + turn.usage.cache_read_tokens + turn.usage.cache_write_tokens + turn.usage.output_tokens; (s.usage.clone(), s.context_tokens) };
        mgr.emit(AgentEvent::Usage { session_id: session_id.clone(), message_id: message_id.clone(), usage, context_tokens: ctx_tokens, session_usage });

        let tool_uses = turn.tool_uses();
        for (id, _, input) in &tool_uses { update_tool(&mgr, &h, &message_id, id, |r| { r.input = input.clone(); r.summary = tool_map.get(&r.name).and_then(|t| t.label(input)); }); }

        match turn.stop {
            Some(StopReason::Refusal) => { end_message(&mgr, &h, &message_id, MessageStatus::Error, Some(format!("The model declined to continue{}.", turn.stop_detail.as_ref().map(|d| format!(" ({d})")).unwrap_or_default())), started); break 'outer; }
            Some(StopReason::MaxTokens) if !tool_uses.is_empty() => { end_message(&mgr, &h, &message_id, MessageStatus::Error, Some("The response was cut off by the output token limit while calling a tool. Send \"continue\" to retry.".into()), started); break 'outer; }
            _ => {}
        }
        if tool_uses.is_empty() {
            { let mut s = h.session.lock(); if !turn.content.is_empty() { s.transcript.push(LlmMessage { role: Role::Assistant, content: turn.content.clone() }); } }
            final_text = turn.text();
            end_message(&mgr, &h, &message_id, MessageStatus::Done, None, started);

            // after-response hooks / lint fix / stop hook
            let _ = hooks::run(root.as_deref(), HookEvent::AfterAgentResponse, json!({ "text": final_text, "session_id": session_id })).await;
            if settings.agent.auto_fix_lints && !edited_files.is_empty() && lint_rounds < 3 && !cancel.is_cancelled() {
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                let paths: Vec<String> = edited_files.iter().cloned().collect();
                let now: Vec<String> = lint_summary(&app, Some(&paths), 60).lines().map(String::from).filter(|l| l.contains("[error]")).collect();
                let new_errors: Vec<String> = now.into_iter().filter(|l| !lint_before.contains(l)).collect();
                if !new_errors.is_empty() {
                    lint_rounds += 1;
                    let note = format!("Linter reported {} error(s) in files you edited:\n{}\n\nFix them (read the surrounding code first). Do not disable the rules.", new_errors.len(), new_errors.join("\n"));
                    let mut sm = ChatMessage::new(MessageRole::System); sm.parts.push(MessagePart::Text { text: format!("Auto-fixing {} linter error(s)…", new_errors.len()) }); sm.turn_id = Some(turn_id.clone());
                    { let mut s = h.session.lock(); s.messages.push(sm.clone()); s.transcript.push(LlmMessage::user_text(note)); }
                    mgr.emit(AgentEvent::MessageStart { session_id: session_id.clone(), message: sm });
                    continue 'outer;
                }
            }
            if stop_hook_rounds < 1 {
                let hk = hooks::run(root.as_deref(), HookEvent::Stop, json!({ "text": final_text, "session_id": session_id, "iterations": iterations })).await;
                if let Some(fp) = hk.followup_prompt.filter(|f| !f.trim().is_empty()) {
                    stop_hook_rounds += 1;
                    let mut sm = ChatMessage::new(MessageRole::System); sm.parts.push(MessagePart::Text { text: format!("Hook follow-up: {fp}") }); sm.turn_id = Some(turn_id.clone());
                    { let mut s = h.session.lock(); s.messages.push(sm.clone()); s.transcript.push(LlmMessage::user_text(fp)); }
                    mgr.emit(AgentEvent::MessageStart { session_id: session_id.clone(), message: sm });
                    continue 'outer;
                }
            }
            break 'outer;
        }

        // ── execute tools ──
        let runs: Vec<ToolRun> = tool_uses.iter().map(|(id, name, input)| ToolRun { id: id.clone(), name: name.clone(), input: input.clone(), tool: tool_map.get(name).cloned() }).collect();
        let make_ctx = |call_id: &str| ToolContext { app: app.clone(), manager: mgr.clone(), session_id: session_id.clone(), message_id: message_id.clone(), tool_call_id: call_id.to_string(), turn_id: turn_id.clone(), root: root.clone(), cwd: cwd.clone(), settings: settings.clone(), cancel: cancel.clone(), mode: mode.clone() };
        let mut results: HashMap<String, ToolOutput> = HashMap::new();
        let (ro, rw): (Vec<ToolRun>, Vec<ToolRun>) = runs.into_iter().partition(|r| r.tool.as_ref().map(|t| t.readonly()).unwrap_or(true));
        if !ro.is_empty() {
            let futs: Vec<_> = ro.into_iter().map(|r| { let ctx = make_ctx(&r.id); execute_one(mgr.clone(), h.clone(), ctx, r) }).collect();
            for (id, out) in futures::future::join_all(futs).await { results.insert(id, out); }
        }
        for r in rw {
            if cancel.is_cancelled() { results.insert(r.id.clone(), ToolOutput::error("Cancelled by user")); update_tool(&mgr, &h, &message_id, &r.id, |rec| { rec.status = ToolStatus::Cancelled; }); continue; }
            let ctx = make_ctx(&r.id);
            let (id, out) = execute_one(mgr.clone(), h.clone(), ctx, r).await;
            results.insert(id, out);
        }
        let mut result_blocks = vec![];
        for (id, _, _) in &tool_uses {
            let out = results.remove(id).unwrap_or_else(|| ToolOutput::error("no result"));
            for fc in &out.file_changes { edited_files.insert(crate::paths::resolve(Some(&cwd), &fc.path).to_string_lossy().to_string()); }
            let mut content = vec![ToolResultContent::Text { text: if out.text.is_empty() { "(no output)".into() } else { out.text.clone() } }];
            for (m, d) in &out.images { content.push(ToolResultContent::Image { media_type: m.clone(), data: d.clone() }); }
            result_blocks.push(ContentBlock::ToolResult { tool_use_id: id.clone(), content, is_error: out.is_error });
        }
        { let mut s = h.session.lock(); s.transcript.push(LlmMessage { role: Role::Assistant, content: turn.content.clone() }); s.transcript.push(LlmMessage { role: Role::User, content: result_blocks }); s.context_tokens = s.estimate_context_tokens(); }
        end_message(&mgr, &h, &message_id, if cancel.is_cancelled() { MessageStatus::Cancelled } else { MessageStatus::Done }, None, started);
        if cancel.is_cancelled() { break 'outer; }
        if matches!(turn.stop, Some(StopReason::MaxTokens)) { break 'outer; }
    }

    { let mut s = h.session.lock(); s.current_turn = None; s.touch(); }
    mgr.persist_now(&h);
    // Verified Diff: once a turn edited files, run the proof checks in the background (never blocks the turn).
    if settings.verify.auto_run_after_turn && !edited_files.is_empty() && !cancel.is_cancelled() {
        if let Some(app2) = app.clone() { let sid = h.session.lock().id.clone(); let tid = turn_id.clone(); tauri::async_runtime::spawn(async move { crate::verify::auto_run_after_turn(app2, sid, tid).await; }); }
    }
    // Title
    let needs_title = { let s = h.session.lock(); !s.title_generated && !s.is_subagent && s.messages.iter().filter(|m| m.role == MessageRole::Assistant).count() >= 1 };
    if needs_title { let (mgr3, h3, r3) = (mgr.clone(), h.clone(), resolved.clone()); tauri::async_runtime::spawn(async move { generate_title(mgr3, h3, r3).await; }); }
    Ok(final_text)
}

pub async fn compact_now(mgr: &AgentManager, h: &SessionHandle, resolved: &ResolvedModel, cancel: &CancellationToken) -> Result<()> {
    let (sid, transcript) = { let s = h.session.lock(); (s.id.clone(), s.transcript.clone()) };
    mgr.set_status(h, SessionStatus::Summarizing);
    let res = summarize::compact(resolved, &transcript, cancel.clone()).await;
    mgr.set_status(h, SessionStatus::Running);
    if let Some((summary, new_t)) = res? {
        let mut sm = ChatMessage::new(MessageRole::System); sm.parts.push(MessagePart::Summary { text: summary.clone() });
        { let mut s = h.session.lock(); s.transcript = new_t; s.summary = Some(summary.clone()); s.messages.push(sm.clone()); s.context_tokens = s.estimate_context_tokens(); s.transcript_marks.clear(); }
        mgr.emit(AgentEvent::MessageStart { session_id: sid.clone(), message: sm });
        mgr.emit(AgentEvent::Summary { session_id: sid, text: summary });
        mgr.persist(h);
    }
    Ok(())
}

async fn generate_title(mgr: Arc<AgentManager>, h: Arc<SessionHandle>, resolved: ResolvedModel) {
    let (sid, user, assistant) = { let s = h.session.lock(); let u = s.messages.iter().find(|m| m.role == MessageRole::User).map(|m| m.preview(1500)).unwrap_or_default(); let a = s.messages.iter().find(|m| m.role == MessageRole::Assistant).map(|m| m.preview(800)).unwrap_or_default(); (s.id.clone(), u, a) };
    if user.trim().is_empty() { return; }
    let req = LlmRequest::new(resolved.model.clone()).system("Generate a concise 3-6 word title for this coding chat. Reply with the title only — no quotes, no trailing punctuation.").user(format!("User: {user}\n\nAssistant: {assistant}")).no_thinking().max_tokens(40).purpose("title");
    if let Ok((t, _)) = llm::complete_text(&resolved, req, CancellationToken::new()).await {
        let title = sanitize_title(&t).unwrap_or_else(|| user.split_whitespace().take(6).collect::<Vec<_>>().join(" "));
        if title.is_empty() { return; }
        { let mut s = h.session.lock(); if s.title_generated { return; } s.title = title.clone(); s.title_generated = true; }
        mgr.emit(AgentEvent::Title { session_id: sid, title });
        mgr.persist(&h);
    }
}

/// Pick a clean one-line title from model output: skips proxy/router banner lines (e.g. `*[Lynkr] … → …*`),
/// strips markdown emphasis, quotes and trailing punctuation, caps length.
pub fn sanitize_title(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let l = line.trim();
        if l.is_empty() { continue; }
        let core = l.trim_matches(|c: char| c == '*' || c == '_' || c == '`' || c == '"' || c == '\'' || c == '#').trim();
        if core.starts_with('[') && (core.contains('→') || core.contains("->")) { continue; } // router banner
        let core = if core.to_ascii_lowercase().starts_with("title:") { core[6..].trim() } else { core };
        let cleaned: String = core.chars().filter(|c| !matches!(c, '*' | '_' | '`' | '"')).collect();
        let cleaned = cleaned.trim().trim_end_matches(|c: char| c == '.' || c == '!').trim();
        if cleaned.is_empty() { continue; }
        return Some(clip(cleaned));
    }
    None
}
fn clip(s: &str) -> String { let t: String = s.chars().take(80).collect(); t }

#[cfg(test)]
mod title_tests {
    use super::sanitize_title;
    #[test]
    fn skips_router_banner_and_markdown() {
        assert_eq!(sanitize_title("*[Lynkr] MEDIUM → z-ai/glm (openrouter) · score 35*\n\nFix discount bug").as_deref(), Some("Fix discount bug"));
        assert_eq!(sanitize_title("\"Add retry logic.\"").as_deref(), Some("Add retry logic"));
        assert_eq!(sanitize_title("Title: **Refactor pricing**").as_deref(), Some("Refactor pricing"));
        assert_eq!(sanitize_title("*[X] a -> b*\n"), None);
    }
}

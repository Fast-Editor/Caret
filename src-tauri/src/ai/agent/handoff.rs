//! Thread forks, provider handoff and multi-model fan-out (T3 Code parity).
//!
//! * `plan_handoff` splits a message history into an older part to summarise and a recent part that travels
//!   verbatim, within a token budget (`chat.handoffBudgetTokens`, estimated as chars / 4).
//! * `fork_session` clones a session, optionally cut right after a message (exact per-turn cut via the
//!   transcript marks, rebuilt from the kept messages when the marks are gone).
//! * `handoff_session` moves a thread between the built-in agent, external ACP agents and models, transferring
//!   the context within the budget and leaving a muted system note in the thread.
//! * `fan_out` sends one prompt to several models at once, one session per model sharing a `fan_out_group`.
use crate::ai::agent::manager::{create_worktree, AgentManager, SessionHandle};
use crate::ai::agent::session::*;
use crate::ai::agent::summarize::SUMMARY_PROMPT;
use crate::ai::llm::{self, ContentBlock, LlmRequest, Message as LlmMessage, ResolvedModel, Role};
use crate::core::settings::Settings;
use crate::error::{CaretError, Result};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub const CARET_AGENT: &str = "caret";

/// Token estimate used for the handoff budget: chars / 4 (deliberately simple and provider neutral).
pub fn estimate_tokens(text: &str) -> usize { text.chars().count().div_ceil(4) }

fn clip_end(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars { return text.to_string(); }
    let mut s: String = text.chars().take(max_chars.saturating_sub(1)).collect(); s.push('…'); s
}
/// Keep head and tail of an oversized text with a marker in the middle.
pub fn clip_middle(text: &str, max_chars: usize) -> String {
    let n = text.chars().count();
    if n <= max_chars || max_chars < 16 { return text.to_string(); }
    let head = max_chars * 7 / 10; let tail = max_chars - head;
    let chars: Vec<char> = text.chars().collect();
    format!("{}\n… [{} characters omitted] …\n{}", chars[..head].iter().collect::<String>(), n - max_chars, chars[n - tail..].iter().collect::<String>())
}

// ───────────────────────── rendering ─────────────────────────

/// One transferable message: who said it and its text form (tool calls as brief lines).
#[derive(Debug, Clone, PartialEq)]
pub struct Rendered { pub role: MessageRole, pub text: String, pub is_summary: bool }
impl Rendered {
    pub fn who(&self) -> &'static str { match self.role { MessageRole::User => "User", MessageRole::Assistant => "Assistant", MessageRole::System => "Summary" } }
}

fn push_line(out: &mut String, s: &str) { if !out.is_empty() { out.push('\n'); } out.push_str(s); }

/// Render a UI message to text another agent or model can read. `None` for messages that carry nothing for the
/// next agent (empty messages, UI-only system notes such as handoff or lint notices).
pub fn render_message(m: &ChatMessage) -> Option<Rendered> {
    let mut out = String::new();
    let mut is_summary = false;
    for p in &m.parts {
        match p {
            MessagePart::Text { text } => { if !text.trim().is_empty() { push_line(&mut out, text.trim()); } }
            MessagePart::Summary { text } => { is_summary = true; push_line(&mut out, text.trim()); }
            MessagePart::Tool { call } => {
                let label = call.summary.clone().or_else(|| call.command.clone()).unwrap_or_default();
                let status = match call.status { ToolStatus::Error => " (error)", ToolStatus::Rejected => " (rejected)", ToolStatus::Cancelled => " (cancelled)", _ => "" };
                let label = if label.is_empty() { String::new() } else { format!(": {}", clip_end(&label, 120)) };
                push_line(&mut out, &format!("[tool {}{}{}]", call.display_name, label, status));
            }
            MessagePart::Context { items } => { if !items.is_empty() { push_line(&mut out, &format!("(attached: {})", items.iter().map(|i| i.label.as_str()).take(8).collect::<Vec<_>>().join(", "))); } }
            MessagePart::Image { .. } => push_line(&mut out, "[image]"),
            MessagePart::Error { message } => push_line(&mut out, &format!("[error: {}]", clip_end(message, 200))),
            MessagePart::Question { question, answer } => push_line(&mut out, &format!("[question: {}] [answer: {}]", question.question, answer.as_deref().unwrap_or("(none)"))),
            MessagePart::Plan { title, path } => push_line(&mut out, &format!("[plan: {title} ({path})]")),
            MessagePart::Thinking { .. } | MessagePart::Checkpoint { .. } => {}
        }
    }
    if m.role == MessageRole::System && !is_summary { return None; }
    if out.trim().is_empty() { return None; }
    Some(Rendered { role: m.role, text: out, is_summary })
}

// ───────────────────────── budget plan ─────────────────────────

/// How a history is transferred: older messages compressed into one summary, recent ones verbatim.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HandoffPlan {
    /// Older messages (chronological) to compress into a `<conversation_summary>` block.
    pub summarize: Vec<Rendered>,
    /// Recent messages (chronological) that travel verbatim.
    pub verbatim: Vec<Rendered>,
    pub budget_tokens: usize,
    /// Estimated tokens of the verbatim part.
    pub verbatim_tokens: usize,
}
impl HandoffPlan {
    pub fn total(&self) -> usize { self.summarize.len() + self.verbatim.len() }
    pub fn needs_summary(&self) -> bool { !self.summarize.is_empty() }
}

/// Tokens set aside for the summary block when older messages overflow the budget.
pub fn summary_reserve(budget: usize) -> usize { (budget / 4).clamp(200, 1500) }

fn cost(r: &Rendered) -> usize { estimate_tokens(&r.text) + 4 }

/// Split `messages` so the most recent ones fit `budget_tokens` verbatim; everything older is summarised.
/// The newest message is always kept verbatim (clipped on render when it alone exceeds the budget). When
/// anything overflows, part of the budget is reserved for the summary block.
pub fn plan_handoff(messages: &[ChatMessage], budget_tokens: usize) -> HandoffPlan {
    let rendered: Vec<Rendered> = messages.iter().filter_map(render_message).collect();
    let budget = budget_tokens.max(1);
    let fits = |limit: usize| -> usize {
        let mut used = 0usize; let mut n = 0usize;
        for r in rendered.iter().rev() {
            let t = cost(r);
            if n > 0 && used + t > limit { break; }
            used += t; n += 1;
            if used > limit { break; }
        }
        n
    };
    let mut keep = fits(budget);
    if keep < rendered.len() { keep = fits(budget.saturating_sub(summary_reserve(budget)).max(1)).max(1); }
    let split = rendered.len() - keep;
    let verbatim_tokens = rendered[split..].iter().map(cost).sum();
    HandoffPlan { summarize: rendered[..split].to_vec(), verbatim: rendered[split..].to_vec(), budget_tokens: budget, verbatim_tokens }
}

/// Deterministic fallback for the summary block: one line per older message, oldest first.
pub fn digest(older: &[Rendered], max_chars: usize) -> String {
    let mut out = String::new();
    for r in older {
        let line: String = r.text.split_whitespace().collect::<Vec<_>>().join(" ");
        out.push_str(&format!("- {}: {}\n", r.who(), clip_end(&line, 240)));
    }
    clip_middle(out.trim_end(), max_chars)
}

/// Plain-text rendering of messages for the summariser input.
pub fn render_text(msgs: &[Rendered], max_chars: usize) -> String {
    let mut out = String::new();
    for r in msgs { out.push_str(&format!("{}: {}\n\n", r.who().to_uppercase(), r.text)); }
    clip_middle(&out, max_chars)
}

/// Compress the plan's older messages with the model (same prompt as transcript compaction). Falls back to the
/// deterministic digest when no model is available or the call fails, so a handoff never blocks on the LLM.
pub async fn summarize_older(plan: &HandoffPlan, resolved: Option<&ResolvedModel>, cancel: CancellationToken) -> String {
    if let Some(r) = resolved {
        let req = LlmRequest::new(r.model.clone()).system(SUMMARY_PROMPT).user(format!("<conversation>\n{}\n</conversation>\n\nWrite the handoff summary.", render_text(&plan.summarize, 200_000))).no_thinking().max_tokens(2000).purpose("summarize");
        match llm::complete_text(r, req, cancel).await {
            Ok((t, _)) if !t.trim().is_empty() => return t.trim().to_string(),
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "handoff summary failed; using the digest fallback"),
        }
    }
    digest(&plan.summarize, summary_reserve(plan.budget_tokens) * 4)
}

fn summary_or_digest(plan: &HandoffPlan, summary: Option<&str>) -> String {
    summary.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| digest(&plan.summarize, summary_reserve(plan.budget_tokens) * 4))
}

/// `<previous_conversation>` block for a fresh external (ACP) session. `summary` covers `plan.summarize`.
pub fn history_block(plan: &HandoffPlan, summary: Option<&str>) -> Option<String> {
    if plan.total() == 0 { return None; }
    let mut out = String::new();
    if plan.needs_summary() {
        out.push_str(&format!("<conversation_summary note=\"{} earlier message(s) compacted\">\n{}\n</conversation_summary>\n\n", plan.summarize.len(), summary_or_digest(plan, summary)));
    }
    let max_one = plan.budget_tokens * 4;
    for r in &plan.verbatim { out.push_str(&format!("{}: {}\n\n", r.who(), clip_middle(&r.text, max_one))); }
    Some(format!("<previous_conversation note=\"earlier turns of this chat, for continuity; the thread was handed to you\">\n{out}</previous_conversation>"))
}

/// Rebuild an LLM transcript from UI messages: an optional summary exchange, then the verbatim messages as text
/// turns. Consecutive same-role messages are merged so providers that require alternation accept the result.
pub fn rebuild_transcript(plan: &HandoffPlan, summary: Option<&str>) -> Vec<LlmMessage> {
    let mut out: Vec<LlmMessage> = vec![];
    if plan.needs_summary() {
        out.push(LlmMessage::user_text(format!("<conversation_summary>\nThis is a summary of the earlier part of this conversation ({} older messages were compacted; call `read_thread` to read any of them verbatim):\n\n{}\n</conversation_summary>", plan.summarize.len(), summary_or_digest(plan, summary))));
        out.push(LlmMessage::assistant_text("Understood. I have the summary of our earlier work and will continue from here."));
    }
    let max_one = plan.budget_tokens * 4;
    for r in &plan.verbatim {
        let role = match r.role { MessageRole::Assistant => Role::Assistant, _ => Role::User };
        let text = if r.is_summary { format!("<conversation_summary>\n{}\n</conversation_summary>", r.text.trim()) } else { clip_middle(&r.text, max_one) };
        match out.last_mut() {
            Some(last) if last.role == role => last.content.push(ContentBlock::Text { text }),
            _ => out.push(LlmMessage { role, content: vec![ContentBlock::Text { text }] }),
        }
    }
    if matches!(out.first(), Some(m) if m.role == Role::Assistant) { out.insert(0, LlmMessage::user_text("(earlier context)")); }
    out
}

// ───────────────────────── forks ─────────────────────────

/// Clone `source` into a new idle session (not persisted or registered). With `message_id` the clone is cut right
/// after that message: messages up to and including it are kept and the LLM transcript is cut at the matching
/// point, exactly when the turn's transcript marks are intact, otherwise rebuilt from the kept messages.
pub fn fork_into(source: &Session, message_id: Option<&str>, budget_tokens: usize) -> Result<Session> {
    let mut s = source.clone();
    s.id = new_id(); s.created_at = now_ms(); s.updated_at = now_ms(); s.status = SessionStatus::Idle; s.current_turn = None;
    s.queued.clear(); s.worktree_path = None; s.branch = None; s.cwd = s.workspace_root.clone();
    s.checkpoints.clear(); s.changes.clear(); s.pinned = false; s.archived = false; s.review_comments.clear(); s.last_verification_id = None;
    s.parent_session_id = Some(source.id.clone()); s.is_subagent = false; s.fan_out_group = None; s.fork_of_message_id = None;
    s.usage = Usage::default();
    s.title = format!("Fork: {}", source.title);
    // File snapshots belong to the parent session: drop the restore links.
    for m in &mut s.messages { m.checkpoint_id = None; }

    if let Some(mid) = message_id {
        let idx = s.messages.iter().position(|m| m.id == mid).ok_or_else(|| CaretError::not_found("message"))?;
        let turn = s.messages[idx].turn_id.clone();
        let next_turn = s.messages[idx + 1..].iter().find_map(|m| m.turn_id.clone().filter(|t| Some(t) != turn.as_ref()));
        let last_of_turn = turn.is_none() || s.messages.get(idx + 1).map(|n| n.turn_id != turn).unwrap_or(true);
        let mark_of = |t: &Option<String>| t.as_ref().and_then(|t| s.transcript_marks.iter().find(|(x, _)| x == t).map(|(_, l)| *l));
        let turn_mark = mark_of(&turn);
        let end = match (&next_turn, mark_of(&next_turn)) { (None, _) => Some(s.transcript.len()), (Some(_), m) => m };
        s.messages.truncate(idx + 1);
        match (turn_mark, end) {
            (Some(start), Some(end)) if start <= end && end <= s.transcript.len() => {
                if last_of_turn { s.transcript.truncate(end); }
                else {
                    // Partial turn: keep the transcript before it and re-add the kept part of the turn as text.
                    let kept: Vec<ChatMessage> = s.messages.iter().filter(|m| m.turn_id == turn).cloned().collect();
                    s.transcript.truncate(start);
                    s.transcript.extend(rebuild_transcript(&plan_handoff(&kept, usize::MAX / 8), None));
                }
                // Keep the marks of earlier turns and of the kept turn itself (its transcript starts at `start`).
                s.transcript_marks.retain(|(t, l)| *l < start || Some(t) == turn.as_ref());
            }
            _ => {
                // Marks are gone (compaction) or the turn predates them: rebuild from what the fork shows.
                let plan = plan_handoff(&s.messages, budget_tokens);
                s.transcript = rebuild_transcript(&plan, s.summary.as_deref());
                s.transcript_model = None;
                s.transcript_marks.clear();
            }
        }
        s.fork_of_message_id = Some(mid.to_string());
    }
    s.summary = s.messages.iter().rev().find_map(|m| m.parts.iter().find_map(|p| if let MessagePart::Summary { text } = p { Some(text.clone()) } else { None }));
    s.context_tokens = s.estimate_context_tokens();
    Ok(s)
}

/// Fork a live session, register the clone with the manager and announce it (`SessionCreated`).
pub fn fork_session(mgr: &AgentManager, source: &SessionHandle, message_id: Option<&str>, budget_tokens: usize) -> Result<Arc<SessionHandle>> {
    let s = { let src = source.session.lock(); fork_into(&src, message_id, budget_tokens)? };
    let h = mgr.adopt(SessionHandle::new(s));
    Ok(h)
}

// ───────────────────────── handoff ─────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct HandoffOutcome { pub note: String, pub transferred: usize, pub summarized: usize }

fn norm_agent(a: Option<&str>) -> Option<String> { a.filter(|a| !a.is_empty() && *a != CARET_AGENT).map(String::from) }
fn norm_model(m: Option<&str>) -> Option<String> { m.filter(|m| !m.is_empty() && *m != "auto").map(String::from) }

pub fn agent_display_name(settings: &Settings, agent_id: Option<&str>) -> String {
    match norm_agent(agent_id) { None => "Caret".into(), Some(a) => crate::acp::registry::find_config(settings, &a).map(|c| c.name).unwrap_or(a) }
}
pub fn model_display_name(settings: &Settings, model_id: &str) -> String { settings.model(model_id).map(|m| m.display_name.clone()).unwrap_or_else(|| model_id.to_string()) }

fn note_message(note: &str, summary: Option<String>) -> ChatMessage {
    let mut m = ChatMessage::new(MessageRole::System);
    m.parts.push(MessagePart::Text { text: note.to_string() });
    if let Some(s) = summary { m.parts.push(MessagePart::Summary { text: s }); }
    m.status = Some(MessageStatus::Done);
    m
}

/// Move a thread to another agent and/or model. `resolve` turns a model id (None = default) into a provider
/// client for the summariser and for `ensure_transcript_model`. The session must be idle. Emits `SessionReset`.
pub async fn handoff_session(mgr: &AgentManager, h: &SessionHandle, agent_id: Option<&str>, model_id: Option<&str>, settings: &Settings, resolve: &(dyn Fn(Option<&str>) -> Result<ResolvedModel> + Sync)) -> Result<HandoffOutcome> {
    if h.is_running() { return Err(CaretError::invalid("Wait for the agent to finish before handing off this thread")); }
    let (session_id, cur_agent, cur_model, messages, old_summary) = { let s = h.session.lock(); (s.id.clone(), norm_agent(s.agent_id.as_deref()), s.model_id.clone(), s.messages.clone(), s.summary.clone()) };
    let target_agent = agent_id.map(|a| norm_agent(Some(a)));
    let model = norm_model(model_id);
    let budget = settings.chat.handoff_budget_tokens.max(500) as usize;
    let agent_changes = matches!(&target_agent, Some(t) if *t != cur_agent);

    let outcome = if agent_changes {
        let target = target_agent.clone().unwrap_or(None);
        let plan = plan_handoff(&messages, budget);
        // Summariser: the model the thread used (to ACP) or the one it moves to (to Caret); digest when none resolves.
        let summariser_pref = if target.is_none() { model.clone().or(cur_model.clone()) } else { cur_model.clone() };
        let summary = if plan.needs_summary() { let r = resolve(summariser_pref.as_deref()).ok(); Some(summarize_older(&plan, r.as_ref(), h.token()).await) } else { None };
        if h.is_running() { return Err(CaretError::invalid("The agent started while preparing the handoff; try again when it is idle")); }
        let name = agent_display_name(settings, target.as_deref());
        let mut note = format!("Handed off to {name}. Transferred {} message{}", plan.verbatim.len(), if plan.verbatim.len() == 1 { "" } else { "s" });
        if plan.needs_summary() { note.push_str(&format!(", {} summarised", plan.summarize.len())); }
        if target.is_none() { if let Some(m) = &model { note.push_str(&format!(" to {}", model_display_name(settings, m))); } }
        note.push('.');
        {
            let mut s = h.session.lock();
            match &target {
                Some(a) => { s.agent_id = Some(a.clone()); }
                None => {
                    s.agent_id = None;
                    s.transcript = rebuild_transcript(&plan, summary.as_deref());
                    s.transcript_model = None;
                    s.transcript_marks.clear();
                    if let Some(m) = &model { s.model_id = Some(m.clone()); }
                }
            }
            if summary.is_some() { s.summary = summary.clone(); } else if plan.needs_summary() && old_summary.is_none() { s.summary = Some(digest(&plan.summarize, summary_reserve(budget) * 4)); }
            s.messages.push(note_message(&note, summary.clone()));
            s.context_tokens = s.estimate_context_tokens();
            s.touch();
        }
        HandoffOutcome { note, transferred: plan.verbatim.len(), summarized: plan.summarize.len() }
    } else {
        // Same agent: a model change on the built-in agent (external agents pick models in their own options).
        let Some(m) = model else { return Err(CaretError::invalid("Nothing to hand off: choose another agent or model")); };
        if cur_agent.is_some() { return Err(CaretError::invalid("This thread runs on an external agent; pick its model in the agent options or hand the thread to Caret first")); }
        if cur_model.as_deref() == Some(m.as_str()) { return Err(CaretError::invalid("The thread already uses this model")); }
        let resolved = resolve(Some(&m))?;
        let note = format!("Switched to {}.", model_display_name(settings, &m));
        {
            let mut s = h.session.lock();
            s.model_id = Some(m.clone());
            s.ensure_transcript_model(&resolved.model.model);
            s.messages.push(note_message(&note, None));
            s.context_tokens = s.estimate_context_tokens();
            s.touch();
        }
        HandoffOutcome { note, transferred: messages.len(), summarized: 0 }
    };
    mgr.persist_now(h);
    mgr.emit(AgentEvent::SessionReset { session_id });
    Ok(outcome)
}

// ───────────────────────── fan-out ─────────────────────────

#[derive(Debug, Clone, Default)]
pub struct FanOutRequest {
    /// Fork this session for each model when it has messages; otherwise start fresh sessions.
    pub session_id: Option<String>,
    pub text: String,
    pub model_ids: Vec<String>,
    pub use_worktree: bool,
    pub mode: Option<String>,
    pub context: Vec<ContextItem>,
    pub images: Vec<ImageAttachment>,
    pub thinking: Option<String>,
    pub effort: Option<String>,
    /// Root for fresh sessions (no source session to fork): the calling window's folder; `None` = folderless.
    pub root: Option<std::path::PathBuf>,
}

fn first_line(text: &str, max: usize) -> String {
    let l = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("Compare");
    clip_end(l, max)
}

/// Send the same prompt to several models concurrently. Returns the new session ids (one per model, in order).
pub async fn fan_out(mgr: &Arc<AgentManager>, settings: &Settings, req: FanOutRequest) -> Result<Vec<String>> {
    let mut models: Vec<String> = vec![];
    for m in req.model_ids.iter().map(|m| m.trim()).filter(|m| !m.is_empty()) { if !models.iter().any(|x| x == m) { models.push(m.to_string()); } }
    if models.is_empty() { return Err(CaretError::invalid("Pick at least one model to compare")); }
    if req.text.trim().is_empty() && req.images.is_empty() { return Err(CaretError::invalid("Type a message to send to the selected models")); }
    let budget = settings.chat.handoff_budget_tokens.max(500) as usize;
    let source = match &req.session_id { Some(id) => Some(mgr.handle(id)?).filter(|h| !h.session.lock().messages.is_empty()), None => None };
    let root = req.root.clone();
    let (base_title, mode) = match &source {
        Some(h) => { let s = h.session.lock(); (s.title.clone(), req.mode.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| s.mode.clone())) }
        None => (first_line(&req.text, 48), req.mode.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| settings.agent.default_mode.clone())),
    };
    let group = new_id();
    let mut handles: Vec<(Arc<SessionHandle>, String)> = vec![];
    for m in &models {
        let title = format!("{base_title} · {}", model_display_name(settings, m));
        let mut s = match &source {
            Some(src) => { let src = src.session.lock(); fork_into(&src, None, budget)? }
            None => { let mut s = Session::default(); s.workspace_root = root.as_ref().map(|r| r.to_string_lossy().to_string()).unwrap_or_default(); s.cwd = s.workspace_root.clone(); s }
        };
        s.title = title.clone(); s.title_generated = true; s.mode = mode.clone(); s.model_id = Some(m.clone()); s.agent_id = None; s.fan_out_group = Some(group.clone());
        if req.use_worktree {
            // the session's own folder: a fork keeps the source's root, a fresh session has the window's
            let root = s.root().ok_or_else(CaretError::no_workspace)?;
            let (path, branch, base) = create_worktree(&root, &settings.worktrees, &title, &s.id, Some(m.as_str())).await?;
            s.worktree_path = Some(path.to_string_lossy().to_string()); s.branch = Some(branch); s.cwd = path.to_string_lossy().to_string(); s.worktree_base = base;
        }
        let h = mgr.adopt(SessionHandle::new(s));
            handles.push((h, m.clone()));
    }
    let mut ids = vec![];
    for (h, m) in handles {
        let id = h.id();
        mgr.submit(SendMessageRequest { session_id: id.clone(), text: req.text.clone(), context: req.context.clone(), images: req.images.clone(), mode: mode.clone(), model_id: Some(m), thinking: req.thinking.clone(), effort: req.effort.clone(), ..Default::default() }).await?;
        ids.push(id);
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn msg(role: MessageRole, text: &str) -> ChatMessage { let mut m = ChatMessage::new(role); m.parts.push(MessagePart::Text { text: text.into() }); m }
    fn u(t: &str) -> ChatMessage { msg(MessageRole::User, t) }
    fn a(t: &str) -> ChatMessage { msg(MessageRole::Assistant, t) }

    #[test]
    fn estimate_is_chars_over_four() { assert_eq!(estimate_tokens(""), 0); assert_eq!(estimate_tokens("abcd"), 1); assert_eq!(estimate_tokens("abcde"), 2); }

    #[test]
    fn plan_under_budget_keeps_everything_verbatim() {
        let msgs = vec![u("hello"), a("hi there"), u("fix the bug"), a("done")];
        let p = plan_handoff(&msgs, 12_000);
        assert!(!p.needs_summary()); assert_eq!(p.verbatim.len(), 4); assert_eq!(p.total(), 4);
        assert_eq!(p.verbatim[0].text, "hello"); assert_eq!(p.verbatim[3].role, MessageRole::Assistant);
        assert!(p.verbatim_tokens <= 12_000);
    }

    #[test]
    fn plan_over_budget_summarises_oldest_and_reserves_room() {
        let big = "x".repeat(400); // 100 tokens each
        let msgs: Vec<ChatMessage> = (0..10).flat_map(|i| vec![u(&format!("{big} {i}")), a(&format!("{big} r{i}"))]).collect();
        let p = plan_handoff(&msgs, 600);
        assert!(p.needs_summary());
        assert_eq!(p.total(), 20);
        assert_eq!(p.verbatim.last().unwrap().text, format!("{big} r9"), "newest message is verbatim");
        assert!(p.verbatim_tokens <= 600 - summary_reserve(600), "verbatim part leaves room for the summary");
        assert!(p.verbatim.len() >= 1 && p.verbatim.len() < 20);
        let d = digest(&p.summarize, 2000);
        assert!(d.starts_with("- User: "));
    }

    #[test]
    fn newest_message_alone_over_budget_is_still_kept() {
        let msgs = vec![u("short"), a(&"y".repeat(10_000))];
        let p = plan_handoff(&msgs, 100);
        assert_eq!(p.verbatim.len(), 1); assert_eq!(p.summarize.len(), 1);
        let block = history_block(&p, None).unwrap();
        assert!(block.contains("characters omitted"), "oversized message is clipped on render");
        assert!(block.contains("<conversation_summary"));
    }

    #[test]
    fn render_skips_system_notes_but_keeps_summaries_and_tools() {
        let mut sys = ChatMessage::new(MessageRole::System); sys.parts.push(MessagePart::Text { text: "Handed off to Codex.".into() });
        assert!(render_message(&sys).is_none());
        let mut sum = ChatMessage::new(MessageRole::System); sum.parts.push(MessagePart::Summary { text: "earlier work".into() });
        let r = render_message(&sum).unwrap(); assert!(r.is_summary); assert_eq!(r.text, "earlier work");
        let mut t = ChatMessage::new(MessageRole::Assistant);
        let mut call = ToolCallRecord::new("c1", "run_terminal_cmd", "Run"); call.command = Some("npm test".into()); call.status = ToolStatus::Done;
        t.parts.push(MessagePart::Tool { call }); t.parts.push(MessagePart::Text { text: "Tests pass.".into() });
        assert_eq!(render_message(&t).unwrap().text, "[tool Run: npm test]\nTests pass.");
        assert!(render_message(&ChatMessage::new(MessageRole::Assistant)).is_none());
    }

    #[test]
    fn rebuilt_transcript_alternates_and_carries_summary_pair() {
        let msgs = vec![u("one"), u("two"), a("three"), a("four")];
        let t = rebuild_transcript(&plan_handoff(&msgs, 12_000), None);
        assert_eq!(t.len(), 2); assert_eq!(t[0].role, Role::User); assert_eq!(t[0].content.len(), 2); assert_eq!(t[1].role, Role::Assistant);
        let big = "z".repeat(4000);
        let msgs: Vec<ChatMessage> = (0..6).flat_map(|i| vec![u(&format!("{big}{i}")), a("ok")]).collect();
        let p = plan_handoff(&msgs, 1200);
        let t = rebuild_transcript(&p, Some("the gist"));
        assert!(t[0].text().contains("<conversation_summary>") && t[0].text().contains("the gist"));
        // Only the newest "ok" fits next to the reserve: it merges into the acknowledgement so roles keep alternating.
        assert_eq!(t.len(), 2); assert_eq!(t[1].role, Role::Assistant);
        assert_eq!(t[1].content.len(), 2); assert!(t.last().unwrap().text().ends_with("ok"));
    }

    #[test]
    fn history_block_is_none_for_empty_threads() { assert!(history_block(&plan_handoff(&[], 100), None).is_none()); }

    #[test]
    fn fork_into_without_cut_copies_and_resets_bookkeeping() {
        let mut src = Session::default(); src.title = "Auth bug".into(); src.pinned = true; src.worktree_path = Some("/wt".into());
        src.messages.push({ let mut m = u("hi"); m.checkpoint_id = Some("t1".into()); m });
        src.transcript.push(LlmMessage::user_text("hi"));
        src.usage.input_tokens = 99;
        let f = fork_into(&src, None, 12_000).unwrap();
        assert_ne!(f.id, src.id); assert_eq!(f.title, "Fork: Auth bug"); assert_eq!(f.parent_session_id.as_deref(), Some(src.id.as_str()));
        assert!(!f.pinned && !f.is_subagent && f.worktree_path.is_none() && f.fork_of_message_id.is_none());
        assert_eq!(f.messages.len(), 1); assert!(f.messages[0].checkpoint_id.is_none()); assert_eq!(f.transcript.len(), 1); assert_eq!(f.usage.input_tokens, 0);
        assert!(fork_into(&src, Some("nope"), 12_000).is_err());
    }
}

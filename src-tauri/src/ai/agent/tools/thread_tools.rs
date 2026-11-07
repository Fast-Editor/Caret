//! `read_thread`: page through the earlier messages of the current thread (the UI history), including the ones
//! that compaction or a provider handoff summarised away from the model's context.
use super::{arg_u64, cap_output, s_int, schema, Tool, ToolContext, ToolOutput};
use crate::ai::agent::handoff::{clip_middle, render_message};
use crate::ai::agent::policy::ToolEffect;
use crate::ai::agent::session::MessageRole;
use crate::error::Result;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn tools() -> Vec<Arc<dyn Tool>> { vec![Arc::new(ReadThread)] }

const DEFAULT_LIMIT: u64 = 20;
const MAX_LIMIT: u64 = 50;
const MAX_MESSAGE_CHARS: usize = 4000;

pub struct ReadThread;
#[async_trait::async_trait]
impl Tool for ReadThread {
    fn name(&self) -> &str { "read_thread" }
    fn display_name(&self) -> &str { "Read thread" }
    fn description(&self) -> String { "Read earlier messages of this conversation verbatim (oldest first), including messages that were summarised away from your context by compaction or when the thread was handed over from another agent or model. Use it when the user refers to something the summary lacks. `offset` is the 0-based index of the first message to return, `limit` the number of messages (default 20, max 50). The reply states the total so you can page.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "offset": s_int("0-based index of the first message (oldest = 0)"), "limit": s_int("Number of messages to return (default 20, max 50)") }), &[]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "context" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::ReadOnly }
    fn label(&self, i: &Value) -> Option<String> { let o = arg_u64(i, &["offset"]).unwrap_or(0); let l = arg_u64(i, &["limit"]).unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT); Some(format!("messages {o}-{}", o + l)) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let offset = arg_u64(&input, &["offset"]).unwrap_or(0) as usize;
        let limit = arg_u64(&input, &["limit"]).unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as usize;
        let h = ctx.manager.handle(&ctx.session_id)?;
        let (title, entries) = {
            let s = h.session.lock();
            // The turn in flight is already in the model's context: only earlier turns are "the thread".
            let current = s.current_turn.clone();
            let entries: Vec<(MessageRole, Option<String>, String)> = s.messages.iter()
                .filter(|m| current.is_none() || m.turn_id != current)
                .filter_map(|m| render_message(m).map(|r| (r.role, m.model_id.clone(), if r.is_summary { format!("(context summary)\n{}", r.text) } else { r.text })))
                .collect();
            (s.title.clone(), entries)
        };
        let total = entries.len();
        if total == 0 { return Ok(ToolOutput::text("This thread has no earlier messages.")); }
        if offset >= total { return Ok(ToolOutput::text(format!("No messages at offset {offset}: the thread has {total} earlier message(s) (indices 0-{}).", total - 1))); }
        let end = (offset + limit).min(total);
        let mut out = format!("Thread \"{title}\" has {total} earlier message(s). Showing {offset}-{} (oldest first).{}\n", end - 1, if end < total { format!(" Call again with offset={end} for more.") } else { String::new() });
        for (i, (role, model, text)) in entries.iter().enumerate().skip(offset).take(end - offset) {
            let who = match role { MessageRole::User => "user".to_string(), MessageRole::Assistant => match model { Some(m) => format!("assistant ({m})"), None => "assistant".into() }, MessageRole::System => "system".into() };
            out.push_str(&format!("\n#{i} {who}:\n{}\n", clip_middle(text, MAX_MESSAGE_CHARS)));
        }
        let (text, _) = cap_output(&out, ctx.max_output());
        Ok(ToolOutput::text(text).with_summary(format!("{}-{} of {total}", offset, end - 1)))
    }
}

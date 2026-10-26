//! Transcript compaction: summarize older turns with the model and keep the recent tail verbatim.
use crate::ai::llm::{self, ContentBlock, LlmRequest, Message, ResolvedModel, Role};
use crate::error::Result;
use tokio_util::sync::CancellationToken;

const KEEP_TAIL: usize = 6;

/// Index where the kept tail starts: the latest user message with plain text (not tool results)
/// such that at least `min_old` messages precede it.
pub fn split_point(transcript: &[Message], min_old: usize) -> Option<usize> {
    let mut candidates: Vec<usize> = transcript.iter().enumerate().filter(|(_, m)| m.role == Role::User && m.content.iter().any(|b| matches!(b, ContentBlock::Text { .. })) && !m.content.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. }))).map(|(i, _)| i).collect();
    candidates.retain(|&i| i >= min_old);
    // prefer the candidate that keeps ≤ KEEP_TAIL messages, else the earliest eligible
    let n = transcript.len();
    candidates.iter().rev().find(|&&i| n - i <= KEEP_TAIL).copied().or_else(|| candidates.first().copied())
}

pub fn render(messages: &[Message], max_chars: usize) -> String {
    let mut out = String::new();
    for m in messages {
        let who = match m.role { Role::User => "USER", Role::Assistant => "ASSISTANT" };
        for b in &m.content {
            match b {
                ContentBlock::Text { text } => out.push_str(&format!("{who}: {text}\n")),
                ContentBlock::ToolUse { name, input, .. } => out.push_str(&format!("{who} called {name}({})\n", input.to_string().chars().take(300).collect::<String>())),
                ContentBlock::ToolResult { content, is_error, .. } => { let t: String = content.iter().filter_map(|c| if let llm::ToolResultContent::Text { text } = c { Some(text.as_str()) } else { None }).collect::<Vec<_>>().join("\n"); out.push_str(&format!("TOOL RESULT{}: {}\n", if *is_error { " (error)" } else { "" }, t.chars().take(1200).collect::<String>())); }
                ContentBlock::Image { .. } => out.push_str(&format!("{who}: [image]\n")),
                _ => {}
            }
        }
    }
    if out.len() > max_chars { let (t, _) = crate::tokens::truncate_middle(&out, max_chars / 4); t } else { out }
}

pub const SUMMARY_PROMPT: &str = "You are compacting an AI coding session. Write a dense handoff summary of the conversation so far so the assistant can continue seamlessly. Include: the user's goals and constraints; decisions made and why; files inspected/changed (paths) and what changed; commands run and results; open problems, errors seen and next steps; any facts about the codebase learned. Use headed bullet lists. Do not include pleasantries. Max ~700 words.";

/// Replace older transcript messages with a summary. Returns (summary text, new transcript) or None when too short.
pub async fn compact(resolved: &ResolvedModel, transcript: &[Message], cancel: CancellationToken) -> Result<Option<(String, Vec<Message>)>> {
    let Some(split) = split_point(transcript, 2) else { return Ok(None) };
    if split == 0 { return Ok(None); }
    let older = &transcript[..split];
    let rendered = render(older, 200_000);
    let req = LlmRequest::new(resolved.model.clone()).system(SUMMARY_PROMPT).user(format!("<conversation>\n{rendered}\n</conversation>\n\nWrite the handoff summary.")).no_thinking().max_tokens(2000).purpose("summarize");
    let (summary, _) = llm::complete_text(resolved, req, cancel).await?;
    let mut new_t = vec![Message::user_text(format!("<conversation_summary>\nThis is a summary of the earlier part of this conversation (older messages were compacted):\n\n{summary}\n</conversation_summary>")), Message::assistant_text("Understood. I have the summary of our earlier work and will continue from here.")];
    new_t.extend_from_slice(&transcript[split..]);
    Ok(Some((summary, new_t)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn u(t: &str) -> Message { Message::user_text(t) }
    fn a(t: &str) -> Message { Message::assistant_text(t) }
    #[test]
    fn split_keeps_recent_user_turn() {
        let t = vec![u("1"), a("r1"), u("2"), a("r2"), u("3"), a("r3"), u("4"), a("r4")];
        let s = split_point(&t, 2).unwrap();
        assert_eq!(t[s].role, Role::User);
        assert!(t.len() - s <= KEEP_TAIL);
        assert!(s >= 2);
    }
    #[test]
    fn split_none_when_short() { assert!(split_point(&[u("1"), a("r")], 2).is_none()); }
    #[test]
    fn render_truncates() { let big = vec![u(&"x".repeat(10_000)); 30]; assert!(render(&big, 20_000).len() < 40_000); }
}

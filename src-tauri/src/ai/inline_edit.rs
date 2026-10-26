//! Cmd+K: inline edit / generate / quick question (streaming), plus terminal command generation.
use crate::ai::cancel;
use crate::ai::llm::{self, LlmEvent, LlmRequest, Message, SystemBlock, UsageDelta};
use crate::ai::util::{extract_json, pick_model, read_text_capped, strip_fences, workspace_root};
use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineHistoryItem { pub instruction: String, pub result: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineEditRequest {
    pub request_id: String,
    pub path: String,
    pub language: String,
    pub full_text: String,
    pub start_line: u32,
    pub end_line: u32,
    pub selected_text: String,
    pub instruction: String,
    /// ContextItem[] (opaque here; `path`/`range`/`data`/`label`/`kind` are read leniently).
    #[serde(default)] pub context: Vec<Value>,
    pub mode: String,
    #[serde(default)] pub model_id: Option<String>,
    #[serde(default)] pub history: Option<Vec<InlineHistoryItem>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum InlineEditEvent {
    Delta { text: String },
    Done { text: String, #[serde(skip_serializing_if = "Option::is_none")] usage: Option<UsagePublic> },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePublic { pub input_tokens: u64, pub output_tokens: u64, pub cache_read_tokens: u64, pub cache_write_tokens: u64, #[serde(skip_serializing_if = "Option::is_none")] pub cost_usd: Option<f64> }
impl UsagePublic { pub fn from(u: &UsageDelta, m: &crate::core::settings::ModelConfig) -> Self { Self { input_tokens: u.input_tokens, output_tokens: u.output_tokens, cache_read_tokens: u.cache_read_tokens, cache_write_tokens: u.cache_write_tokens, cost_usd: Some(u.cost_usd(m)) } } }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCommandRequest { pub request_id: String, pub instruction: String, pub shell: String, pub cwd: String, #[serde(default)] pub recent_output: Option<String>, pub os: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCommandResult { pub command: String, #[serde(skip_serializing_if = "Option::is_none")] pub explanation: Option<String> }

const EDIT_SYSTEM: &str = "You are an expert programmer performing a precise inline edit inside a code editor.\nYou will receive the whole file (for context), the exact selected range to rewrite, and an instruction.\nOutput ONLY the replacement code for the selected range, wrapped exactly as <code>…</code>.\nRules: no explanations, no markdown fences, no line numbers; preserve the file's indentation style and the indentation level of the selection; keep everything outside the selection consistent (do not re-emit surrounding code); if the instruction asks for a deletion, output <code></code>; when generating new code at an empty selection, output just the new code.";
const QUESTION_SYSTEM: &str = "You are an expert programmer answering a quick question about a selected region of code inside an editor. Answer concisely in Markdown, referencing the selection and nearby code; include short code snippets only when they add clarity.";
const TERMINAL_SYSTEM: &str = "You translate natural-language requests into a single shell command for the user's OS and shell. Respond with JSON only: {\"command\": string, \"explanation\": string}. Prefer safe, idempotent commands; use the cwd as given; never chain destructive operations. If the request is inherently destructive or unclear, set command to \"\" and explain.";

/// Incremental `<code>…</code>` extractor for streamed output.
#[derive(Default)]
pub struct TagStripper { started: bool, ended: bool, pending: String, out_len: usize, saw_open: bool }
impl TagStripper {
    /// Push a delta; returns text safe to emit now.
    pub fn push(&mut self, delta: &str) -> String {
        if self.ended { return String::new(); }
        self.pending.push_str(delta);
        if !self.started {
            // Drop router/proxy banner lines (e.g. `*[Lynkr] … → …*`) that some gateways prepend.
            if self.pending.contains('\n') { let cleaned = crate::ai::tab::strip_banner_lines(&self.pending); if cleaned.len() != self.pending.len() { self.pending = cleaned; } }
            if let Some(i) = self.pending.find("<code>") { self.pending = self.pending[i + 6..].to_string(); self.started = true; self.saw_open = true; if self.pending.starts_with('\n') { self.pending.remove(0); } }
            else if self.pending.len() > 64 && !self.pending.contains('<') && !self.pending.trim_start().starts_with("```") && !self.pending.trim_start().starts_with('*') && !self.pending.trim_start().starts_with('[') { self.started = true; } // model skipped tags
            else { return String::new(); }
        }
        if let Some(i) = self.pending.find("</code>") { self.ended = true; let out = self.pending[..i].to_string(); self.pending.clear(); self.out_len += out.len(); return out; }
        // Hold back a tail that could be the start of "</code>".
        let hold = (0..7.min(self.pending.len())).rev().find(|&k| "</code>".starts_with(&self.pending[self.pending.len() - k..])).unwrap_or(0);
        let emit_to = self.pending.len() - hold;
        let out = self.pending[..emit_to].to_string();
        self.pending = self.pending[emit_to..].to_string();
        self.out_len += out.len();
        out
    }
    /// Flush at end of stream.
    pub fn finish(&mut self) -> String {
        if self.ended { return String::new(); }
        let mut rest = std::mem::take(&mut self.pending);
        if !self.started { rest = crate::ai::tab::strip_banner_lines(&rest); rest = strip_fences(&rest); if let Some(i) = rest.find("<code>") { rest = rest[i + 6..].to_string(); if rest.starts_with('\n') { rest.remove(0); } } }
        if let Some(i) = rest.find("</code>") { rest.truncate(i); }
        self.ended = true;
        rest
    }
}

fn context_text(root: Option<&std::path::Path>, items: &[Value]) -> String {
    let mut s = String::new();
    for it in items.iter().take(12) {
        let kind = it["kind"].as_str().unwrap_or("");
        let label = it["label"].as_str().unwrap_or("context");
        if let Some(data) = it["data"].as_str() { s.push_str(&format!("<context kind=\"{kind}\" label=\"{label}\">\n{}\n</context>\n", &data[..data.len().min(6000)])); continue; }
        if let Some(p) = it["path"].as_str() {
            let abs = crate::paths::resolve(root, p);
            if abs.is_file() { if let Ok((text, _)) = read_text_capped(&abs, 48 * 1024) {
                let body = match (it["range"]["startLine"].as_u64(), it["range"]["endLine"].as_u64()) {
                    (Some(a), Some(b)) => text.lines().skip(a.saturating_sub(1) as usize).take((b.saturating_sub(a) + 1) as usize).collect::<Vec<_>>().join("\n"),
                    _ => text,
                };
                s.push_str(&format!("<context kind=\"{kind}\" path=\"{p}\">\n{}\n</context>\n", &body[..body.len().min(8000)]));
            } }
        }
    }
    s
}

fn windowed_file(full: &str, start: u32, end: u32, max_lines: usize) -> (String, usize) {
    let lines: Vec<&str> = full.lines().collect();
    let total = lines.len();
    if total <= max_lines { return (full.to_string(), 1); }
    let sel_len = (end.saturating_sub(start) + 1) as usize;
    let room = max_lines.saturating_sub(sel_len) / 2;
    let s = (start as usize).saturating_sub(1).saturating_sub(room);
    let e = ((end as usize) + room).min(total);
    (lines[s..e].join("\n"), s + 1)
}

pub async fn inline_edit(app: &AppHandle, req: InlineEditRequest, ch: Channel<InlineEditEvent>) -> Result<()> {
    let settings_state = app.state::<SettingsState>();
    let settings = settings_state.effective();
    let resolved = pick_model(&settings, &settings_state, req.model_id.as_deref(), false)?;
    let root = workspace_root(app);
    let is_question = req.mode == "question";
    let (window, first_line) = windowed_file(&req.full_text, req.start_line, req.end_line, 1500);
    let mut numbered = String::new();
    for (i, l) in window.lines().enumerate() { let n = first_line + i; let marker = if n as u32 >= req.start_line && n as u32 <= req.end_line { ">" } else { " " }; numbered.push_str(&format!("{marker}{n:>5}| {l}\n")); }
    let ctx = context_text(root.as_deref(), &req.context);
    let mut user = format!("Language: {}\nFile: {}\nSelected range: lines {}-{} (marked with '>')\n\n<file>\n{}</file>\n\n<selection>\n{}\n</selection>\n", req.language, req.path, req.start_line, req.end_line, numbered, req.selected_text);
    if !ctx.is_empty() { user.push_str("\nAdditional context:\n"); user.push_str(&ctx); }
    if let Some(h) = &req.history { for item in h.iter().rev().take(3).rev() { user.push_str(&format!("\nPrevious instruction: {}\nPrevious result:\n{}\n", item.instruction, &item.result[..item.result.len().min(3000)])); } }
    user.push_str(&format!("\nInstruction: {}\n", req.instruction));
    if !is_question { user.push_str("\nReturn the replacement for the selected range inside <code></code>."); }

    let mut llm_req = LlmRequest::new(resolved.model.clone()).purpose(if is_question { "quick-question" } else { "inline-edit" });
    llm_req.system = vec![SystemBlock { text: if is_question { QUESTION_SYSTEM.into() } else { EDIT_SYSTEM.into() }, cache: true }];
    llm_req.messages = vec![Message::user_text(user)];
    llm_req.max_tokens = llm_req.max_tokens.min(16_000);
    llm_req.thinking.enabled = settings.agent.thinking != "off" && !is_question && req.end_line.saturating_sub(req.start_line) > 20;
    llm_req.thinking.effort = crate::ai::llm::Effort::Low;
    if !llm_req.thinking.enabled { llm_req.temperature = Some(0.2); }

    let guard = cancel::Guard::new(&req.request_id);
    let provider = llm::provider_for(&resolved)?;
    let mut stripper = TagStripper::default();
    let mut emitted = String::new();
    let ch2 = ch.clone();
    let result = llm::run_stream(provider, &llm_req, guard.token(), |ev| {
        if let LlmEvent::TextDelta { text, .. } = ev {
            let out = if is_question { text.clone() } else { stripper.push(text) };
            if !out.is_empty() { emitted.push_str(&out); let _ = ch2.send(InlineEditEvent::Delta { text: out }); }
        }
    }).await;
    match result {
        Ok(turn) => {
            if turn.stop == Some(crate::ai::llm::StopReason::Refusal) { let m = format!("The model declined this request{}", turn.stop_detail.map(|d| format!(": {d}")).unwrap_or_default()); let _ = ch.send(InlineEditEvent::Error { message: m.clone() }); return Err(CaretError::provider(m)); }
            if !is_question { let rest = stripper.finish(); if !rest.is_empty() { emitted.push_str(&rest); let _ = ch.send(InlineEditEvent::Delta { text: rest }); } }
            let text = if is_question { emitted } else { emitted.trim_end_matches('\n').to_string() };
            tracing::debug!(model = %resolved.model.id, input = turn.usage.input_tokens, output = turn.usage.output_tokens, "inline edit done");
            let _ = ch.send(InlineEditEvent::Done { text, usage: Some(UsagePublic::from(&turn.usage, &resolved.model)) });
            Ok(())
        }
        Err(e) => { if e.code != "cancelled" { let _ = ch.send(InlineEditEvent::Error { message: e.message.clone() }); } Err(e) }
    }
}

pub fn is_destructive(cmd: &str) -> bool {
    let c = cmd.to_ascii_lowercase();
    let pats = ["rm -rf /", "rm -rf ~", "rm -rf *", "rm -rf .", "mkfs", "dd if=", ":(){", "shutdown", "reboot", "> /dev/sd", "chmod -r 777 /", "chown -r", "git push --force", "git push -f", "git reset --hard", "git clean -fd", "format c:", "del /s /q", "diskutil erase", "killall -9", "sudo rm", "curl | sh", "curl | bash", "wget | sh", "| sudo"];
    pats.iter().any(|p| c.contains(p))
}

pub async fn terminal_command(app: &AppHandle, req: TerminalCommandRequest) -> Result<TerminalCommandResult> {
    let settings_state = app.state::<SettingsState>();
    let settings = settings_state.effective();
    let resolved = pick_model(&settings, &settings_state, None, true)?;
    let mut user = format!("OS: {}\nShell: {}\ncwd: {}\n", req.os, req.shell, req.cwd);
    if let Some(o) = &req.recent_output { if !o.trim().is_empty() { let tail: String = o.chars().rev().take(3000).collect::<Vec<_>>().into_iter().rev().collect(); user.push_str(&format!("\nRecent terminal output:\n{tail}\n")); } }
    user.push_str(&format!("\nRequest: {}", req.instruction));
    let mut llm_req = LlmRequest::new(resolved.model.clone()).no_thinking().max_tokens(400).purpose("terminal-cmdk");
    llm_req.system = vec![SystemBlock { text: TERMINAL_SYSTEM.into(), cache: true }];
    llm_req.messages = vec![Message::user_text(user)];
    llm_req.temperature = Some(0.0);
    let guard = cancel::Guard::new(&req.request_id);
    let (raw, _) = llm::complete_text(&resolved, llm_req, guard.token()).await?;
    let v = extract_json(&raw).unwrap_or_else(|| serde_json::json!({"command": strip_fences(&raw).lines().next().unwrap_or("").trim(), "explanation": null}));
    let command = v["command"].as_str().unwrap_or("").trim().to_string();
    let explanation = v["explanation"].as_str().map(String::from);
    if is_destructive(&command) {
        return Ok(TerminalCommandResult { command: String::new(), explanation: Some(format!("Refusing to auto-generate a destructive command ({command}). {}", explanation.unwrap_or_default())) });
    }
    Ok(TerminalCommandResult { command, explanation })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tag_stripper_streams_only_inner_code() {
        let mut s = TagStripper::default();
        let mut out = String::new();
        for d in ["Here you go: <co", "de>\nfn a() {", "\n  1\n}</co", "de>\nthanks"] { out.push_str(&s.push(d)); }
        out.push_str(&s.finish());
        assert_eq!(out, "fn a() {\n  1\n}");
    }
    #[test]
    fn tag_stripper_without_tags_and_with_fences() {
        let mut s = TagStripper::default();
        let mut out = s.push("```rust\nlet x = 1;\n```");
        out.push_str(&s.finish());
        assert_eq!(out, "let x = 1;");
        let mut s = TagStripper::default();
        let long = "x".repeat(80);
        let out = s.push(&long);
        assert_eq!(out.len(), 80 - 0.max(0));
    }
    #[test]
    fn destructive_detection() {
        assert!(is_destructive("sudo rm -rf /tmp/x"));
        assert!(is_destructive("git push --force origin main"));
        assert!(!is_destructive("ls -la"));
        assert!(!is_destructive("cargo test"));
    }
    #[test]
    fn windowing_keeps_selection() {
        let text = (1..=3000).map(|i| format!("l{i}")).collect::<Vec<_>>().join("\n");
        let (w, first) = windowed_file(&text, 2000, 2010, 1500);
        assert!(w.contains("l2000") && w.contains("l2010"));
        assert!(first > 1 && w.lines().count() <= 1500);
    }
}

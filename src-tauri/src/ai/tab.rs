//! Tab completions: fill-in-the-middle via chat prompt, LRU cache, in-flight dedupe, next-edit prediction.
use crate::ai::cancel;
use crate::ai::llm::{self, LlmRequest};
use crate::ai::util::{extract_json, pick_model, strip_fences};
use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use lru::LruCache;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::time::Instant;
use crate::ai::llm::ResolvedModel;
use crate::core::settings::Settings;
use tauri::{AppHandle, Manager};
use tokio::sync::watch;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentEdit { pub path: String, pub before: String, pub after: String, pub line: u32, pub at: u64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenFileSnippet { pub path: String, pub snippet: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionRequest {
    pub request_id: String,
    pub path: String,
    pub language: String,
    pub prefix: String,
    pub suffix: String,
    pub cursor_line: u32,
    pub cursor_column: u32,
    #[serde(default)] pub recent_edits: Option<Vec<RecentEdit>>,
    #[serde(default)] pub diagnostics: Option<Vec<String>>,
    #[serde(default)] pub open_files: Option<Vec<OpenFileSnippet>>,
    #[serde(default)] pub multiline: bool,
    #[serde(default)] pub manual: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionResult { pub request_id: String, pub text: String, #[serde(skip_serializing_if = "Option::is_none")] pub model_id: Option<String>, pub latency_ms: u64, pub cached: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NextEditRequest { pub request_id: String, pub path: String, pub language: String, pub content: String, pub cursor_line: u32, #[serde(default)] pub recent_edits: Vec<RecentEdit> }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NextEditResult {
    pub request_id: String, pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub start_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")] pub end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")] pub new_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub reason: Option<String>,
}

pub struct TabState {
    cache: Mutex<LruCache<String, String>>,
    inflight: Mutex<HashMap<String, watch::Receiver<Option<Option<String>>>>>,
    /// provider id → until when it is treated as unreachable (after a failed Tab request), so keystrokes don't keep
    /// paying for a dead local proxy and fall straight through to the default model instead.
    down: Mutex<HashMap<String, Instant>>,
}
impl Default for TabState {
    fn default() -> Self { Self { cache: Mutex::new(LruCache::new(NonZeroUsize::new(200).unwrap())), inflight: Mutex::new(HashMap::new()), down: Mutex::new(HashMap::new()) } }
}

const PROVIDER_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(45);

/// The fast (Tab) model plus a fallback (the default model, when different). While the fast model's provider is in
/// cooldown the fallback is returned directly.
fn pick_for_tab(st: &TabState, settings: &Settings, secrets: &SettingsState) -> Result<(ResolvedModel, Option<ResolvedModel>)> {
    let fast = pick_model(settings, secrets, None, true)?;
    // Fallback = the default model when it lives on another provider, else the first enabled model on any other usable
    // provider (a dead local proxy must not take every model with it).
    let other_provider = |m: &ResolvedModel| m.model.id != fast.model.id && m.provider.id != fast.provider.id;
    let fallback = llm::resolve_model(settings, secrets, None).ok().filter(|d| other_provider(d)).or_else(|| {
        settings.models.iter().filter(|m| m.enabled && m.provider_id != fast.provider.id)
            .filter_map(|m| llm::resolve_model(settings, secrets, Some(&m.id)).ok())
            .find(|r| crate::ai::util::usable(&r.provider, secrets))
    });
    let down = st.down.lock().get(&fast.provider.id).map(|t| *t > Instant::now()).unwrap_or(false);
    if down { if let Some(d) = fallback { return Ok((d, None)); } }
    Ok((fast, fallback))
}
fn mark_provider_down(st: &TabState, provider_id: &str) { st.down.lock().insert(provider_id.to_string(), Instant::now() + PROVIDER_COOLDOWN); }

/// Run a single-shot request, retrying once on the fallback model when the primary's provider fails (not on cancel).
async fn complete_with_fallback(st: &TabState, primary: ResolvedModel, fallback: Option<ResolvedModel>, req: LlmRequest, cancel: tokio_util::sync::CancellationToken, purpose: &str) -> Result<(String, llm::UsageDelta, ResolvedModel)> {
    match llm::complete_text(&primary, req.clone(), cancel.clone()).await {
        Ok((raw, usage)) => Ok((raw, usage, primary)),
        Err(e) if e.code != "cancelled" && fallback.is_some() => {
            tracing::warn!(purpose, provider = %primary.provider.id, model = %primary.model.id, error = %e.message, "fast model failed; falling back to the default model");
            mark_provider_down(st, &primary.provider.id);
            let fb = fallback.unwrap();
            let mut req2 = req; req2.model = fb.model.clone();
            let (raw, usage) = llm::complete_text(&fb, req2, cancel).await?;
            Ok((raw, usage, fb))
        }
        Err(e) => Err(e),
    }
}

const SYSTEM: &str = "You are a code completion engine inside a code editor. You receive the text before the cursor (<prefix>) and after the cursor (<suffix>) and must produce ONLY the text to insert at the cursor.\n\nRules:\n- Output the insertion wrapped exactly as <completion>…</completion>. No explanations, no markdown fences.\n- Continue precisely from the cursor position: never repeat text already in the prefix, never duplicate text that already appears at the start of the suffix.\n- Match the file's language, indentation, naming and style; use surrounding code, recent edits and diagnostics as hints for intent.\n- Stop at a natural boundary (end of statement, block, or line). Produce at most {MAX_LINES} line(s).\n- If no sensible completion exists, output <completion></completion>.";

fn tail_chars(s: &str, n: usize) -> &str { let cut = s.char_indices().rev().nth(n.saturating_sub(1)).map(|(i, _)| i).unwrap_or(0); &s[cut..] }
fn head_chars(s: &str, n: usize) -> &str { let cut = s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len()); &s[..cut] }
fn last_lines(s: &str, n: usize) -> String { let lines: Vec<&str> = s.lines().collect(); let start = lines.len().saturating_sub(n); lines[start..].join("\n") + if s.ends_with('\n') { "\n" } else { "" } }
fn first_lines(s: &str, n: usize) -> String { s.lines().take(n).collect::<Vec<_>>().join("\n") }

pub fn cache_key(req: &CompletionRequest) -> String {
    let mut h = blake3::Hasher::new();
    h.update(req.path.as_bytes()); h.update(b"\0");
    h.update(tail_chars(&req.prefix, 2048).as_bytes()); h.update(b"\0");
    h.update(head_chars(&req.suffix, 512).as_bytes()); h.update(b"\0");
    h.update(&[req.multiline as u8]);
    h.finalize().to_hex().to_string()
}

/// Clean raw model output into an insertable completion. None when unusable.
/// Drop leading "router/proxy banner" lines such as `*[Lynkr] SIMPLE → model*` and empty lines before real content.
pub fn strip_banner_lines(raw: &str) -> String {
    let mut lines = raw.lines().peekable();
    let mut skipped = false;
    while let Some(l) = lines.peek() {
        let t = l.trim().trim_matches('*').trim();
        if t.is_empty() && skipped { lines.next(); continue; }
        if t.starts_with('[') && t.contains(']') && (t.contains('→') || t.contains("->") || t.to_ascii_lowercase().contains("model")) { lines.next(); skipped = true; continue; }
        break;
    }
    lines.collect::<Vec<_>>().join("\n")
}

/// Remove the longest tail of `prefix` that the completion re-emits at its start (models often restate the current function).
pub fn strip_prefix_overlap(prefix: &str, text: &str) -> String {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let p_lines: Vec<&str> = prefix.lines().collect();
    // try from the longest candidate (up to 40 lines) down to 1 line
    let max = p_lines.len().min(40);
    for k in (1..=max).rev() {
        let tail = p_lines[p_lines.len() - k..].join("\n");
        let tail_n = norm(&tail);
        if tail_n.is_empty() { continue; }
        let text_n = norm(text);
        if text_n.starts_with(&tail_n) {
            // consume the same number of non-whitespace chars from text
            let target: usize = tail_n.chars().filter(|c| !c.is_whitespace()).count();
            let mut seen = 0usize; let mut cut = 0usize;
            for (i, c) in text.char_indices() { if !c.is_whitespace() { seen += 1; } if seen >= target { cut = i + c.len_utf8(); break; } }
            let mut rest = text[cut..].trim_start_matches(|c: char| c == ' ' || c == '\t');
            rest = rest.trim_start_matches('\n');
            // The cursor line's existing indentation is already typed — do not emit it again.
            let cur = prefix.rsplit('\n').next().unwrap_or("");
            if !cur.is_empty() && cur.trim().is_empty() { if let Some(r) = rest.strip_prefix(cur) { rest = r; } }
            return rest.to_string();
        }
    }
    text.to_string()
}

pub fn clean_completion(raw: &str, prefix: &str, suffix: &str, max_lines: usize) -> Option<String> {
    let mut text = strip_banner_lines(raw);
    if let Some(i) = text.find("<completion>") { text = text[i + "<completion>".len()..].to_string(); }
    if let Some(i) = text.find("</completion>") { text.truncate(i); }
    let text = if text.trim_start().starts_with("```") { strip_fences(&text) } else { text };
    let mut text = text.trim_end_matches('\n').to_string();
    if text.trim().is_empty() { return None; }
    text = strip_prefix_overlap(prefix, &text);
    if text.trim().is_empty() { return None; }
    // Drop a leading newline when the cursor is already at the start of an empty line.
    let cursor_line = prefix.rsplit('\n').next().unwrap_or("");
    if cursor_line.trim().is_empty() && text.starts_with('\n') { text = text.trim_start_matches('\n').to_string(); }
    // Never repeat the current line's prefix.
    if !cursor_line.is_empty() && text.starts_with(cursor_line.trim_start()) && !cursor_line.trim().is_empty() { text = text[cursor_line.trim_start().len()..].to_string(); }
    // Trim overlap with the suffix start (e.g. the model re-emits a closing bracket that already follows).
    let suffix_head = suffix.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim_end();
    if !suffix_head.is_empty() {
        let st = suffix_head.trim_start();
        if text.trim() == st { return None; }
        if let Some(stripped) = text.trim_end().strip_suffix(st) { text = stripped.trim_end().to_string(); }
    }
    // Cap lines.
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.len() > max_lines { text = lines[..max_lines].join("\n"); }
    if text.trim().is_empty() { None } else { Some(text) }
}

fn build_user_prompt(req: &CompletionRequest, context_lines: usize) -> String {
    let mut s = String::with_capacity(8192);
    s.push_str(&format!("Language: {}\nFile: {}\n", req.language, req.path));
    if let Some(edits) = &req.recent_edits { if !edits.is_empty() {
        s.push_str("\nRecent edits (most recent last):\n");
        for e in edits.iter().rev().take(5).rev() { s.push_str(&format!("--- {}:{}\n- {}\n+ {}\n", e.path, e.line, head_chars(&e.before, 300), head_chars(&e.after, 300))); }
    } }
    if let Some(d) = &req.diagnostics { if !d.is_empty() { s.push_str("\nDiagnostics near the cursor:\n"); for l in d.iter().take(6) { s.push_str("- "); s.push_str(head_chars(l, 200)); s.push('\n'); } } }
    if let Some(files) = &req.open_files { for f in files.iter().take(3) { s.push_str(&format!("\n<related_file path=\"{}\">\n{}\n</related_file>\n", f.path, head_chars(&f.snippet, 1500))); } }
    let prefix = last_lines(&req.prefix, context_lines);
    let suffix = first_lines(&req.suffix, context_lines / 2);
    s.push_str(&format!("\n<prefix>{}</prefix><suffix>{}</suffix>", head_chars(tail_chars(&prefix, 6000), 6000), head_chars(&suffix, 2500)));
    s
}

pub async fn complete(app: &AppHandle, req: CompletionRequest) -> Result<CompletionResult> {
    let started = std::time::Instant::now();
    let st = app.state::<TabState>();
    let settings_state = app.state::<SettingsState>();
    let settings = settings_state.effective();
    if !settings.tab.enabled && req.manual != Some(true) { return Err(CaretError::new("disabled", "Tab completions are disabled")); }
    if settings.tab.disabled_languages.iter().any(|l| l == &req.language) && req.manual != Some(true) { return Ok(CompletionResult { request_id: req.request_id, text: String::new(), model_id: None, latency_ms: 0, cached: false }); }
    let key = cache_key(&req);
    { let mut c = st.cache.lock(); if c.cap().get() != settings.tab.cache_size.max(16) as usize { c.resize(NonZeroUsize::new(settings.tab.cache_size.max(16) as usize).unwrap()); }
      if let Some(hit) = c.get(&key) { return Ok(CompletionResult { request_id: req.request_id, text: hit.clone(), model_id: None, latency_ms: started.elapsed().as_millis() as u64, cached: true }); } }

    // In-flight dedupe: wait for an identical request already running.
    let existing = st.inflight.lock().get(&key).cloned();
    if let Some(mut rx) = existing {
        let _ = tokio::time::timeout(std::time::Duration::from_secs(20), async { while rx.borrow().is_none() { if rx.changed().await.is_err() { break; } } }).await;
        let v = rx.borrow().clone().flatten();
        // Only reuse a real answer. If the earlier identical request was cancelled or failed, fall through and run our own.
        if let Some(text) = v { return Ok(CompletionResult { request_id: req.request_id, text, model_id: None, latency_ms: started.elapsed().as_millis() as u64, cached: true }); }
        if let Some(hit) = st.cache.lock().get(&key) { return Ok(CompletionResult { request_id: req.request_id, text: hit.clone(), model_id: None, latency_ms: started.elapsed().as_millis() as u64, cached: true }); }
    }
    let (tx, rx) = watch::channel::<Option<Option<String>>>(None);
    st.inflight.lock().insert(key.clone(), rx);
    struct Cleanup<'a>(&'a TabState, String, watch::Sender<Option<Option<String>>>, Option<String>);
    impl Drop for Cleanup<'_> { fn drop(&mut self) { self.0.inflight.lock().remove(&self.1); let _ = self.2.send(Some(self.3.take())); } }
    let mut cleanup = Cleanup(&st, key.clone(), tx, None);

    let (resolved, fallback) = pick_for_tab(&st, &settings, &settings_state)?;
    let max_lines = if req.multiline { settings.tab.max_lines.max(1) as usize } else { 1 };
    let mut llm_req = LlmRequest::new(resolved.model.clone()).no_thinking().max_tokens(if req.multiline { 256 } else { 96 }).purpose("tab");
    llm_req.system = vec![crate::ai::llm::SystemBlock { text: SYSTEM.replace("{MAX_LINES}", &max_lines.to_string()), cache: true }];
    llm_req.messages = vec![crate::ai::llm::Message::user_text(build_user_prompt(&req, settings.tab.context_lines.max(10) as usize))];
    llm_req.temperature = Some(0.1);
    llm_req.stop = vec!["</completion>".into()];
    let guard = cancel::Guard::new(&req.request_id);
    let (raw, usage, resolved) = complete_with_fallback(&st, resolved, fallback, llm_req, guard.token(), "tab").await?;
    tracing::debug!(model = %resolved.model.id, input = usage.input_tokens, output = usage.output_tokens, ms = started.elapsed().as_millis() as u64, "tab completion");
    let text = clean_completion(&raw, &req.prefix, &req.suffix, max_lines).unwrap_or_default();
    if text.is_empty() {
        // Usually the model echoed the file or answered in prose; keep the head of the raw answer so the pattern can be diagnosed from the log.
        tracing::debug!(model = %resolved.model.id, output = usage.output_tokens, raw = %head_chars(&raw, 300).replace('\n', "⏎"), "tab completion produced nothing usable");
    }
    if !text.is_empty() { st.cache.lock().put(key, text.clone()); }
    cleanup.3 = Some(text.clone());
    Ok(CompletionResult { request_id: req.request_id, text, model_id: Some(resolved.model.id.clone()), latency_ms: started.elapsed().as_millis() as u64, cached: false })
}

const NEXT_EDIT_SYSTEM: &str = "You predict the next edit a developer will make. Given a file and the edits they just made, decide whether another edit elsewhere in the SAME file is clearly implied (e.g. a renamed symbol still used under its old name, a changed function signature whose call sites must change, a new enum variant needing a match arm, an added parameter missing at call sites). Only report edits that are near-certain; otherwise report found=false.\nRespond with JSON only: {\"found\": boolean, \"startLine\": number, \"endLine\": number, \"newText\": string, \"reason\": string}. Lines are 1-based and inclusive; newText replaces exactly those lines (no trailing newline needed). Keep the edit small and local (≤ 15 lines).";

pub async fn next_edit(app: &AppHandle, req: NextEditRequest) -> Result<NextEditResult> {
    let settings_state = app.state::<SettingsState>();
    let settings = settings_state.effective();
    if req.recent_edits.is_empty() { return Ok(NextEditResult { request_id: req.request_id, found: false, ..Default::default() }); }
    let lines: Vec<&str> = req.content.lines().collect();
    // Window ~400 lines around the most recent edit lines + cursor.
    let mut focus: Vec<usize> = req.recent_edits.iter().map(|e| e.line as usize).collect();
    focus.push(req.cursor_line as usize);
    let center = focus.iter().sum::<usize>() / focus.len().max(1);
    let start = center.saturating_sub(200).min(lines.len());
    let end = (center + 200).min(lines.len());
    let mut numbered = String::new();
    for (i, l) in lines[start..end].iter().enumerate() { numbered.push_str(&format!("{:>5}| {}\n", start + i + 1, l)); }
    let mut edits = String::new();
    for e in req.recent_edits.iter().rev().take(5).rev() { edits.push_str(&format!("line {}:\n- {}\n+ {}\n", e.line, head_chars(&e.before, 400), head_chars(&e.after, 400))); }
    let st = app.state::<TabState>();
    let (resolved, fallback) = pick_for_tab(&st, &settings, &settings_state)?;
    let mut llm_req = LlmRequest::new(resolved.model.clone()).no_thinking().max_tokens(600).purpose("next-edit");
    llm_req.system = vec![crate::ai::llm::SystemBlock { text: NEXT_EDIT_SYSTEM.into(), cache: true }];
    llm_req.messages = vec![crate::ai::llm::Message::user_text(format!("Language: {}\nFile: {}\nCursor line: {}\n\nRecent edits:\n{}\nFile (lines {}-{}):\n{}", req.language, req.path, req.cursor_line, edits, start + 1, end, numbered))];
    llm_req.temperature = Some(0.0);
    let guard = cancel::Guard::new(&req.request_id);
    let (raw, _, _resolved) = complete_with_fallback(&st, resolved, fallback, llm_req, guard.token(), "next-edit").await?;
    let mut out = NextEditResult { request_id: req.request_id.clone(), found: false, ..Default::default() };
    let Some(v) = extract_json(&raw) else { return Ok(out) };
    if v["found"].as_bool() != Some(true) { return Ok(out); }
    let (Some(s), Some(e), Some(t)) = (v["startLine"].as_u64(), v["endLine"].as_u64(), v["newText"].as_str()) else { return Ok(out) };
    let (s, e) = (s as usize, e as usize);
    if s == 0 || e < s || e > lines.len() || e - s > 40 { return Ok(out); }
    let original = lines[s - 1..e].join("\n");
    if original.trim() == t.trim() { return Ok(out); }
    out.found = true; out.start_line = Some(s as u32); out.end_line = Some(e as u32); out.new_text = Some(t.to_string()); out.reason = v["reason"].as_str().map(String::from);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleans_completions() {
        assert_eq!(clean_completion("<completion>foo(bar)</completion>", "let x = ", "", 1).unwrap(), "foo(bar)");
        assert_eq!(clean_completion("```\nreturn 1;\n```", "  ", "", 1).unwrap(), "return 1;");
        // repeats suffix start → dropped
        assert!(clean_completion("<completion>}</completion>", "  foo();\n", "}\n", 1).is_none());
        // trims overlapping suffix
        assert_eq!(clean_completion("<completion>x + 1)</completion>", "let y = (", ")", 1).unwrap(), "x + 1");
        // caps lines
        assert_eq!(clean_completion("a\nb\nc", "", "", 2).unwrap(), "a\nb");
        assert!(clean_completion("<completion>   </completion>", "", "", 1).is_none());
        // does not repeat the current line prefix
        assert_eq!(clean_completion("console.log(x)", "  console.", "", 1).unwrap(), "log(x)");
    }
    #[test]
    fn cache_key_stable_and_bounded() {
        let mk = |p: &str| CompletionRequest { request_id: "1".into(), path: "a.ts".into(), language: "typescript".into(), prefix: p.into(), suffix: "".into(), cursor_line: 1, cursor_column: 1, recent_edits: None, diagnostics: None, open_files: None, multiline: true, manual: None };
        assert_eq!(cache_key(&mk("abc")), cache_key(&mk("abc")));
        assert_ne!(cache_key(&mk("abc")), cache_key(&mk("abd")));
        let tail = "t".repeat(2100);
        assert_eq!(cache_key(&mk(&("x".repeat(5000) + &tail))), cache_key(&mk(&("y".repeat(5000) + &tail))), "only the last 2 KiB matter");
    }
}

#[cfg(test)]
mod clean_tests {
    use super::*;
    #[test]
    fn strips_banner_fence_and_repeated_prefix() {
        let prefix = "export function average(values: number[]): number {\n  if (values.length === 0) return 0\n  ";
        let raw = "*[Lynkr] SIMPLE → z-ai/glm (openrouter)*\n\n```typescript\nexport function average(values: number[]): number {\n  if (values.length === 0) return 0\n  let sum = 0\n  for (const v of values) sum += v\n  return sum / values.length\n}\n```";
        let out = clean_completion(raw, prefix, "\n}\n", 12).unwrap();
        assert!(out.starts_with("let sum = 0"), "got: {out:?}");
        assert!(!out.contains("export function"));
        assert!(!out.trim_end().ends_with('}') || out.contains("return"));
    }
    #[test]
    fn keeps_plain_completion() {
        assert_eq!(clean_completion("  return a + b", "function add(a, b) {\n", "}\n", 12).as_deref(), Some("  return a + b"));
    }
}

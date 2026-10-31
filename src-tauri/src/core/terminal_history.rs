//! Persisted, cross-session terminal command history.
//!
//! Finished commands (OSC 133 blocks) are appended as JSONL to `~/.caret/terminal/history.jsonl` and
//! loaded lazily into memory. Two search modes: lexical (token scoring + recency) and natural-language
//! (a fast model turns a description into keywords, then lexical search runs again). Secret-looking
//! commands are never stored and long tokens are redacted from the captured output head.
use crate::ai::cancel;
use crate::ai::llm::{self, LlmRequest, Message, SystemBlock};
use crate::ai::util::{extract_json, now_ms, pick_model};
use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

/// Hard cap on stored entries; exceeding it rewrites the file keeping the newest `KEEP_ON_COMPACT`.
pub const MAX_ENTRIES: usize = 20_000;
pub const KEEP_ON_COMPACT: usize = 15_000;
/// `output_head` is capped to this many chars.
pub const OUTPUT_HEAD_CHARS: usize = 2_000;
/// Same command + cwd within this window is treated as a duplicate and skipped.
const DUP_WINDOW_MS: u64 = 2_000;
const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 500;
/// Cancel-registry id for the NL keyword call; a newer search replaces (cancels) an in-flight one.
const NL_CANCEL_ID: &str = "terminal-history-nl";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    /// Unix epoch milliseconds when the command started.
    pub ts: u64,
    pub cwd: String,
    pub shell: String,
    pub command: String,
    #[serde(default)] pub exit_code: Option<i32>,
    #[serde(default)] pub duration_ms: Option<u64>,
    /// Plain-text head of the output (≤ `OUTPUT_HEAD_CHARS`, secrets redacted).
    #[serde(default)] pub output_head: String,
    #[serde(default)] pub workspace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySearchHit { pub entry: HistoryEntry, pub score: f32, pub why: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryNlResult { pub hits: Vec<HistorySearchHit>, pub keywords: Vec<String>, pub used_model: bool }

/// `~/.caret/terminal/history.jsonl` (honours `CARET_HOME`).
pub fn default_path() -> PathBuf { crate::paths::sub_dir("terminal").join("history.jsonl") }

/// Managed in `lib.rs`; wraps the default on-disk store.
pub struct TerminalHistoryState(pub HistoryStore);
impl Default for TerminalHistoryState { fn default() -> Self { Self(HistoryStore::new(default_path())) } }

/// On-disk + in-memory store. Entries are oldest → newest; `None` until first use (lazy load).
pub struct HistoryStore {
    path: PathBuf,
    max_entries: usize,
    keep_on_compact: usize,
    entries: RwLock<Option<Vec<HistoryEntry>>>,
}

impl HistoryStore {
    pub fn new(path: impl Into<PathBuf>) -> Self { Self::with_limits(path, MAX_ENTRIES, KEEP_ON_COMPACT) }
    pub fn with_limits(path: impl Into<PathBuf>, max_entries: usize, keep_on_compact: usize) -> Self {
        let max_entries = max_entries.max(1);
        Self { path: path.into(), max_entries, keep_on_compact: keep_on_compact.clamp(1, max_entries), entries: RwLock::new(None) }
    }
    pub fn path(&self) -> &Path { &self.path }

    fn ensure_loaded(&self) {
        if self.entries.read().is_some() { return; }
        let mut w = self.entries.write();
        if w.is_none() { *w = Some(load_file(&self.path)); }
    }
    /// Run `f` over the loaded entries (oldest → newest).
    fn with<R>(&self, f: impl FnOnce(&[HistoryEntry]) -> R) -> R {
        self.ensure_loaded();
        let g = self.entries.read();
        f(g.as_deref().unwrap_or(&[]))
    }
    pub fn len(&self) -> usize { self.with(|e| e.len()) }
    pub fn is_empty(&self) -> bool { self.len() == 0 }

    /// Append one entry. Returns `Ok(false)` when it was skipped (empty, consecutive duplicate, or secret-looking).
    pub fn append(&self, mut entry: HistoryEntry) -> Result<bool> {
        entry.command = entry.command.trim().to_string();
        if entry.command.is_empty() || looks_like_secret_command(&entry.command) { return Ok(false); }
        if entry.id.is_empty() { entry.id = uuid::Uuid::new_v4().to_string(); }
        if entry.ts == 0 { entry.ts = now_ms(); }
        entry.output_head = redact(&truncate_chars(&entry.output_head, OUTPUT_HEAD_CHARS));
        self.ensure_loaded();
        let mut w = self.entries.write();
        let list = w.get_or_insert_with(Vec::new);
        if let Some(last) = list.last() {
            if last.command == entry.command && last.cwd == entry.cwd && last.ts.abs_diff(entry.ts) <= DUP_WINDOW_MS { return Ok(false); }
        }
        let line = serde_json::to_string(&entry)?;
        list.push(entry);
        if list.len() > self.max_entries {
            let drop = list.len() - self.keep_on_compact;
            list.drain(..drop);
            write_all(&self.path, list)?;
        } else {
            append_line(&self.path, &line)?;
        }
        Ok(true)
    }

    /// Most recent first, distinct by command. With a `prefix`, matching commands come first and, among those,
    /// ones that ran in `cwd` rank above the rest; non-matching commands follow in recency order.
    pub fn recent(&self, prefix: Option<&str>, cwd: Option<&str>, limit: usize) -> Vec<HistoryEntry> {
        let prefix = prefix.map(str::trim_start).filter(|p| !p.is_empty());
        let cwd = cwd.map(str::trim).filter(|c| !c.is_empty());
        self.with(|entries| {
            // command → (best tier, newest entry)
            let mut best: HashMap<&str, (u8, &HistoryEntry)> = HashMap::new();
            for e in entries.iter().rev() {
                let tier = match prefix {
                    Some(p) if e.command.starts_with(p) => u8::from(cwd != Some(e.cwd.as_str())),
                    Some(_) => 2,
                    None => 0,
                };
                match best.get_mut(e.command.as_str()) {
                    Some(b) => { if tier < b.0 { b.0 = tier; } }
                    None => { best.insert(e.command.as_str(), (tier, e)); }
                }
            }
            let mut v: Vec<(u8, &HistoryEntry)> = best.into_values().collect();
            v.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.ts.cmp(&a.1.ts)).then_with(|| a.1.command.cmp(&b.1.command)));
            v.into_iter().take(limit).map(|(_, e)| e.clone()).collect()
        })
    }

    /// Lexical search: tokens matched in command (×3), cwd (×1) and output head (×1), plus a log-scale recency
    /// boost and a small boost for failed commands when the query mentions errors/failures. Distinct by command.
    pub fn search(&self, query: &str, limit: usize) -> Vec<HistorySearchHit> { self.search_at(query, limit, now_ms()) }

    pub fn search_at(&self, query: &str, limit: usize, now: u64) -> Vec<HistorySearchHit> {
        let tokens = tokenize(query);
        if tokens.is_empty() { return vec![]; }
        let wants_failures = tokens.iter().any(|t| FAILURE_WORDS.contains(&t.as_str()));
        self.with(|entries| {
            let mut best: HashMap<&str, HistorySearchHit> = HashMap::new();
            for e in entries.iter().rev() {
                let Some(hit) = score_entry(e, &tokens, wants_failures, now) else { continue };
                match best.get_mut(e.command.as_str()) {
                    Some(prev) if prev.score >= hit.score => {}
                    Some(prev) => *prev = hit,
                    None => { best.insert(e.command.as_str(), hit); }
                }
            }
            let mut hits: Vec<HistorySearchHit> = best.into_values().collect();
            sort_hits(&mut hits);
            hits.truncate(limit);
            hits
        })
    }

    pub fn clear(&self) -> Result<()> {
        let mut w = self.entries.write();
        *w = Some(Vec::new());
        if let Some(parent) = self.path.parent() { std::fs::create_dir_all(parent)?; }
        std::fs::write(&self.path, b"")?;
        Ok(())
    }
}

// ───────────────────────────── scoring ─────────────────────────────

const FAILURE_WORDS: &[&str] = &["error", "errors", "fail", "failed", "failing", "failure", "failures", "broke", "broken"];

/// Lowercase whitespace/comma-separated tokens, quotes and trailing sentence punctuation stripped, deduped.
pub fn tokenize(q: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in q.to_lowercase().split(|c: char| c.is_whitespace() || c == ',') {
        let t = raw.trim_matches(|c: char| c == '"' || c == '\'' || c == '`').trim_end_matches(|c: char| matches!(c, '?' | '.' | '!' | ':' | ';'));
        if t.is_empty() || out.iter().any(|x| x == t) { continue; }
        out.push(t.to_string());
    }
    out
}

fn score_entry(e: &HistoryEntry, tokens: &[String], wants_failures: bool, now: u64) -> Option<HistorySearchHit> {
    let cmd = e.command.to_lowercase();
    let cwd = e.cwd.to_lowercase();
    let out = e.output_head.to_lowercase();
    let mut score = 0f32;
    let (mut in_cmd, mut in_cwd, mut in_out) = (Vec::new(), Vec::new(), Vec::new());
    for t in tokens {
        if cmd.contains(t.as_str()) { score += 3.0; in_cmd.push(t.as_str()); }
        if cwd.contains(t.as_str()) { score += 1.0; in_cwd.push(t.as_str()); }
        if out.contains(t.as_str()) { score += 1.0; in_out.push(t.as_str()); }
    }
    if score == 0.0 { return None; }
    let mut why = Vec::new();
    if !in_cmd.is_empty() { why.push(format!("command: {}", in_cmd.join(", "))); }
    if !in_cwd.is_empty() { why.push(format!("cwd: {}", in_cwd.join(", "))); }
    if !in_out.is_empty() { why.push(format!("output: {}", in_out.join(", "))); }
    if wants_failures && e.exit_code.map(|c| c != 0).unwrap_or(false) { score += 1.0; why.push("failed".into()); }
    score += recency_boost(e.ts, now);
    Some(HistorySearchHit { entry: e.clone(), score, why: why.join(" · ") })
}

/// 1.5 for a command that just ran, decaying logarithmically with age in hours (≈0.36 after a day, ≈0.2 after a month).
/// Always below a single command-token match (3.0) so relevance beats recency.
fn recency_boost(ts: u64, now: u64) -> f32 {
    let age_h = now.saturating_sub(ts) as f32 / 3_600_000.0;
    1.5 / (1.0 + (1.0 + age_h).ln())
}

fn sort_hits(hits: &mut [HistorySearchHit]) {
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then(b.entry.ts.cmp(&a.entry.ts)).then_with(|| a.entry.command.cmp(&b.entry.command)));
}

/// Union two hit lists by command, keeping the better score; sorted best-first and truncated to `limit`.
pub fn merge_hits(a: Vec<HistorySearchHit>, b: Vec<HistorySearchHit>, limit: usize) -> Vec<HistorySearchHit> {
    let mut best: HashMap<String, HistorySearchHit> = HashMap::new();
    for h in a.into_iter().chain(b) {
        match best.get_mut(&h.entry.command) {
            Some(prev) if prev.score >= h.score => {}
            Some(prev) => *prev = h,
            None => { best.insert(h.entry.command.clone(), h); }
        }
    }
    let mut hits: Vec<HistorySearchHit> = best.into_values().collect();
    sort_hits(&mut hits);
    hits.truncate(limit);
    hits
}

// ───────────────────────────── secrets ─────────────────────────────

static SECRET_ASSIGN: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(api[_-]?key|token|secret|password)\s*=").unwrap());
static BEARER_IN_COMMAND: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)authorization:\s*bearer\s+\S+").unwrap());
static BEARER: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(authorization:\s*bearer\s+)[A-Za-z0-9\-._~+/]+=*").unwrap());
static JWT: Lazy<Regex> = Lazy::new(|| Regex::new(r"\beyJ[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+\.[A-Za-z0-9_\-]+").unwrap());

/// `export API_KEY=…`, `TOKEN=… cmd`, `… password=…`, or an inline `Authorization: Bearer …` header.
pub fn looks_like_secret_command(cmd: &str) -> bool { SECRET_ASSIGN.is_match(cmd) || BEARER_IN_COMMAND.is_match(cmd) }

/// Redact bearer tokens, JWTs, long hex strings (≥ 32) and long mixed alphanumeric tokens (≥ 40, base64/API-key shaped).
pub fn redact(text: &str) -> String {
    let t = BEARER.replace_all(text, "${1}[redacted]");
    let t = JWT.replace_all(&t, "[redacted]");
    redact_long_tokens(&t)
}

fn redact_long_tokens(text: &str) -> String {
    fn is_tok(c: char) -> bool { c.is_ascii_alphanumeric() || matches!(c, '+' | '_' | '-' | '=') }
    fn flush(run: &mut String, out: &mut String) {
        if run.is_empty() { return; }
        let n = run.len();
        let hex = run.chars().all(|c| c.is_ascii_hexdigit());
        let has_digit = run.chars().any(|c| c.is_ascii_digit());
        let has_alpha = run.chars().any(|c| c.is_ascii_alphabetic());
        if (hex && n >= 32) || (n >= 40 && has_digit && has_alpha) { out.push_str("[redacted]"); } else { out.push_str(run); }
        run.clear();
    }
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    for c in text.chars() {
        if is_tok(c) { run.push(c); } else { flush(&mut run, &mut out); out.push(c); }
    }
    flush(&mut run, &mut out);
    out
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max { return s.to_string(); }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

// ───────────────────────────── file I/O ─────────────────────────────

fn load_file(path: &Path) -> Vec<HistoryEntry> {
    let Ok(f) = std::fs::File::open(path) else { return Vec::new() };
    let mut out = Vec::new();
    for line in std::io::BufReader::new(f).lines().map_while(|l| l.ok()) {
        let t = line.trim();
        if t.is_empty() { continue; }
        match serde_json::from_str::<HistoryEntry>(t) {
            Ok(e) => out.push(e),
            Err(e) => tracing::debug!(error = %e, "terminal history: skipping corrupt line"),
        }
    }
    out
}

fn append_line(path: &Path, line: &str) -> Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(line.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(())
}

/// Rewrite the whole file atomically (tmp + rename).
fn write_all(path: &Path, entries: &[HistoryEntry]) -> Result<()> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut f = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        for e in entries { serde_json::to_writer(&mut f, e)?; f.write_all(b"\n")?; }
        f.flush()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ───────────────────────────── natural-language search ─────────────────────────────

const NL_SYSTEM: &str = "You turn a user's natural-language description of a past terminal command into search keywords.\n\
Respond with JSON only: {\"keywords\": [..], \"cwd_hint\": string|null}\n\
- keywords: 3 to 8 lowercase terms likely to appear in the command line itself (program names, subcommands, flags, package or file names) or in its output (error messages, tool names).\n\
- cwd_hint: a directory or project name the user mentioned, else null.\n\
No prose, no markdown fences.";

pub struct NlKeywords { pub keywords: Vec<String>, pub cwd_hint: Option<String> }

/// Parse the model's `{"keywords": [...], "cwd_hint": ...}` reply leniently (fences, prose, string instead of array).
pub fn parse_nl_keywords(raw: &str) -> NlKeywords {
    let v = extract_json(raw);
    let mut keywords: Vec<String> = Vec::new();
    if let Some(arr) = v.as_ref().and_then(|v| v["keywords"].as_array()) {
        for k in arr.iter().filter_map(|k| k.as_str()) {
            for t in tokenize(k) { if !keywords.contains(&t) { keywords.push(t); } }
        }
    } else if let Some(s) = v.as_ref().and_then(|v| v["keywords"].as_str()) {
        keywords = tokenize(s);
    }
    keywords.truncate(8);
    let cwd_hint = v.as_ref().and_then(|v| v["cwd_hint"].as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s != "null");
    NlKeywords { keywords, cwd_hint }
}

/// Ask the model when the query reads like a sentence (≥ 3 words) or the lexical search came back thin (< 3 hits).
pub fn should_ask_model(query: &str, lexical_hits: usize) -> bool { query.split_whitespace().count() >= 3 || lexical_hits < 3 }

async fn nl_search(app: &AppHandle, store: &HistoryStore, query: &str, limit: usize) -> HistoryNlResult {
    let lexical = store.search(query, limit);
    if !should_ask_model(query, lexical.len()) { return HistoryNlResult { hits: lexical, keywords: vec![], used_model: false }; }
    let fallback = |hits: Vec<HistorySearchHit>| HistoryNlResult { hits, keywords: vec![], used_model: false };
    let settings_state = app.state::<SettingsState>();
    let settings = settings_state.effective();
    let resolved = match pick_model(&settings, &settings_state, None, true) {
        Ok(r) => r,
        Err(e) => { tracing::debug!(error = %e, "terminal history: no model available for NL search"); return fallback(lexical); }
    };
    let mut user = String::new();
    if let Some(root) = crate::ai::util::workspace_root(app) { user.push_str(&format!("Current workspace: {}\n", root.display())); }
    user.push_str(&format!("Description: {}", query.trim()));
    let mut req = LlmRequest::new(resolved.model.clone()).no_thinking().max_tokens(120).purpose("terminal-history");
    req.system = vec![SystemBlock { text: NL_SYSTEM.into(), cache: true }];
    req.messages = vec![Message::user_text(user)];
    req.temperature = Some(0.0);
    let guard = cancel::Guard::new(NL_CANCEL_ID);
    let raw = match llm::complete_text(&resolved, req, guard.token()).await {
        Ok((raw, _)) => raw,
        Err(e) => { tracing::warn!(error = %e, "terminal history: NL keyword extraction failed"); return fallback(lexical); }
    };
    let parsed = parse_nl_keywords(&raw);
    if parsed.keywords.is_empty() { return fallback(lexical); }
    let mut kw_query = parsed.keywords.join(" ");
    if let Some(seg) = parsed.cwd_hint.as_deref().and_then(|h| Path::new(h).file_name()).and_then(|s| s.to_str()) { kw_query.push(' '); kw_query.push_str(seg); }
    let by_keywords = store.search(&kw_query, limit);
    HistoryNlResult { hits: merge_hits(lexical, by_keywords, limit), keywords: parsed.keywords, used_model: true }
}

// ───────────────────────────── commands ─────────────────────────────

fn clamp_limit(limit: Option<usize>) -> usize { limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) }

#[tauri::command(async)]
pub fn terminal_history_append(st: State<'_, TerminalHistoryState>, entry: HistoryEntry) -> Result<()> { st.0.append(entry).map(|_| ()) }

#[tauri::command(async)]
pub fn terminal_history_recent(st: State<'_, TerminalHistoryState>, prefix: Option<String>, cwd: Option<String>, limit: Option<usize>) -> Result<Vec<HistoryEntry>> {
    Ok(st.0.recent(prefix.as_deref(), cwd.as_deref(), clamp_limit(limit)))
}

#[tauri::command(async)]
pub fn terminal_history_search(st: State<'_, TerminalHistoryState>, query: String, limit: Option<usize>) -> Result<Vec<HistorySearchHit>> {
    Ok(st.0.search(&query, clamp_limit(limit)))
}

#[tauri::command]
pub async fn terminal_history_nl_search(app: AppHandle, st: State<'_, TerminalHistoryState>, query: String, limit: Option<usize>) -> Result<HistoryNlResult> {
    if query.trim().is_empty() { return Err(CaretError::invalid("query is empty")); }
    Ok(nl_search(&app, &st.0, &query, clamp_limit(limit)).await)
}

#[tauri::command(async)]
pub fn terminal_history_clear(st: State<'_, TerminalHistoryState>) -> Result<()> { st.0.clear() }

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(cmd: &str, cwd: &str, ts: u64) -> HistoryEntry {
        HistoryEntry { id: String::new(), ts, cwd: cwd.into(), shell: "zsh".into(), command: cmd.into(), exit_code: Some(0), duration_ms: Some(10), output_head: String::new(), workspace: None }
    }
    fn failed(mut e: HistoryEntry, out: &str) -> HistoryEntry { e.exit_code = Some(1); e.output_head = out.into(); e }
    fn cmds(v: &[HistoryEntry]) -> Vec<&str> { v.iter().map(|e| e.command.as_str()).collect() }

    #[test]
    fn append_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        {
            let s = HistoryStore::new(&path);
            assert!(s.append(entry("ls -la", "/a", 1_000)).unwrap());
            assert!(s.append(entry("git status", "/a", 5_000)).unwrap());
            assert_eq!(s.len(), 2);
        }
        let s = HistoryStore::new(&path);
        let all = s.recent(None, None, 10);
        assert_eq!(cmds(&all), vec!["git status", "ls -la"]);
        assert!(!all[0].id.is_empty(), "ids are assigned when missing");
        assert_eq!(all[1].shell, "zsh");
        assert_eq!(std::fs::read_to_string(&path).unwrap().lines().count(), 2);
    }

    #[test]
    fn skips_empty_and_consecutive_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let s = HistoryStore::new(dir.path().join("h.jsonl"));
        assert!(!s.append(entry("   ", "/a", 1_000)).unwrap());
        assert!(s.append(entry("npm test", "/a", 1_000)).unwrap());
        assert!(!s.append(entry("npm test", "/a", 2_500)).unwrap(), "same command+cwd within 2s is a duplicate");
        assert!(s.append(entry("npm test", "/b", 2_600)).unwrap(), "a different cwd is not a duplicate");
        assert!(s.append(entry("npm test", "/b", 6_000)).unwrap(), "after the window it is recorded again");
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn compacts_when_over_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.jsonl");
        let s = HistoryStore::with_limits(&path, 10, 7);
        for i in 0..11u64 { assert!(s.append(entry(&format!("cmd {i}"), "/a", 10_000 * (i + 1))).unwrap()); }
        assert_eq!(s.len(), 7);
        let lines: Vec<String> = std::fs::read_to_string(&path).unwrap().lines().map(String::from).collect();
        assert_eq!(lines.len(), 7, "file is rewritten with the newest entries only");
        let first: HistoryEntry = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(first.command, "cmd 4", "oldest entries are dropped");
        assert_eq!(s.recent(None, None, 1)[0].command, "cmd 10");
        assert_eq!(HistoryStore::new(&path).len(), 7, "reload sees the compacted file");
        assert!(!path.with_extension("jsonl.tmp").exists());
    }

    #[test]
    fn filters_secret_commands_and_redacts_output() {
        let dir = tempfile::tempdir().unwrap();
        let s = HistoryStore::new(dir.path().join("h.jsonl"));
        for c in ["export API_KEY=sk-123", "export OPENAI_API_KEY = abc", "TOKEN=xyz ./run", "echo password=hunter2", "set SECRET=1", "curl -H 'Authorization: Bearer abc.def' https://x"] {
            assert!(!s.append(entry(c, "/a", 1_000)).unwrap(), "{c} must not be stored");
        }
        assert!(s.append(entry("echo token", "/a", 1_000)).unwrap(), "the word alone is fine");
        let hex = "0123456789abcdef".repeat(3);
        let b64 = "AbC123xyz".repeat(6);
        let mut e = entry("curl https://api.example.com", "/a", 2_000);
        e.output_head = format!("Authorization: Bearer abcDEF123.xyz\nsha {hex}\nkey {b64}\ncommit a1b2c3d ok\njwt eyJhbGciOi.eyJzdWIiOi.SflKxwRJ");
        assert!(s.append(e).unwrap());
        let stored = &s.recent(Some("curl"), None, 1)[0];
        assert!(stored.output_head.contains("Authorization: Bearer [redacted]"));
        assert!(!stored.output_head.contains("abcDEF123") && !stored.output_head.contains(&hex) && !stored.output_head.contains(&b64));
        assert!(!stored.output_head.contains("eyJhbGciOi"));
        assert!(stored.output_head.contains("commit a1b2c3d ok"), "short hashes and words survive");
        let mut long = entry("yes", "/a", 9_000);
        long.output_head = "y\n".repeat(5_000);
        assert!(s.append(long).unwrap());
        assert!(s.recent(Some("yes"), None, 1)[0].output_head.chars().count() <= OUTPUT_HEAD_CHARS);
        assert!(!redact("------------------------------------------------------").contains("[redacted]"), "separator lines are not tokens");
    }

    #[test]
    fn recent_prefix_ranking_and_distinct() {
        let dir = tempfile::tempdir().unwrap();
        let s = HistoryStore::new(dir.path().join("h.jsonl"));
        s.append(entry("git status", "/proj/a", 10_000)).unwrap();
        s.append(entry("git push", "/proj/b", 20_000)).unwrap();
        s.append(entry("ls", "/proj/a", 30_000)).unwrap();
        s.append(entry("git log", "/proj/a", 40_000)).unwrap();
        s.append(entry("git status", "/proj/a", 50_000)).unwrap();
        assert_eq!(cmds(&s.recent(None, None, 10)), vec!["git status", "git log", "ls", "git push"], "newest first, distinct");
        assert_eq!(cmds(&s.recent(Some("git"), Some("/proj/a"), 10)), vec!["git status", "git log", "git push", "ls"], "prefix+cwd, then prefix, then the rest");
        assert_eq!(cmds(&s.recent(Some("git p"), None, 10)), vec!["git push", "git status", "git log", "ls"]);
        assert_eq!(cmds(&s.recent(Some("git"), Some("/proj/a"), 2)), vec!["git status", "git log"]);
        assert_eq!(cmds(&s.recent(Some("GIT"), None, 10)), vec!["git status", "git log", "ls", "git push"], "prefix is case-sensitive");
    }

    #[test]
    fn lexical_search_scores_and_dedupes() {
        let dir = tempfile::tempdir().unwrap();
        let s = HistoryStore::new(dir.path().join("h.jsonl"));
        let now = 1_700_000_000_000u64;
        let h = 3_600_000u64;
        s.append(failed(entry("npm test", "/w/caret", now - 48 * h), "12 passing\n1 failing\nError: expected 2")).unwrap();
        s.append(entry("cargo test --lib", "/w/caret/src-tauri", now - 2 * h)).unwrap();
        s.append(entry("npm run build", "/w/caret", now - h / 2)).unwrap();
        s.append(entry("npm test", "/w/caret", now - 10_000)).unwrap();

        let hits = s.search_at("npm test", 10, now);
        assert_eq!(hits.iter().map(|h| h.entry.command.as_str()).collect::<Vec<_>>(), vec!["npm test", "npm run build", "cargo test --lib"]);
        assert!(hits[0].score > hits[1].score && hits[1].score > hits[2].score);
        assert!(hits[0].why.contains("command: npm, test"), "why = {}", hits[0].why);
        assert_eq!(hits.iter().filter(|h| h.entry.command == "npm test").count(), 1, "distinct by command");
        assert_eq!(hits[0].entry.exit_code, Some(0), "the newest occurrence wins when scores tie on tokens");

        let hits = s.search_at("error", 10, now);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.exit_code, Some(1));
        assert!(hits[0].why.contains("output: error") && hits[0].why.contains("failed"), "why = {}", hits[0].why);

        let hits = s.search_at("src-tauri", 10, now);
        assert_eq!(hits[0].entry.command, "cargo test --lib");
        assert!(hits[0].why.starts_with("cwd:"));

        assert!(s.search_at("   ", 10, now).is_empty());
        assert!(s.search_at("zzz-nomatch", 10, now).is_empty());
        assert_eq!(s.search_at("npm", 1, now).len(), 1, "limit is honoured");
    }

    #[test]
    fn nl_helpers_parse_keywords_and_merge() {
        let k = parse_nl_keywords("```json\n{\"keywords\": [\"npm\", \"Test\", \"npm\"], \"cwd_hint\": \"caret\"}\n```");
        assert_eq!(k.keywords, vec!["npm", "test"]);
        assert_eq!(k.cwd_hint.as_deref(), Some("caret"));
        let k = parse_nl_keywords("Sure: {\"keywords\": \"docker compose up\", \"cwd_hint\": null}");
        assert_eq!(k.keywords, vec!["docker", "compose", "up"]);
        assert!(k.cwd_hint.is_none());
        let k = parse_nl_keywords("not json at all");
        assert!(k.keywords.is_empty() && k.cwd_hint.is_none());

        assert!(should_ask_model("the docker thing", 10), "3+ words reads like a sentence");
        assert!(should_ask_model("docker", 1), "thin lexical results");
        assert!(!should_ask_model("docker ps", 5));

        let e = entry("ls", "/a", 1);
        let a = vec![HistorySearchHit { entry: e.clone(), score: 2.0, why: "a".into() }];
        let b = vec![HistorySearchHit { entry: e, score: 5.0, why: "b".into() }, HistorySearchHit { entry: entry("pwd", "/a", 2), score: 1.0, why: "c".into() }];
        let m = merge_hits(a, b, 10);
        assert_eq!(m.len(), 2);
        assert_eq!((m[0].entry.command.as_str(), m[0].score, m[0].why.as_str()), ("ls", 5.0, "b"));
        assert_eq!(merge_hits(vec![], vec![], 10).len(), 0);
    }

    #[test]
    fn tokenize_normalises() {
        assert_eq!(tokenize("Where did I run Docker?"), vec!["where", "did", "i", "run", "docker"]);
        assert_eq!(tokenize("\"npm\" npm,  TEST."), vec!["npm", "test"]);
        assert_eq!(tokenize("./run.sh --flag"), vec!["./run.sh", "--flag"]);
        assert!(tokenize("  ").is_empty());
    }

    #[test]
    fn clear_empties_memory_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h.jsonl");
        let s = HistoryStore::new(&path);
        s.append(entry("ls", "/a", 1_000)).unwrap();
        s.clear().unwrap();
        assert!(s.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        assert!(s.append(entry("ls", "/a", 1_000)).unwrap(), "not a duplicate after clear");
        assert_eq!(HistoryStore::new(&path).len(), 1);
    }
}

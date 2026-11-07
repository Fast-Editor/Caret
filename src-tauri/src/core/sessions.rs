//! Chat session persistence: ~/.caret/sessions/<workspace-hash>/<id>.json + index.json (summaries).
//! Writes are debounced per session (≤ 1/s) and atomic (temp + rename).
use crate::ai::agent::session::{MessageRole, MessageSearchHit, Session, SessionStatus, SessionSummary};
use crate::error::{CaretError, Result};
use crate::paths;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub fn workspace_key(root: Option<&Path>) -> String { root.map(paths::workspace_hash).unwrap_or_else(|| "global".into()) }
pub fn sessions_dir(ws_key: &str) -> PathBuf { let p = paths::sub_dir("sessions").join(ws_key); let _ = std::fs::create_dir_all(&p); p }
fn session_path(ws_key: &str, id: &str) -> PathBuf { sessions_dir(ws_key).join(format!("{id}.json")) }
fn index_path(ws_key: &str) -> PathBuf { sessions_dir(ws_key).join("index.json") }
/// Every per-workspace key (sub-directory) under `~/.caret/sessions`, sorted.
pub fn all_workspace_keys() -> Vec<String> {
    let root = paths::sub_dir("sessions");
    let mut keys: Vec<String> = std::fs::read_dir(&root).map(|rd| rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
    keys.sort();
    keys
}

/// Byte range `[start, end)` of the first case-insensitive occurrence of `needle` in `hay`.
fn find_ci(hay: &str, needle: &str) -> Option<(usize, usize)> {
    let needle = needle.to_lowercase();
    if needle.is_empty() { return None; }
    // Lower-case copy plus a byte map back to the original string (lower-casing can change byte lengths).
    let mut lower = String::with_capacity(hay.len());
    let mut map: Vec<usize> = Vec::with_capacity(hay.len() + 1);
    for (i, c) in hay.char_indices() { for lc in c.to_lowercase() { let before = lower.len(); lower.push(lc); for _ in before..lower.len() { map.push(i); } } }
    map.push(hay.len());
    let pos = lower.find(&needle)?;
    Some((map[pos], map[(pos + needle.len()).min(map.len() - 1)]))
}

/// `~width` chars of `text` around the first case-insensitive match of `query`, whitespace flattened, ellipses where cut.
pub fn snippet_around(text: &str, query: &str, width: usize) -> Option<String> {
    let (start, end) = find_ci(text, query)?;
    let flat: String = text.chars().map(|c| if c.is_whitespace() { ' ' } else { c }).collect();
    let hit_chars = flat[start..end].chars().count();
    let before = width.saturating_sub(hit_chars) / 3;
    let after = width.saturating_sub(hit_chars) - before;
    let mut s_idx = start;
    for _ in 0..before { match flat[..s_idx].char_indices().next_back() { Some((i, _)) => s_idx = i, None => break } }
    let mut e_idx = end;
    for _ in 0..after { match flat[e_idx..].chars().next() { Some(c) => e_idx += c.len_utf8(), None => break } }
    let mut out = String::new();
    if s_idx > 0 { out.push('…'); }
    out.push_str(flat[s_idx..e_idx].trim());
    if e_idx < flat.len() { out.push('…'); }
    Some(out)
}

fn write_atomic(p: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
    let tmp = p.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}

#[derive(Default)]
struct Pending { last_write: HashMap<String, Instant>, scheduled: HashMap<String, bool> }

/// Debounced writer. `save` schedules a write ≤ 1/s per session; `save_now` writes immediately.
#[derive(Clone, Default)]
pub struct SessionStore { pending: Arc<Mutex<Pending>> }

impl SessionStore {
    pub fn load(&self, ws_key: &str, id: &str) -> Result<Session> {
        let p = session_path(ws_key, id);
        let s = std::fs::read_to_string(&p).map_err(|_| CaretError::not_found(format!("session {id}")))?;
        let mut sess: Session = serde_json::from_str(&s)?;
        sess.status = SessionStatus::Idle;
        Ok(sess)
    }
    pub fn exists(&self, ws_key: &str, id: &str) -> bool { session_path(ws_key, id).exists() }
    /// Workspace key whose directory holds `<id>.json` (sessions of other projects, e.g. "group by project").
    pub fn find_workspace_key(&self, id: &str) -> Option<String> { all_workspace_keys().into_iter().find(|k| self.exists(k, id)) }

    /// Case-insensitive substring search over user/assistant message text in every session file under `keys`
    /// (reads disk, not just loaded sessions). Newest sessions first, capped at `limit`.
    pub fn search_messages(&self, keys: &[String], query: &str, include_archived: bool, limit: usize) -> Vec<MessageSearchHit> {
        let q = query.trim();
        if q.is_empty() || limit == 0 { return vec![]; }
        let mut hits = vec![];
        for key in keys {
            let Ok(rd) = std::fs::read_dir(sessions_dir(key)) else { continue };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name == "index.json" || !name.ends_with(".json") { continue; }
                let Ok(s) = self.load(key, name.trim_end_matches(".json")) else { continue };
                if s.archived && !include_archived { continue; }
                for m in &s.messages {
                    if !matches!(m.role, MessageRole::User | MessageRole::Assistant) { continue; }
                    if let Some(snippet) = snippet_around(&m.text(), q, 160) {
                        hits.push(MessageSearchHit { session_id: s.id.clone(), session_title: s.title.clone(), workspace_root: s.workspace_root.clone(), message_id: m.id.clone(), role: m.role, snippet, updated_at: s.updated_at });
                    }
                }
            }
        }
        hits.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        hits.truncate(limit);
        hits
    }

    pub fn save_now(&self, ws_key: &str, session: &Session) -> Result<()> {
        let bytes = serde_json::to_vec(session)?;
        write_atomic(&session_path(ws_key, &session.id), &bytes)?;
        self.update_index(ws_key, session.summary_view())?;
        self.pending.lock().last_write.insert(session.id.clone(), Instant::now());
        Ok(())
    }

    /// Debounced save: writes now if ≥1s since last write, else schedules one write after the gap.
    pub fn save(&self, ws_key: &str, session: &Session) {
        let id = session.id.clone();
        let due = {
            let mut p = self.pending.lock();
            match p.last_write.get(&id) { Some(t) if t.elapsed() < Duration::from_secs(1) => { if p.scheduled.get(&id).copied().unwrap_or(false) { return; } p.scheduled.insert(id.clone(), true); false } _ => true }
        };
        if due { if let Err(e) = self.save_now(ws_key, session) { tracing::warn!(error=%e, "session save failed"); } return; }
        let snapshot = session.clone();
        let ws = ws_key.to_string();
        let store = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1000)).await;
            store.pending.lock().scheduled.remove(&snapshot.id);
            if let Err(e) = store.save_now(&ws, &snapshot) { tracing::warn!(error=%e, "deferred session save failed"); }
        });
    }

    pub fn delete(&self, ws_key: &str, id: &str) -> Result<()> {
        let _ = std::fs::remove_file(session_path(ws_key, id));
        let mut idx = self.read_index(ws_key);
        idx.retain(|s| s.id != id);
        self.write_index(ws_key, &idx)
    }

    pub fn read_index(&self, ws_key: &str) -> Vec<SessionSummary> {
        let p = index_path(ws_key);
        let mut v: Vec<SessionSummary> = std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        // Self-heal: pick up session files missing from the index.
        if let Ok(rd) = std::fs::read_dir(sessions_dir(ws_key)) {
            let known: std::collections::HashSet<String> = v.iter().map(|s| s.id.clone()).collect();
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name == "index.json" || !name.ends_with(".json") { continue; }
                let id = name.trim_end_matches(".json").to_string();
                if !known.contains(&id) { if let Ok(s) = self.load(ws_key, &id) { v.push(s.summary_view()); } }
            }
        }
        for s in &mut v { s.status = SessionStatus::Idle; }
        v.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        v
    }
    fn write_index(&self, ws_key: &str, idx: &[SessionSummary]) -> Result<()> { write_atomic(&index_path(ws_key), &serde_json::to_vec(idx)?) }
    fn update_index(&self, ws_key: &str, summary: SessionSummary) -> Result<()> {
        let mut idx: Vec<SessionSummary> = std::fs::read_to_string(index_path(ws_key)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        idx.retain(|s| s.id != summary.id);
        idx.push(summary);
        idx.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        idx.truncate(2000);
        self.write_index(ws_key, &idx)
    }

    pub fn export_markdown(session: &Session) -> String {
        use crate::ai::agent::session::{MessagePart, MessageRole};
        let mut out = format!("# {}\n\n_Mode: {} · Model: {} · {}_\n\n", session.title, session.mode, session.model_id.clone().unwrap_or_else(|| "auto".into()), chrono::DateTime::from_timestamp_millis(session.created_at as i64).map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default());
        for m in &session.messages {
            let who = match m.role { MessageRole::User => "**User**", MessageRole::Assistant => "**Caret**", MessageRole::System => "**System**" };
            out.push_str(who); out.push_str("\n\n");
            for p in &m.parts {
                match p {
                    MessagePart::Text { text } => { out.push_str(text); out.push_str("\n\n"); }
                    MessagePart::Thinking { text, .. } => { if !text.trim().is_empty() { out.push_str("<details><summary>Thinking</summary>\n\n"); out.push_str(text); out.push_str("\n\n</details>\n\n"); } }
                    MessagePart::Tool { call } => {
                        out.push_str(&format!("<details><summary>🔧 {} ({:?})</summary>\n\n```json\n{}\n```\n", call.display_name, call.status, serde_json::to_string_pretty(&call.input).unwrap_or_default()));
                        if let Some(o) = &call.output { out.push_str("\n```\n"); out.push_str(&o.chars().take(4000).collect::<String>()); out.push_str("\n```\n"); }
                        out.push_str("\n</details>\n\n");
                    }
                    MessagePart::Context { items } => { out.push_str(&format!("_Context: {}_\n\n", items.iter().map(|i| format!("@{}", i.label)).collect::<Vec<_>>().join(", "))); }
                    MessagePart::Error { message } => { out.push_str(&format!("> ⚠️ {message}\n\n")); }
                    MessagePart::Plan { path, title } => { out.push_str(&format!("📋 Plan: [{title}]({path})\n\n")); }
                    MessagePart::Question { question, answer } => { out.push_str(&format!("❓ {}\n\n→ {}\n\n", question.question, answer.clone().unwrap_or_default())); }
                    MessagePart::Summary { text } => { out.push_str(&format!("_Summary:_ {text}\n\n")); }
                    MessagePart::Image { image } => { out.push_str(&format!("![{}](data:{};base64,…)\n\n", image.name, image.mime)); }
                    MessagePart::Checkpoint { .. } => {}
                }
            }
            out.push_str("---\n\n");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_matches_case_insensitively_and_trims_around_the_hit() {
        let text = format!("{} The NEEDLE sits here. {}", "a".repeat(300), "b".repeat(300));
        let snip = snippet_around(&text, "needle", 60).unwrap();
        assert!(snip.contains("NEEDLE"), "{snip}");
        assert!(snip.starts_with('…') && snip.ends_with('…'), "{snip}");
        assert!(snip.chars().count() <= 64, "{}", snip.chars().count());
        assert!(snippet_around(&text, "absent", 60).is_none());
        assert!(snippet_around(&text, "", 60).is_none());
    }

    #[test]
    fn snippet_handles_short_text_unicode_and_newlines() {
        assert_eq!(snippet_around("Fix the\nlogin bug", "LOGIN", 160).unwrap(), "Fix the login bug");
        assert_eq!(snippet_around("héllo wörld ÄPFEL kuchen", "äpfel", 160).unwrap(), "héllo wörld ÄPFEL kuchen");
        assert_eq!(find_ci("abc", "C"), Some((2, 3)));
    }
}

//! Per-workspace memories persisted at `~/.caret/memories/<workspace-hash>.json` ("global" without a workspace).
use crate::error::{CaretError, Result};
use crate::paths;
use crate::tokens;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryItem { pub id: String, pub text: String, pub created_at: u64, pub source: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub session_id: Option<String>, pub enabled: bool, #[serde(default, skip_serializing_if = "Option::is_none")] pub title: Option<String> }

pub const MAX_MEMORIES: usize = 200;
pub const MAX_PROMPT_TOKENS: usize = 2_000;

fn now_ms() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) }
fn base_dir() -> PathBuf { paths::config_dir().join("memories") }
pub fn store_path_in(base: &Path, root: Option<&Path>) -> PathBuf { base.join(format!("{}.json", root.map(paths::workspace_hash).unwrap_or_else(|| "global".into()))) }
pub fn store_path(root: Option<&Path>) -> PathBuf { store_path_in(&base_dir(), root) }

fn load(p: &Path) -> Vec<MemoryItem> { std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default() }
fn save(p: &Path, items: &[MemoryItem]) -> Result<()> {
    if let Some(d) = p.parent() { std::fs::create_dir_all(d)?; }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(items)?)?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}
fn norm(s: &str) -> String { s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase() }

pub fn list(root: Option<&Path>) -> Vec<MemoryItem> { list_at(&store_path(root)) }
pub fn list_at(p: &Path) -> Vec<MemoryItem> { load(p) }

pub fn add(root: Option<&Path>, text: &str, title: Option<&str>, source: &str, session_id: Option<&str>) -> Result<MemoryItem> { add_at(&store_path(root), text, title, source, session_id) }
pub fn add_at(p: &Path, text: &str, title: Option<&str>, source: &str, session_id: Option<&str>) -> Result<MemoryItem> {
    let text = text.trim();
    if text.is_empty() { return Err(CaretError::invalid("memory text is empty")); }
    let mut items = load(p);
    let key = norm(text);
    if let Some(existing) = items.iter_mut().find(|m| norm(&m.text) == key) {
        existing.enabled = true;
        if existing.title.is_none() { existing.title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()); }
        let out = existing.clone();
        save(p, &items)?;
        return Ok(out);
    }
    let item = MemoryItem { id: uuid::Uuid::new_v4().to_string(), text: text.to_string(), created_at: now_ms(), source: source.to_string(), session_id: session_id.map(String::from), enabled: true, title: title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) };
    items.push(item.clone());
    // cap: drop oldest disabled first, then oldest overall
    while items.len() > MAX_MEMORIES {
        if let Some(pos) = items.iter().position(|m| !m.enabled) { items.remove(pos); } else { items.remove(0); }
    }
    save(p, &items)?;
    Ok(item)
}

pub fn update(root: Option<&Path>, id: &str, text: &str) -> Result<()> { update_at(&store_path(root), id, text) }
pub fn update_at(p: &Path, id: &str, text: &str) -> Result<()> {
    let mut items = load(p);
    let m = items.iter_mut().find(|m| m.id == id).ok_or_else(|| CaretError::not_found(format!("Memory {id}")))?;
    if text.trim().is_empty() { return Err(CaretError::invalid("memory text is empty")); }
    m.text = text.trim().to_string();
    save(p, &items)
}
pub fn delete(root: Option<&Path>, id: &str) -> Result<()> { delete_at(&store_path(root), id) }
pub fn delete_at(p: &Path, id: &str) -> Result<()> {
    let mut items = load(p);
    let before = items.len();
    items.retain(|m| m.id != id);
    if items.len() == before { return Err(CaretError::not_found(format!("Memory {id}"))); }
    save(p, &items)
}
pub fn toggle(root: Option<&Path>, id: &str, enabled: bool) -> Result<()> { toggle_at(&store_path(root), id, enabled) }
pub fn toggle_at(p: &Path, id: &str, enabled: bool) -> Result<()> {
    let mut items = load(p);
    let m = items.iter_mut().find(|m| m.id == id).ok_or_else(|| CaretError::not_found(format!("Memory {id}")))?;
    m.enabled = enabled;
    save(p, &items)
}

/// Enabled memories rendered for the system prompt (None when empty), capped at ~2k tokens (newest kept).
pub fn prompt_block(root: Option<&Path>) -> Option<String> { prompt_block_at(&store_path(root)) }
pub fn prompt_block_at(p: &Path) -> Option<String> {
    let items: Vec<MemoryItem> = load(p).into_iter().filter(|m| m.enabled).collect();
    if items.is_empty() { return None; }
    let mut lines: Vec<String> = vec![];
    let mut used = 0usize;
    for m in items.iter().rev() {
        let line = match &m.title { Some(t) => format!("- {t}: {}", m.text), None => format!("- {}", m.text) };
        let t = tokens::estimate(&line);
        if used + t > MAX_PROMPT_TOKENS { break; }
        used += t;
        lines.push(line);
    }
    lines.reverse();
    Some(format!("## Memories (facts the user wanted remembered)\n{}", lines.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crud_dedupe_cap_and_prompt() {
        let tmp = tempfile::tempdir().unwrap(); let p = tmp.path().join("m.json");
        let a = add_at(&p, "Prefer pnpm over npm", Some("Package manager"), "agent", Some("s1")).unwrap();
        let b = add_at(&p, "  prefer   PNPM over npm ", None, "user", None).unwrap();
        assert_eq!(a.id, b.id, "near-identical text dedupes");
        assert_eq!(list_at(&p).len(), 1);
        assert!(add_at(&p, "   ", None, "user", None).is_err());
        let c = add_at(&p, "Tests live in tests/", None, "user", None).unwrap();
        toggle_at(&p, &c.id, false).unwrap();
        let blk = prompt_block_at(&p).unwrap();
        assert!(blk.contains("Package manager: Prefer pnpm over npm") && !blk.contains("Tests live"));
        update_at(&p, &c.id, "Tests live in spec/").unwrap();
        toggle_at(&p, &c.id, true).unwrap();
        assert!(prompt_block_at(&p).unwrap().contains("spec/"));
        delete_at(&p, &a.id).unwrap();
        assert!(delete_at(&p, &a.id).is_err());
        assert_eq!(list_at(&p).len(), 1);
        for i in 0..MAX_MEMORIES + 20 { add_at(&p, &format!("fact number {i}"), None, "agent", None).unwrap(); }
        assert_eq!(list_at(&p).len(), MAX_MEMORIES);
        assert!(prompt_block_at(&p).unwrap().len() < 12_000);
        assert!(prompt_block_at(&tmp.path().join("none.json")).is_none());
    }
}

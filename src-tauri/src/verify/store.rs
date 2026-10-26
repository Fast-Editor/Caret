//! Persistence for verification reports: ~/.caret/verify/<ws-hash>/<report-id>.json (+ index.json, keep 50).
use super::VerificationReport;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const KEEP: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexEntry { id: String, session_id: Option<String>, started_at: u64, status: String, risk_tier: u8 }

fn dir_for(root: &Path) -> PathBuf { let d = crate::paths::sub_dir("verify").join(crate::paths::workspace_hash(root)); let _ = std::fs::create_dir_all(&d); d }
fn index_path(dir: &Path) -> PathBuf { dir.join("index.json") }
fn read_index(dir: &Path) -> Vec<IndexEntry> { std::fs::read_to_string(index_path(dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default() }

pub fn save(r: &VerificationReport) -> Result<()> {
    let dir = dir_for(Path::new(&r.workspace_root));
    let tmp = dir.join(format!("{}.json.tmp", r.id));
    std::fs::write(&tmp, serde_json::to_vec(r)?)?;
    std::fs::rename(&tmp, dir.join(format!("{}.json", r.id)))?;
    let mut idx = read_index(&dir);
    idx.retain(|e| e.id != r.id);
    idx.push(IndexEntry { id: r.id.clone(), session_id: r.session_id.clone(), started_at: r.started_at, status: r.status.clone(), risk_tier: r.risk_tier });
    idx.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    while idx.len() > KEEP { if let Some(old) = idx.pop() { let _ = std::fs::remove_file(dir.join(format!("{}.json", old.id))); } }
    std::fs::write(index_path(&dir), serde_json::to_vec(&idx)?)?;
    Ok(())
}

pub fn load(root: Option<&Path>, id: &str) -> Option<VerificationReport> {
    let dir = dir_for(root?);
    if id.contains('/') || id.contains("..") { return None; }
    std::fs::read_to_string(dir.join(format!("{id}.json"))).ok().and_then(|s| serde_json::from_str(&s).ok())
}

pub fn list(root: Option<&Path>, session_id: Option<&str>, limit: usize) -> Vec<VerificationReport> {
    let Some(root) = root else { return vec![] };
    let dir = dir_for(root);
    read_index(&dir).into_iter().filter(|e| session_id.map(|s| e.session_id.as_deref() == Some(s)).unwrap_or(true)).take(limit).filter_map(|e| load(Some(root), &e.id)).collect()
}

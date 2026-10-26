//! File checkpoints: before the first mutation of a file in a turn its original content is stored at
//! ~/.caret/checkpoints/<ws-key>/<session>/<turn>/ so the user can restore any earlier point.
use crate::error::Result;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest { pub files: Vec<ManifestEntry> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry { pub path: String, pub existed: bool, pub blob: String }

pub fn turn_dir(ws_key: &str, session_id: &str, turn_id: &str) -> PathBuf { paths::sub_dir("checkpoints").join(ws_key).join(session_id).join(turn_id) }
fn manifest_path(dir: &Path) -> PathBuf { dir.join("manifest.json") }
pub fn read_manifest(dir: &Path) -> Manifest { std::fs::read_to_string(manifest_path(dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default() }
fn write_manifest(dir: &Path, m: &Manifest) -> Result<()> { std::fs::create_dir_all(dir)?; std::fs::write(manifest_path(dir), serde_json::to_vec(m)?)?; Ok(()) }

/// Store the pre-change content of `abs` (if not already stored in this turn). Returns true when newly stored.
pub fn snapshot(ws_key: &str, session_id: &str, turn_id: &str, abs: &Path, current: Option<&str>) -> Result<bool> {
    let dir = turn_dir(ws_key, session_id, turn_id);
    let mut m = read_manifest(&dir);
    let key = abs.to_string_lossy().to_string();
    if m.files.iter().any(|f| f.path == key) { return Ok(false); }
    let existed = abs.exists();
    let blob = format!("{}.bin", blake3::hash(key.as_bytes()).to_hex());
    std::fs::create_dir_all(&dir)?;
    if existed {
        match current { Some(c) => std::fs::write(dir.join(&blob), c)?, None => { if abs.is_file() { std::fs::copy(abs, dir.join(&blob))?; } } }
    }
    m.files.push(ManifestEntry { path: key, existed, blob });
    write_manifest(&dir, &m)?;
    Ok(true)
}

/// Restore every file recorded for a turn to its pre-turn state. Returns restored paths.
pub fn restore_turn(ws_key: &str, session_id: &str, turn_id: &str) -> Result<Vec<String>> {
    let dir = turn_dir(ws_key, session_id, turn_id);
    let m = read_manifest(&dir);
    let mut restored = vec![];
    for f in m.files.iter().rev() {
        let p = Path::new(&f.path);
        if f.existed {
            if let Ok(bytes) = std::fs::read(dir.join(&f.blob)) { if let Some(parent) = p.parent() { let _ = std::fs::create_dir_all(parent); } if std::fs::write(p, bytes).is_ok() { restored.push(f.path.clone()); } }
        } else if p.exists() {
            let ok = if p.is_dir() { std::fs::remove_dir_all(p).is_ok() } else { std::fs::remove_file(p).is_ok() };
            if ok { restored.push(f.path.clone()); }
        }
    }
    Ok(restored)
}

pub fn remove_session(ws_key: &str, session_id: &str) { let _ = std::fs::remove_dir_all(paths::sub_dir("checkpoints").join(ws_key).join(session_id)); }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_and_restore_round_trip() {
        crate::ai::agent::testutil::test_home();
        let ws = tempfile::tempdir().unwrap();
        let f = ws.path().join("a.txt"); std::fs::write(&f, "original").unwrap();
        let g = ws.path().join("new.txt");
        assert!(snapshot("ws", "s1", "t1", &f, None).unwrap());
        assert!(!snapshot("ws", "s1", "t1", &f, Some("changed-later")).unwrap());
        assert!(snapshot("ws", "s1", "t1", &g, None).unwrap());
        std::fs::write(&f, "modified").unwrap(); std::fs::write(&g, "created").unwrap();
        let restored = restore_turn("ws", "s1", "t1").unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "original");
        assert!(!g.exists());
    }
}

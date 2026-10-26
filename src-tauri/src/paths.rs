//! Path helpers: config dirs, workspace confinement, normalization.
use crate::error::{CaretError, Result};
use std::path::{Component, Path, PathBuf};

pub fn home_dir() -> PathBuf { dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")) }
/// `~/.caret`
pub fn config_dir() -> PathBuf {
    if let Ok(p) = std::env::var("CARET_HOME") { return PathBuf::from(p); }
    home_dir().join(".caret")
}
pub fn ensure_dir(p: &Path) -> Result<PathBuf> { std::fs::create_dir_all(p)?; Ok(p.to_path_buf()) }
pub fn sub_dir(name: &str) -> PathBuf { let p = config_dir().join(name); let _ = std::fs::create_dir_all(&p); p }

/// Lexically normalize (resolve `.`/`..`) without touching the filesystem.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => { out.pop(); }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolve `candidate` (absolute or workspace-relative) and confine it to `root`.
pub fn confine(root: &Path, candidate: &str) -> Result<PathBuf> {
    let raw = Path::new(candidate);
    let joined = if raw.is_absolute() { raw.to_path_buf() } else { root.join(raw) };
    let norm = normalize(&joined);
    let root_n = normalize(root);
    if !norm.starts_with(&root_n) {
        return Err(CaretError::denied(format!("Path escapes workspace: {}", candidate)));
    }
    Ok(norm)
}

/// Resolve absolute-or-relative path against root WITHOUT confinement (for user-driven UI ops).
pub fn resolve(root: Option<&Path>, candidate: &str) -> PathBuf {
    let raw = Path::new(candidate);
    if raw.is_absolute() { normalize(raw) } else if let Some(r) = root { normalize(&r.join(raw)) } else { normalize(raw) }
}

pub fn rel_to(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).map(|r| r.to_string_lossy().to_string()).unwrap_or_else(|_| p.to_string_lossy().to_string())
}

/// Stable short hash for a workspace path (used for per-workspace storage dirs).
pub fn workspace_hash(root: &Path) -> String {
    let h = blake3::hash(root.to_string_lossy().as_bytes());
    h.to_hex()[..16].to_string()
}

pub fn is_hidden(name: &str) -> bool { name.starts_with('.') }

//! Workspace file watcher: debounced notify events → `caret://fs/changed` / `caret://git/changed`,
//! plus incremental quick-open index updates.
use crate::core::workspace::WorkspaceInfo;
use crate::events;
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use parking_lot::Mutex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Listener, Manager};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsChangeEvent { pub kind: &'static str, pub paths: Vec<String> }

#[derive(Default)]
pub struct WatcherState { inner: Mutex<Option<Debouncer<notify::RecommendedWatcher, RecommendedCache>>> }

const MAX_PATHS: usize = 500;

/// Paths that never produce UI events (build outputs, VCS internals, our own data).
fn is_noise(root: &Path, p: &Path) -> bool {
    let rel = p.strip_prefix(root).unwrap_or(p);
    let mut comps = rel.components().map(|c| c.as_os_str().to_string_lossy());
    let first = comps.next().unwrap_or_default();
    if first == ".caret" {
        let second = comps.next().unwrap_or_default();
        return matches!(second.as_ref(), "index" | "checkpoints" | "logs" | "sessions");
    }
    rel.components().any(|c| { let s = c.as_os_str().to_string_lossy(); s == "node_modules" || s == "target" || s == ".git" })
}

/// `.git/HEAD`, `.git/index`, `.git/refs/**`, `.git/ORIG_HEAD`, `.git/MERGE_HEAD`, `.git/packed-refs` → git changed.
fn is_git_signal(root: &Path, p: &Path) -> bool {
    let rel = p.strip_prefix(root).unwrap_or(p);
    let s = rel.to_string_lossy();
    let Some(rest) = s.strip_prefix(".git/") else { return false };
    rest == "HEAD" || rest == "index" || rest == "ORIG_HEAD" || rest == "MERGE_HEAD" || rest == "FETCH_HEAD" || rest == "packed-refs" || rest.starts_with("refs/") || rest.starts_with("worktrees/")
}

fn kind_of(k: &notify::EventKind) -> &'static str {
    use notify::EventKind::*;
    match k { Create(_) => "create", Modify(notify::event::ModifyKind::Name(_)) => "rename", Modify(_) => "modify", Remove(_) => "remove", _ => "any" }
}

fn start(app: &AppHandle, root: &Path) -> notify::Result<Debouncer<notify::RecommendedWatcher, RecommendedCache>> {
    let h = app.clone();
    let root_b: PathBuf = root.to_path_buf();
    let mut deb = new_debouncer(Duration::from_millis(300), None, move |res: DebounceEventResult| {
        let events = match res { Ok(ev) => ev, Err(errs) => { for e in errs { tracing::debug!(error = %e, "watch error"); } return; } };
        let mut fs_paths: Vec<String> = Vec::new();
        let mut kinds: Vec<&'static str> = Vec::new();
        let mut git_changed = false;
        for ev in events {
            for p in &ev.paths {
                if is_git_signal(&root_b, p) { git_changed = true; continue; }
                if is_noise(&root_b, p) { continue; }
                let k = kind_of(&ev.kind);
                // keep the quick-open index in sync
                match k {
                    "create" => crate::core::fs::index_add(&h, p),
                    "remove" => crate::core::fs::index_remove(&h, p),
                    "rename" | "any" | "modify" => { if p.exists() { if p.is_file() { crate::core::fs::index_add(&h, p); } } else { crate::core::fs::index_remove(&h, p); } }
                    _ => {}
                }
                if fs_paths.len() < MAX_PATHS { fs_paths.push(p.to_string_lossy().to_string()); }
                kinds.push(k);
                // an in-workspace edit also affects git status (worktree dirty state)
                if !p.to_string_lossy().contains("/.caret/") { git_changed = true; }
            }
        }
        if !kinds.is_empty() {
            let first = kinds[0];
            let kind = if kinds.len() > MAX_PATHS || kinds.iter().any(|k| *k != first) { "any" } else { first };
            events::emit(&h, events::FS_CHANGED, FsChangeEvent { kind, paths: fs_paths });
        }
        if git_changed { events::emit(&h, events::GIT_CHANGED, serde_json::Value::Null); }
    })?;
    deb.watch(root, RecursiveMode::Recursive)?;
    tracing::info!(root = %root.display(), "file watcher started");
    Ok(deb)
}

pub fn init(app: &AppHandle) {
    let h = app.clone();
    app.listen(events::WORKSPACE_CHANGED, move |ev| {
        let info: Option<WorkspaceInfo> = serde_json::from_str(ev.payload()).ok().flatten();
        let st = h.state::<WatcherState>();
        let mut guard = st.inner.lock();
        *guard = None; // drop old watcher first
        if let Some(info) = info {
            match start(&h, Path::new(&info.root)) {
                Ok(d) => *guard = Some(d),
                Err(e) => { tracing::warn!(error = %e, "failed to start watcher"); events::notify(&h, "warn", "File watcher unavailable", Some(e.to_string())); }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn noise_and_git_signals() {
        let r = Path::new("/ws");
        assert!(is_noise(r, Path::new("/ws/node_modules/x/y.js")));
        assert!(is_noise(r, Path::new("/ws/.git/objects/ab/cd")));
        assert!(is_noise(r, Path::new("/ws/.caret/index/meta.json")));
        assert!(!is_noise(r, Path::new("/ws/.caret/rules/a.mdc")));
        assert!(!is_noise(r, Path::new("/ws/src/main.rs")));
        assert!(is_git_signal(r, Path::new("/ws/.git/HEAD")));
        assert!(is_git_signal(r, Path::new("/ws/.git/refs/heads/main")));
        assert!(!is_git_signal(r, Path::new("/ws/.git/objects/aa")));
    }
}

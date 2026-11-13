//! Workspace file watcher: debounced notify events → `caret://fs/changed` / `caret://git/changed`,
//! plus incremental quick-open index updates.
//!
//! The watcher is (re)started on a blocking thread when the workspace changes and uses `NoCache`: the debouncer's
//! default file-ID cache walks the whole tree on `watch()` (node_modules included), which took 7 s for a 600 MB
//! JavaScript checkout and blocked the open. Without the cache a rename arrives as two events (old path, new path)
//! instead of one paired event; the handler below treats every path by its current existence, so nothing is lost.
use crate::core::generation::Generation;
use crate::core::workspace::WorkspaceInfo;
use crate::events;
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer_opt, DebounceEventResult, Debouncer, NoCache};
use parking_lot::Mutex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Listener, Manager};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsChangeEvent { pub kind: &'static str, pub paths: Vec<String> }

type WorkspaceWatcher = Debouncer<notify::RecommendedWatcher, NoCache>;

#[derive(Default)]
pub struct WatcherState { inner: Mutex<Option<WorkspaceWatcher>>, generation: Generation }

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

fn start(app: &AppHandle, root: &Path) -> notify::Result<WorkspaceWatcher> {
    let h = app.clone();
    let root_b: PathBuf = root.to_path_buf();
    let mut deb = new_debouncer_opt::<_, notify::RecommendedWatcher, NoCache>(Duration::from_millis(300), None, move |res: DebounceEventResult| {
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
    }, NoCache::new(), notify::Config::default())?;
    deb.watch(root, RecursiveMode::Recursive)?;
    Ok(deb)
}

/// Swap the watcher for a new root (or none) off the emitting thread. A ticket from `WatcherState::generation` makes
/// a stale start (the user switched folders again meanwhile) discard its watcher instead of installing it.
fn restart(app: AppHandle, info: Option<WorkspaceInfo>) {
    let st = app.state::<WatcherState>();
    let ticket = st.generation.begin();
    // Detach the old watcher right away so no more events arrive for the previous root; its teardown
    // (stopping the debouncer thread, closing the OS stream) happens on the blocking thread below.
    let old = st.inner.lock().take();
    tauri::async_runtime::spawn_blocking(move || {
        let t0 = Instant::now();
        drop(old);
        let Some(info) = info else { return };
        let st = app.state::<WatcherState>();
        if !st.generation.is_current(ticket) { return; }
        match start(&app, Path::new(&info.root)) {
            Ok(d) => {
                let mut guard = st.inner.lock();
                if st.generation.is_current(ticket) {
                    *guard = Some(d);
                    tracing::info!(root = %info.root, ms = t0.elapsed().as_millis() as u64, "file watcher started");
                } else {
                    tracing::debug!(root = %info.root, "file watcher start superseded by a newer workspace");
                }
            }
            Err(e) => { tracing::warn!(error = %e, "failed to start watcher"); events::notify(&app, "warn", "File watcher unavailable", Some(e.to_string())); }
        }
    });
}

pub fn init(app: &AppHandle) {
    let h = app.clone();
    app.listen(events::WORKSPACE_CHANGED, move |ev| {
        let info: Option<WorkspaceInfo> = serde_json::from_str(ev.payload()).ok().flatten();
        restart(h.clone(), info);
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

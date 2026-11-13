//! Per-folder file watchers: debounced notify events → `caret://fs/changed` / `caret://git/changed` for the windows
//! showing that folder, plus incremental updates of that folder's quick-open index.
//!
//! A watcher starts when the first window opens a folder (`WorkspaceChange::first_for_root`) and stops when the last
//! window showing it leaves (`last_for_prev_root`); windows on the same folder share one. Each start runs on a
//! blocking thread and uses `NoCache`: the debouncer's default file-ID cache walks the whole tree on `watch()`
//! (node_modules included), which took 7 s for a 600 MB JavaScript checkout and blocked the open. Without the cache a
//! rename arrives as two events (old path, new path) instead of one paired event; the handler below treats every path
//! by its current existence, so nothing is lost.
use crate::core::generation::Generation;
use crate::events::{self, WorkspaceChange};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer_opt, DebounceEventResult, Debouncer, NoCache};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsChangeEvent { pub kind: &'static str, pub paths: Vec<String> }

type WorkspaceWatcher = Debouncer<notify::RecommendedWatcher, NoCache>;

/// One watcher per open folder, keyed by canonical root.
#[derive(Default)]
pub struct WatcherState {
    watchers: DashMap<PathBuf, WorkspaceWatcher>,
    /// One ticket counter per root: a start that finishes after a newer start or a stop for the same root discards
    /// its watcher instead of installing it (rapid close / reopen of one folder). Counters are never removed; a
    /// `u64` per folder ever opened is nothing.
    generations: DashMap<PathBuf, Arc<Generation>>,
}

impl WatcherState {
    fn generation(&self, root: &Path) -> Arc<Generation> { Arc::clone(&*self.generations.entry(root.to_path_buf()).or_default()) }
    pub fn is_watching(&self, root: &Path) -> bool { self.watchers.contains_key(root) }
    pub fn roots(&self) -> Vec<PathBuf> { self.watchers.iter().map(|e| e.key().clone()).collect() }
}

/// What a workspace change means for a per-root resource: stop the previous folder's when no window shows it any
/// more, start the new folder's when this window is the first to show it. Pure; the quick-open index uses it too.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RootPlan { pub stop: Option<PathBuf>, pub start: Option<PathBuf> }

pub fn root_plan(change: &WorkspaceChange) -> RootPlan {
    RootPlan {
        stop: if change.last_for_prev_root { change.prev_root.as_deref().map(PathBuf::from) } else { None },
        start: if change.first_for_root { change.info.as_ref().map(|i| PathBuf::from(&i.root)) } else { None },
    }
}

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

/// Build and arm the watcher for `root`. Its events go to the windows showing `root` and to Rust listeners
/// (`emit_for_root`), and keep `root`'s quick-open index in sync.
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
                // keep this folder's quick-open index in sync
                match k {
                    "create" => crate::core::fs::index_add(&h, &root_b, p),
                    "remove" => crate::core::fs::index_remove(&h, &root_b, p),
                    "rename" | "any" | "modify" => { if p.exists() { if p.is_file() { crate::core::fs::index_add(&h, &root_b, p); } } else { crate::core::fs::index_remove(&h, &root_b, p); } }
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
            events::emit_for_root(&h, &root_b, events::FS_CHANGED, FsChangeEvent { kind, paths: fs_paths });
        }
        if git_changed { events::emit_for_root(&h, &root_b, events::GIT_CHANGED, serde_json::Value::Null); }
    }, NoCache::new(), notify::Config::default())?;
    deb.watch(root, RecursiveMode::Recursive)?;
    Ok(deb)
}

/// Start watching `root` off the emitting thread. A ticket from the root's generation makes a stale start (the folder
/// was closed, or closed and reopened, meanwhile) discard its watcher instead of installing it.
fn start_root(app: &AppHandle, root: PathBuf) {
    let generation = app.state::<WatcherState>().generation(&root);
    let ticket = generation.begin();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let t0 = Instant::now();
        if !generation.is_current(ticket) { return; }
        match start(&app, &root) {
            Ok(d) => {
                let st = app.state::<WatcherState>();
                // Check and install under the entry lock, so a concurrent stop (bump + remove, below) cannot slip
                // between the check and the insert and leave a watcher behind for a closed folder.
                let installed = match st.watchers.entry(root.clone()) {
                    _ if !generation.is_current(ticket) => false,
                    Entry::Vacant(v) => { v.insert(d); true }
                    Entry::Occupied(mut o) => { let old = o.insert(d); drop(o); drop(old); true }
                };
                if installed { tracing::info!(root = %root.display(), ms = t0.elapsed().as_millis() as u64, "file watcher started"); }
                else { tracing::debug!(root = %root.display(), "file watcher start superseded: folder closed or reopened"); }
            }
            Err(e) => { tracing::warn!(root = %root.display(), error = %e, "failed to start watcher"); events::notify(&app, "warn", "File watcher unavailable", Some(e.to_string())); }
        }
    });
}

/// Stop watching `root`: detach the watcher right away so no more events arrive for it and invalidate any start still
/// in flight; its teardown (stopping the debouncer thread, closing the OS stream) happens on a blocking thread.
fn stop_root(app: &AppHandle, root: &Path) {
    let st = app.state::<WatcherState>();
    let old = {
        let entry = st.watchers.entry(root.to_path_buf());
        st.generation(root).begin();
        match entry { Entry::Occupied(o) => Some(o.remove()), Entry::Vacant(_) => None }
    };
    let root = root.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let t0 = Instant::now();
        let had = old.is_some();
        drop(old);
        if had { tracing::info!(root = %root.display(), ms = t0.elapsed().as_millis() as u64, "file watcher stopped"); }
    });
}

/// Per-folder lifecycle: start on the first window that opens a folder, stop when the last window showing it leaves.
pub fn init(app: &AppHandle) {
    events::on_workspace_change(app, |app, change| {
        let plan = root_plan(&change);
        if let Some(root) = plan.stop { stop_root(app, &root); }
        if let Some(root) = plan.start { start_root(app, root); }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::workspace::WorkspaceInfo;

    fn change(label: &str, root: Option<&str>, prev: Option<&str>, first: bool, last: bool) -> WorkspaceChange {
        let info = root.map(|r| WorkspaceInfo { root: r.into(), name: "x".into(), is_git_repo: false, has_caret_dir: false, has_cursor_dir: false });
        WorkspaceChange { label: label.into(), info, prev_root: prev.map(String::from), first_for_root: first, last_for_prev_root: last }
    }
    fn p(s: &str) -> Option<PathBuf> { Some(PathBuf::from(s)) }

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

    #[test]
    fn root_plan_starts_and_stops_from_the_change_flags() {
        assert_eq!(root_plan(&change("main", Some("/a"), None, true, false)), RootPlan { stop: None, start: p("/a") }, "first window on a folder");
        assert_eq!(root_plan(&change("w2", Some("/a"), None, false, false)), RootPlan::default(), "a second window on the same folder shares the watcher");
        assert_eq!(root_plan(&change("main", Some("/b"), Some("/a"), true, true)), RootPlan { stop: p("/a"), start: p("/b") }, "switching folders, alone on both");
        assert_eq!(root_plan(&change("main", Some("/b"), Some("/a"), true, false)), RootPlan { stop: None, start: p("/b") }, "another window still shows /a");
        assert_eq!(root_plan(&change("main", Some("/b"), Some("/a"), false, true)), RootPlan { stop: p("/a"), start: None }, "/b is already watched for another window");
        assert_eq!(root_plan(&change("w2", None, Some("/a"), false, true)), RootPlan { stop: p("/a"), start: None }, "the last window on /a closed it (or was destroyed)");
        assert_eq!(root_plan(&change("w2", None, Some("/a"), false, false)), RootPlan::default(), "closed, but another window still shows /a");
        assert_eq!(root_plan(&change("w3", None, None, false, false)), RootPlan::default(), "an empty window closed");
        assert_eq!(root_plan(&change("w3", Some("/a"), Some("/a"), false, false)), RootPlan::default(), "the same folder re-opened in place: nothing to do");
    }

    #[test]
    fn watcher_state_keeps_one_generation_per_root() {
        let st = WatcherState::default();
        let a = st.generation(Path::new("/a"));
        let b = st.generation(Path::new("/b"));
        assert!(Arc::ptr_eq(&a, &st.generation(Path::new("/a"))), "the same folder gets the same counter");
        let ticket = a.begin();
        b.begin(); b.begin();
        assert!(a.is_current(ticket), "another folder's opens do not invalidate this folder's start");
        a.begin();
        assert!(!a.is_current(ticket), "a newer start or a stop for the same folder does");
        assert!(!st.is_watching(Path::new("/a")) && st.roots().is_empty(), "no watcher is installed until a start completes");
    }
}

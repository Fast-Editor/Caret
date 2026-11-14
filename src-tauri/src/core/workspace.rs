//! Workspace (open folder) state per window, recent list, and `caret://workspace/changed` events.
//!
//! Which folder a window shows lives in [`WindowRegistry`](crate::core::windows::WindowRegistry); `WorkspaceState`
//! is a facade over it that keeps the old call sites compiling (see docs/MULTI_WINDOW.md).
use crate::core::settings::SettingsState;
use crate::core::windows::{RootRefs, WindowRegistry, MAIN_LABEL};
use crate::error::{CaretError, Result};
use crate::events::{self, WorkspaceChange};
use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo { pub root: String, pub name: String, pub is_git_repo: bool, pub has_caret_dir: bool, pub has_cursor_dir: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentWorkspace { pub path: String, pub name: String, pub last_opened: u64, pub exists: bool }

/// Facade over the window registry. Holds no root itself: `root_for(label)` is the per-window accessor; `root()` /
/// `require_root()` resolve the *focused* window and exist only so unmigrated call sites keep working (every command
/// that depends on the workspace should take `window: tauri::Window` and use `window.label()`).
#[derive(Default)]
pub struct WorkspaceState { app: OnceLock<AppHandle> }

impl WorkspaceState {
    /// Called once at setup; before that every accessor answers `None`.
    pub fn attach(&self, app: &AppHandle) { let _ = self.app.set(app.clone()); }
    fn registry(&self) -> Option<State<'_, WindowRegistry>> { self.app.get().map(|a| a.state::<WindowRegistry>()) }

    pub fn root_for(&self, label: &str) -> Option<PathBuf> { self.registry()?.root_of(label) }
    pub fn require_root_for(&self, label: &str) -> Result<PathBuf> { self.root_for(label).ok_or_else(CaretError::no_workspace) }
    pub fn info_for_label(&self, label: &str) -> Option<WorkspaceInfo> { self.root_for(label).map(|r| info_for(&r)) }

    /// The focused window's root. When no focused window is known: `main`'s root, then the only open root if exactly
    /// one. Unmigrated call sites only; logged at trace level so phase-1 migrations can be spotted in a trace run.
    pub fn root(&self) -> Option<PathBuf> {
        let reg = self.registry()?;
        let root = focused_root(&reg);
        tracing::trace!(root = ?root, "WorkspaceState::root(): implicit focused-window lookup");
        root
    }
    pub fn require_root(&self) -> Result<PathBuf> { self.root().ok_or_else(CaretError::no_workspace) }
    pub fn info(&self) -> Option<WorkspaceInfo> { self.root().map(|r| info_for(&r)) }
}

fn focused_root(reg: &WindowRegistry) -> Option<PathBuf> {
    if let Some(label) = reg.focused() { if reg.contains(&label) { return reg.root_of(&label); } }
    if let Some(r) = reg.root_of(MAIN_LABEL) { return Some(r); }
    let roots = reg.roots();
    if roots.len() == 1 { roots.into_iter().next() } else { None }
}

/// True when changes in `label` are what the single-root Rust listeners should follow: it is the focused window, or
/// the only one.
fn drives_legacy_listeners(reg: &WindowRegistry, label: &str) -> bool {
    match reg.focused() { Some(f) => f == label, None => reg.len() <= 1 || label == MAIN_LABEL }
}

pub fn info_for(root: &Path) -> WorkspaceInfo {
    WorkspaceInfo {
        root: root.to_string_lossy().to_string(),
        name: root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| root.to_string_lossy().to_string()),
        is_git_repo: root.join(".git").exists(),
        has_caret_dir: root.join(".caret").is_dir(),
        has_cursor_dir: root.join(".cursor").is_dir(),
    }
}

fn recent_path() -> PathBuf { paths::config_dir().join("recent.json") }
fn now() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) }

fn load_recent() -> Vec<RecentWorkspace> {
    let mut v: Vec<RecentWorkspace> = std::fs::read_to_string(recent_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    for r in &mut v { r.exists = Path::new(&r.path).is_dir(); }
    v
}
fn save_recent(v: &[RecentWorkspace]) { let _ = std::fs::create_dir_all(paths::config_dir()); let _ = std::fs::write(recent_path(), serde_json::to_string_pretty(v).unwrap_or_default()); }
fn touch_recent(root: &Path) {
    let mut v = load_recent();
    let p = root.to_string_lossy().to_string();
    v.retain(|r| r.path != p);
    v.insert(0, RecentWorkspace { path: p, name: root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), last_opened: now(), exists: true });
    v.truncate(30);
    save_recent(&v);
}

/// Open `path` in window `label`: registry root, per-root reference counts, settings scope (focused window only),
/// recent list, window title, then the events. Other domains react to the events.
///
/// Events, in order: the Rust-only `INTERNAL_WORKSPACE_CHANGED` (`WorkspaceChange`, every change of every window),
/// then `WORKSPACE_CHANGED` with `Option<WorkspaceInfo>` for the frontend of that window.
///
/// `WORKSPACE_CHANGED` goes to that window only (`emit_to`); an app-wide emit would switch every other window's
/// frontend to this folder. Rust-side consumers use `events::on_workspace_change` and never see this event. The
/// focused window additionally drives the settings workspace scope (`drives_legacy_listeners`, kept for that).
///
/// The `WORKSPACE_CHANGED` listeners run inline inside `emit`, so each of them must hand its real work (watcher
/// setup, index builds, server restarts) to a background task; the `emit_ms` figure in the log is how well they do.
pub fn open_workspace_for(app: &AppHandle, label: &str, path: &Path) -> Result<WorkspaceInfo> {
    let t0 = Instant::now();
    let root = std::fs::canonicalize(path).map_err(|_| CaretError::not_found(format!("Folder {}", path.display())))?;
    if !root.is_dir() { return Err(CaretError::invalid(format!("{} is not a directory", root.display()))); }
    let registry = app.state::<WindowRegistry>();
    if !registry.contains(label) { return Err(CaretError::not_found(format!("Window {label}"))); }
    let prev_root = registry.set_root(label, Some(root.clone()));
    let refs = app.state::<RootRefs>();
    let (first_for_root, last_for_prev_root) = if prev_root.as_deref() == Some(root.as_path()) { (false, false) } else {
        let last = prev_root.as_deref().map(|p| refs.release(p)).unwrap_or(false);
        (refs.acquire(&root), last)
    };
    let legacy = drives_legacy_listeners(&registry, label);
    // Settings are scoped to one workspace per process until phase 1 makes them per root: follow the focused window.
    if legacy { app.state::<SettingsState>().set_workspace(Some(&root)); }
    let t_settings = t0.elapsed();
    touch_recent(&root);
    let t_recent = t0.elapsed();
    let info = info_for(&root);
    if let Some(w) = app.get_webview_window(label) { let _ = w.set_title(&crate::core::windows::window_title(Some(&root))); }
    let t_emit = Instant::now();
    events::emit_internal_workspace_changed(app, WorkspaceChange {
        label: label.to_string(), info: Some(info.clone()), prev_root: prev_root.as_ref().map(|p| p.to_string_lossy().to_string()), first_for_root, last_for_prev_root,
    });
    events::emit_to(app, label, events::WORKSPACE_CHANGED, Some(info.clone()));
    crate::menu::rebuild_window_items(app);
    let ms = |d: std::time::Duration| d.as_millis() as u64;
    tracing::info!(label, root = %root.display(), first_for_root, focused = legacy, settings_ms = ms(t_settings), recent_ms = ms(t_recent - t_settings), emit_ms = ms(t_emit.elapsed()), total_ms = ms(t0.elapsed()), "workspace opened");
    Ok(info)
}

/// Close the folder in window `label` (the window stays open and shows Welcome).
pub fn close_workspace_for(app: &AppHandle, label: &str) { close_workspace_inner(app, label, true); }

/// Resource-release half of `close_workspace_for` for a window that is being destroyed: nothing is sent to its
/// webview and the Window menu is left to the caller (which also unregisters the label).
pub fn release_workspace_for_destroyed(app: &AppHandle, label: &str) { close_workspace_inner(app, label, false); }

fn close_workspace_inner(app: &AppHandle, label: &str, window_alive: bool) {
    let registry = app.state::<WindowRegistry>();
    if !registry.contains(label) { return; }
    let prev_root = registry.set_root(label, None);
    let last_for_prev_root = prev_root.as_deref().map(|p| app.state::<RootRefs>().release(p)).unwrap_or(false);
    let legacy = drives_legacy_listeners(&registry, label);
    if legacy { app.state::<SettingsState>().set_workspace(None); }
    if window_alive { if let Some(w) = app.get_webview_window(label) { let _ = w.set_title(&crate::core::windows::window_title(None)); } }
    if prev_root.is_none() && !window_alive { return; }
    events::emit_internal_workspace_changed(app, WorkspaceChange {
        label: label.to_string(), info: None, prev_root: prev_root.as_ref().map(|p| p.to_string_lossy().to_string()), first_for_root: false, last_for_prev_root,
    });
    // See `open_workspace_for` for why the focused window's change is still broadcast app-wide.
    if window_alive { events::emit_to::<Option<WorkspaceInfo>>(app, label, events::WORKSPACE_CHANGED, None); }
    if window_alive { crate::menu::rebuild_window_items(app); }
    tracing::info!(label, prev_root = ?prev_root, last_for_prev_root, focused = legacy, window_alive, "workspace closed");
}

/// Open `path` in the focused window (falls back to `main`). Thin wrapper for callers that have no window yet (CLI
/// routing until phase 1); new code should call `open_workspace_for`.
pub fn open_workspace(app: &AppHandle, path: &Path) -> Result<WorkspaceInfo> {
    let label = crate::core::windows::focused_label(app).unwrap_or_else(|| MAIN_LABEL.to_string());
    open_workspace_for(app, &label, path)
}
/// Close the folder in the focused window (see `open_workspace`).
pub fn close_workspace(app: &AppHandle) {
    let label = crate::core::windows::focused_label(app).unwrap_or_else(|| MAIN_LABEL.to_string());
    close_workspace_for(app, &label);
}

// `async` commands run on the blocking pool instead of the main thread: everything here touches the disk (canonicalize,
// settings and recent-list files), and a plain command would stall every other IPC call for as long as it runs.
#[tauri::command(async)]
pub fn workspace_open(app: AppHandle, window: tauri::Window, path: String) -> Result<WorkspaceInfo> { open_workspace_for(&app, window.label(), Path::new(&path)) }
#[tauri::command(async)]
pub fn workspace_current(ws: State<'_, WorkspaceState>, window: tauri::Window) -> Result<Option<WorkspaceInfo>> { Ok(ws.info_for_label(window.label())) }
#[tauri::command(async)]
pub fn workspace_close(app: AppHandle, window: tauri::Window) -> Result<()> { close_workspace_for(&app, window.label()); Ok(()) }
#[tauri::command(async)]
pub fn workspace_recent() -> Result<Vec<RecentWorkspace>> { Ok(load_recent()) }
#[tauri::command(async)]
pub fn workspace_remove_recent(path: String) -> Result<()> { let mut v = load_recent(); v.retain(|r| r.path != path); save_recent(&v); Ok(()) }
#[tauri::command(async)]
pub fn workspace_ensure_project_dir(ws: State<'_, WorkspaceState>, window: tauri::Window) -> Result<String> {
    let root = ws.require_root_for(window.label())?;
    let dir = root.join(".caret");
    std::fs::create_dir_all(dir.join("rules"))?;
    Ok(dir.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_root_prefers_focused_then_main_then_only_root() {
        let r = WindowRegistry::default();
        assert_eq!(focused_root(&r), None);
        r.register("main"); r.register("w2");
        r.set_root("w2", Some(PathBuf::from("/b")));
        assert_eq!(focused_root(&r), Some(PathBuf::from("/b")), "only one root open");
        r.set_root("main", Some(PathBuf::from("/a")));
        assert_eq!(focused_root(&r), Some(PathBuf::from("/a")), "no focus known: main");
        r.set_focused("w2");
        assert_eq!(focused_root(&r), Some(PathBuf::from("/b")), "focused window wins");
        r.set_root("w2", None);
        assert_eq!(focused_root(&r), None, "a focused empty window has no root, even when main has one");
        r.unregister("w2");
        assert_eq!(focused_root(&r), Some(PathBuf::from("/a")));
    }

    #[test]
    fn legacy_listeners_follow_the_focused_or_only_window() {
        let r = WindowRegistry::default();
        r.register("main");
        assert!(drives_legacy_listeners(&r, "main"), "single window, no focus event yet");
        r.register("w2");
        assert!(drives_legacy_listeners(&r, "main"), "no focus known: main drives");
        assert!(!drives_legacy_listeners(&r, "w2"));
        r.set_focused("w2");
        assert!(drives_legacy_listeners(&r, "w2"));
        assert!(!drives_legacy_listeners(&r, "main"));
    }
}

//! Window registry (which folder each window shows, which window is focused), per-root reference counts, window
//! creation and the `window_*` commands. See docs/MULTI_WINDOW.md.
//!
//! Labels: the window from `tauri.conf.json` is `main`; windows created here are `w2`, `w3`, … (capability `w*`).
//! Browser tool windows (`browser-*`) and other helper windows are not registered and never count as Caret windows.
use crate::core::workspace;
use crate::error::{CaretError, Result};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;
use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};

pub const MAIN_LABEL: &str = "main";
/// Cascade offset (logical px) between a new window and the focused one.
const CASCADE_OFFSET: f64 = 28.0;

#[derive(Debug, Clone)]
pub struct WindowEntry { pub label: String, pub root: Option<PathBuf>, pub created_at: Instant }

/// Managed state: one entry per Caret window. Every method takes `&self` and is safe from any thread.
#[derive(Default)]
pub struct WindowRegistry { entries: DashMap<String, WindowEntry>, focused: RwLock<Option<String>>, next_id: AtomicU32 }

impl WindowRegistry {
    /// Register a window (idempotent). `main` is registered at setup; `create_window` registers before `build()`.
    pub fn register(&self, label: &str) {
        self.entries.entry(label.to_string()).or_insert_with(|| WindowEntry { label: label.to_string(), root: None, created_at: Instant::now() });
    }
    /// Remove a window (on `WindowEvent::Destroyed`); returns the root it had. Clears the focus if it was focused.
    pub fn unregister(&self, label: &str) -> Option<PathBuf> {
        let root = self.entries.remove(label).and_then(|(_, e)| e.root);
        let mut f = self.focused.write();
        if f.as_deref() == Some(label) { *f = None; }
        root
    }
    pub fn contains(&self, label: &str) -> bool { self.entries.contains_key(label) }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    /// Set (or clear) the folder a window shows; returns the previous root. Unknown labels are ignored (`None`).
    pub fn set_root(&self, label: &str, root: Option<PathBuf>) -> Option<PathBuf> {
        let mut e = self.entries.get_mut(label)?;
        std::mem::replace(&mut e.root, root)
    }
    pub fn root_of(&self, label: &str) -> Option<PathBuf> { self.entries.get(label)?.root.clone() }
    pub fn entry(&self, label: &str) -> Option<WindowEntry> { self.entries.get(label).map(|e| e.clone()) }
    /// All entries in creation order (the order the Window menu lists them).
    pub fn entries(&self) -> Vec<WindowEntry> {
        let mut v: Vec<WindowEntry> = self.entries.iter().map(|e| e.value().clone()).collect();
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.label.cmp(&b.label)));
        v
    }
    pub fn labels(&self) -> Vec<String> { self.entries().into_iter().map(|e| e.label).collect() }
    /// Windows showing exactly this folder (creation order).
    pub fn labels_for_root(&self, root: &Path) -> Vec<String> {
        let entries = self.entries();
        select_labels_for_root(entries.iter().map(|e| (e.label.as_str(), e.root.as_deref())), root)
    }
    /// Distinct roots currently shown by at least one window.
    pub fn roots(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = Vec::new();
        for e in self.entries() { if let Some(r) = e.root { if !v.contains(&r) { v.push(r); } } }
        v
    }
    /// A window whose root is `path` or an ancestor of it; the deepest root wins, then the focused window, then the
    /// oldest window.
    pub fn window_containing(&self, path: &Path) -> Option<String> {
        let focused = self.focused();
        select_window_containing(&self.entries(), focused.as_deref(), path)
    }
    /// Remember the focused window. Labels that are not registered (browser windows, …) are ignored.
    pub fn set_focused(&self, label: &str) {
        if self.entries.contains_key(label) { *self.focused.write() = Some(label.to_string()); }
    }
    pub fn focused(&self) -> Option<String> { self.focused.read().clone() }
    /// Next free label: `w2`, `w3`, … (skips labels that are somehow still registered).
    pub fn next_label(&self) -> String {
        loop {
            let n = self.next_id.fetch_add(1, Ordering::SeqCst) + 2;
            let label = format!("w{n}");
            if !self.entries.contains_key(&label) { return label; }
        }
    }
    /// True when `label` shows a folder that no other window shows (its per-root resources can stop with it).
    /// A window without a folder is never "the last window for its root".
    pub fn is_last_window_for_root(&self, label: &str) -> bool {
        let Some(root) = self.root_of(label) else { return false };
        !self.entries.iter().any(|e| e.key() != label && e.root.as_deref() == Some(root.as_path()))
    }
}

/// Pure label selection behind `labels_for_root` / `events::emit_for_root`: entries whose root equals `root`
/// (component-wise, so a trailing slash does not matter), in the order given.
pub fn select_labels_for_root<'a>(entries: impl IntoIterator<Item = (&'a str, Option<&'a Path>)>, root: &Path) -> Vec<String> {
    entries.into_iter().filter(|(_, r)| *r == Some(root)).map(|(l, _)| l.to_string()).collect()
}

/// Pure selection behind `window_containing`: deepest matching root, then the focused window, then the oldest.
pub fn select_window_containing(entries: &[WindowEntry], focused: Option<&str>, path: &Path) -> Option<String> {
    entries.iter()
        .filter_map(|e| e.root.as_deref().filter(|r| path.starts_with(r)).map(|r| (r.components().count(), e)))
        .max_by(|(da, a), (db, b)| {
            da.cmp(db)
                .then_with(|| (focused == Some(a.label.as_str())).cmp(&(focused == Some(b.label.as_str()))))
                .then_with(|| b.created_at.cmp(&a.created_at))
        })
        .map(|(_, e)| e.label.clone())
}

/// Reference count of windows per root. Per-root resources (watcher, indexes, LSP, MCP, worktrees) start when
/// `acquire` returns `true` (first window on that folder) and stop when `release` returns `true` (last one left).
#[derive(Default)]
pub struct RootRefs { counts: DashMap<PathBuf, usize> }

impl RootRefs {
    /// Count one more window on `root`; `true` when it is the first.
    pub fn acquire(&self, root: &Path) -> bool {
        let mut e = self.counts.entry(root.to_path_buf()).or_insert(0);
        *e += 1;
        *e == 1
    }
    /// Count one window less on `root`; `true` when it was the last (the entry is dropped). Unknown roots → `false`.
    pub fn release(&self, root: &Path) -> bool {
        use dashmap::mapref::entry::Entry;
        match self.counts.entry(root.to_path_buf()) {
            Entry::Occupied(mut o) => { if *o.get() > 1 { *o.get_mut() -= 1; false } else { o.remove(); true } }
            Entry::Vacant(_) => false,
        }
    }
    pub fn count(&self, root: &Path) -> usize { self.counts.get(root).map(|c| *c).unwrap_or(0) }
    pub fn roots(&self) -> Vec<PathBuf> { self.counts.iter().map(|e| e.key().clone()).collect() }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSummary { pub label: String, pub root: Option<String>, pub name: Option<String>, pub focused: bool }

/// One summary per registered window, creation order (what `window_list` and the Window menu show).
pub fn summaries(registry: &WindowRegistry) -> Vec<WindowSummary> {
    let focused = registry.focused();
    registry.entries().into_iter().map(|e| WindowSummary {
        focused: focused.as_deref() == Some(e.label.as_str()),
        root: e.root.as_ref().map(|r| r.to_string_lossy().to_string()),
        name: e.root.as_ref().map(|r| workspace::info_for(r).name),
        label: e.label,
    }).collect()
}

/// The focused Caret window, falling back to `main`, then to the oldest registered window. `None` only when no
/// Caret window exists (macOS after the last one closed).
pub fn focused_window(app: &AppHandle) -> Option<WebviewWindow> {
    let registry = app.state::<WindowRegistry>();
    if let Some(w) = registry.focused().and_then(|l| app.get_webview_window(&l)) { return Some(w); }
    if let Some(w) = app.get_webview_window(MAIN_LABEL) { return Some(w); }
    registry.labels().into_iter().find_map(|l| app.get_webview_window(&l))
}
pub fn focused_label(app: &AppHandle) -> Option<String> { focused_window(app).map(|w| w.label().to_string()) }

pub fn window_title(root: Option<&Path>) -> String {
    match root.map(workspace::info_for) { Some(info) => format!("{} — Caret", info.name), None => "Caret".to_string() }
}

fn cascade_origin(app: &AppHandle) -> Option<(f64, f64)> {
    let w = focused_window(app)?;
    let pos = w.outer_position().ok()?;
    let scale = w.scale_factor().unwrap_or(1.0);
    let logical = pos.to_logical::<f64>(scale);
    Some((logical.x + CASCADE_OFFSET, logical.y + CASCADE_OFFSET))
}

/// Create a Caret window with the same options as the `main` entry in `tauri.conf.json`, cascaded from the focused
/// window, registered before it is built. With `path`, the folder is opened right away so `workspace_current`
/// already answers when the window's frontend boots. Returns the new label.
pub fn create_window(app: &AppHandle, path: Option<PathBuf>) -> Result<String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    let registry = app.state::<WindowRegistry>();
    let label = registry.next_label();
    let t0 = Instant::now();
    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(window_title(path.as_deref()))
        .inner_size(1440.0, 920.0)
        .min_inner_size(800.0, 500.0)
        .resizable(true)
        .decorations(true)
        .theme(Some(tauri::Theme::Dark))
        .background_color(tauri::window::Color(0x16, 0x16, 0x18, 0xff))
        .disable_drag_drop_handler();
    #[cfg(target_os = "macos")]
    { builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true); }
    if let Some((x, y)) = cascade_origin(app) { builder = builder.position(x, y); }
    registry.register(&label);
    let window = match builder.build() {
        Ok(w) => w,
        Err(e) => { registry.unregister(&label); return Err(CaretError::internal(format!("could not create window: {e}"))); }
    };
    crate::menu::rebuild_window_items(app);
    if let Some(p) = path {
        if let Err(e) = workspace::open_workspace_for(app, &label, &p) {
            tracing::warn!(label, path = %p.display(), error = %e.message, "new window: folder did not open");
        }
    }
    let _ = window.set_focus();
    tracing::info!(label, ms = t0.elapsed().as_millis() as u64, "window created");
    Ok(label)
}

/// Bring a Caret window to the front (unminimising it if needed).
pub fn focus_window(app: &AppHandle, label: &str) -> Result<()> {
    let w = app.get_webview_window(label).ok_or_else(|| CaretError::not_found(format!("window {label}")))?;
    let _ = w.unminimize();
    let _ = w.show();
    w.set_focus().map_err(|e| CaretError::internal(format!("focus window {label}: {e}")))
}

/// `Builder::on_window_event` handler: tracks focus, releases a destroyed window's root, keeps the Window menu
/// current. Windows that are not registered (browser tool windows, …) are ignored.
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    let app = window.app_handle();
    let label = window.label();
    let registry = app.state::<WindowRegistry>();
    match event {
        WindowEvent::Focused(true) => {
            if !registry.contains(label) { return; }
            let changed = registry.focused().as_deref() != Some(label);
            registry.set_focused(label);
            if changed { crate::menu::rebuild_window_items(app); }
        }
        WindowEvent::Destroyed => {
            if !registry.contains(label) { return; }
            // Same resource release as `workspace_close`, without touching the webview that no longer exists.
            workspace::release_workspace_for_destroyed(app, label);
            registry.unregister(label);
            crate::menu::rebuild_window_items(app);
            let remaining = registry.len();
            tracing::info!(label, remaining, "window destroyed");
            // macOS keeps the app alive without windows (see the ExitRequested handling in `run`); elsewhere quit.
            #[cfg(not(target_os = "macos"))]
            if remaining == 0 { app.exit(0); }
        }
        _ => {}
    }
}

// ───────────────────────────── commands ─────────────────────────────

#[tauri::command(async)]
pub fn window_new(app: AppHandle, path: Option<String>) -> Result<String> { create_window(&app, path.map(PathBuf::from)) }
#[tauri::command(async)]
pub fn window_list(app: AppHandle) -> Result<Vec<WindowSummary>> { Ok(summaries(&app.state::<WindowRegistry>())) }
#[tauri::command(async)]
pub fn window_focus(app: AppHandle, label: String) -> Result<()> { focus_window(&app, &label) }
#[tauri::command(async)]
pub fn window_close(app: AppHandle, label: String) -> Result<()> {
    let w = app.get_webview_window(&label).ok_or_else(|| CaretError::not_found(format!("window {label}")))?;
    w.close().map_err(|e| CaretError::internal(format!("close window {label}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg(labels: &[(&str, Option<&str>)]) -> WindowRegistry {
        let r = WindowRegistry::default();
        for (l, root) in labels { r.register(l); r.set_root(l, root.map(PathBuf::from)); std::thread::sleep(std::time::Duration::from_millis(1)); }
        r
    }

    #[test]
    fn register_and_unregister_round_trip() {
        let r = WindowRegistry::default();
        assert!(r.is_empty());
        r.register("main");
        r.register("main"); // idempotent
        assert_eq!(r.labels(), vec!["main"]);
        assert!(r.contains("main"));
        assert_eq!(r.root_of("main"), None);
        r.set_focused("main");
        assert_eq!(r.focused().as_deref(), Some("main"));
        r.set_root("main", Some(PathBuf::from("/tmp/a")));
        assert_eq!(r.unregister("main"), Some(PathBuf::from("/tmp/a")));
        assert!(r.is_empty());
        assert_eq!(r.focused(), None, "unregistering the focused window clears the focus");
        assert_eq!(r.unregister("main"), None);
    }

    #[test]
    fn set_root_returns_previous_root_and_ignores_unknown_labels() {
        let r = WindowRegistry::default();
        assert_eq!(r.set_root("ghost", Some(PathBuf::from("/x"))), None);
        assert!(!r.contains("ghost"), "set_root must not register windows");
        r.register("w2");
        assert_eq!(r.set_root("w2", Some(PathBuf::from("/a"))), None);
        assert_eq!(r.set_root("w2", Some(PathBuf::from("/b"))), Some(PathBuf::from("/a")));
        assert_eq!(r.set_root("w2", None), Some(PathBuf::from("/b")));
        assert_eq!(r.root_of("w2"), None);
    }

    #[test]
    fn labels_for_root_lists_every_window_on_that_folder() {
        let r = reg(&[("main", Some("/repo/a")), ("w2", Some("/repo/b")), ("w3", Some("/repo/a")), ("w4", None)]);
        assert_eq!(r.labels_for_root(Path::new("/repo/a")), vec!["main", "w3"]);
        assert_eq!(r.labels_for_root(Path::new("/repo/a/")), vec!["main", "w3"], "trailing slash is not a different folder");
        assert_eq!(r.labels_for_root(Path::new("/repo/b")), vec!["w2"]);
        assert!(r.labels_for_root(Path::new("/repo/c")).is_empty());
        assert_eq!(r.roots(), vec![PathBuf::from("/repo/a"), PathBuf::from("/repo/b")]);
    }

    #[test]
    fn select_labels_for_root_is_order_preserving() {
        let entries = [("w3", Some(Path::new("/a"))), ("main", Some(Path::new("/a"))), ("w2", Some(Path::new("/b"))), ("w4", None)];
        assert_eq!(select_labels_for_root(entries.iter().copied(), Path::new("/a")), vec!["w3", "main"]);
        assert!(select_labels_for_root(entries.iter().copied(), Path::new("/")).is_empty(), "ancestors do not match");
    }

    #[test]
    fn window_containing_picks_the_deepest_root() {
        let r = reg(&[("main", Some("/repo")), ("w2", Some("/repo/packages/app")), ("w3", None)]);
        assert_eq!(r.window_containing(Path::new("/repo/packages/app/src/x.ts")).as_deref(), Some("w2"));
        assert_eq!(r.window_containing(Path::new("/repo/packages/lib/y.ts")).as_deref(), Some("main"));
        assert_eq!(r.window_containing(Path::new("/repo")).as_deref(), Some("main"));
        assert_eq!(r.window_containing(Path::new("/elsewhere/z")), None);
        assert_eq!(r.window_containing(Path::new("/repository/z")), None, "prefix of the path string is not an ancestor");
    }

    #[test]
    fn window_containing_prefers_focused_then_oldest_on_equal_roots() {
        let r = reg(&[("main", Some("/repo")), ("w2", Some("/repo"))]);
        assert_eq!(r.window_containing(Path::new("/repo/f")).as_deref(), Some("main"), "oldest wins without focus");
        r.set_focused("w2");
        assert_eq!(r.window_containing(Path::new("/repo/f")).as_deref(), Some("w2"), "focused wins");
    }

    #[test]
    fn next_label_sequence_skips_taken_labels() {
        let r = WindowRegistry::default();
        assert_eq!(r.next_label(), "w2");
        assert_eq!(r.next_label(), "w3");
        r.register("w4");
        assert_eq!(r.next_label(), "w5");
        assert_eq!(r.next_label(), "w6");
    }

    #[test]
    fn is_last_window_for_root() {
        let r = reg(&[("main", Some("/a")), ("w2", Some("/a")), ("w3", Some("/b")), ("w4", None)]);
        assert!(!r.is_last_window_for_root("main"));
        assert!(!r.is_last_window_for_root("w2"));
        assert!(r.is_last_window_for_root("w3"));
        assert!(!r.is_last_window_for_root("w4"), "no root, nothing to be last for");
        assert!(!r.is_last_window_for_root("nope"));
        r.unregister("w2");
        assert!(r.is_last_window_for_root("main"));
    }

    #[test]
    fn set_focused_ignores_unregistered_labels() {
        let r = reg(&[("main", None)]);
        r.set_focused("browser-1");
        assert_eq!(r.focused(), None);
        r.set_focused("main");
        r.set_focused("browser-1");
        assert_eq!(r.focused().as_deref(), Some("main"));
    }

    #[test]
    fn root_refs_first_and_last_semantics() {
        let refs = RootRefs::default();
        let a = Path::new("/a");
        assert!(refs.acquire(a), "first window on /a");
        assert!(!refs.acquire(a), "second window on /a");
        assert_eq!(refs.count(a), 2);
        assert!(!refs.release(a), "one window left");
        assert!(refs.release(a), "last window left");
        assert_eq!(refs.count(a), 0);
        assert!(refs.roots().is_empty());
        assert!(!refs.release(a), "releasing an unknown root is not 'last'");
        assert!(refs.acquire(a), "first again after the count dropped to zero");
    }

    #[test]
    fn summaries_carry_name_root_and_focus() {
        let r = reg(&[("main", Some("/repo/caret")), ("w2", None)]);
        r.set_focused("w2");
        let s = summaries(&r);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].label.as_str(), s[0].name.as_deref(), s[0].root.as_deref(), s[0].focused), ("main", Some("caret"), Some("/repo/caret"), false));
        assert_eq!((s[1].label.as_str(), s[1].name.as_deref(), s[1].root.as_deref(), s[1].focused), ("w2", None, None, true));
        assert_eq!(window_title(Some(Path::new("/repo/caret"))), "caret — Caret");
        assert_eq!(window_title(None), "Caret");
    }
}

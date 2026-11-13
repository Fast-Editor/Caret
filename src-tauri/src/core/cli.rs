//! `caret` command-line launcher.
//!
//! * Parses argv (`caret . src/a.ts:10:5 -g b.ts:3`) and macOS open-documents / `caret://open?...` URLs into
//!   [`OpenRequest`]s. [`route_open`] decides which window each request belongs to (docs/CLI.md, "Which window
//!   opens it"); the requests are queued per window label in [`OpenQueue`] until that window's UI drains them
//!   (`cli_take_pending_opens`). The window is poked with the `caret://open-paths` event and always drains its own
//!   queue, so each request is delivered once, to one window.
//! * Installs the launcher the way VS Code does: the script in `src-tauri/cli/caret.sh` is rendered to
//!   `~/.caret/bin/caret` and symlinked into `/usr/local/bin` when writable, else a user-writable PATH dir
//!   (`~/.local/bin`, Homebrew, `~/bin`), else `/usr/local/bin` through the macOS admin prompt (osascript).
//!   The rendered script is refreshed on every start so a moved or updated app keeps working.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::core::windows::{self, WindowEntry, WindowRegistry, MAIN_LABEL};
use crate::error::{CaretError, Result};

pub const SCRIPT_TEMPLATE: &str = include_str!("../../cli/caret.sh");
pub const LINK_PATH: &str = "/usr/local/bin/caret";
pub const OPEN_PATHS_EVENT: &str = "caret://open-paths";
const FALLBACK_BUNDLE_ID: &str = "dev.caret.editor";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenRequest {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub col: Option<u32>,
}

impl OpenRequest {
    pub fn folder(path: &Path) -> Self { Self { path: path.to_string_lossy().to_string(), line: None, col: None } }
}

/// Open requests per window label, queued until that window's UI drains them (`cli_take_pending_opens`): the
/// frontend may not be listening yet (cold start, a window created for the request) or may be busy. A label that
/// was never pushed, or was already drained, yields nothing; `prune` drops the queues of windows that are gone.
#[derive(Default)]
pub struct OpenQueue(Mutex<HashMap<String, Vec<OpenRequest>>>);
impl OpenQueue {
    pub fn push(&self, label: &str, reqs: Vec<OpenRequest>) {
        if reqs.is_empty() { return; }
        self.0.lock().entry(label.to_string()).or_default().extend(reqs);
    }
    /// Remove and return everything queued for `label` (empty for an unknown label).
    pub fn take(&self, label: &str) -> Vec<OpenRequest> { self.0.lock().remove(label).unwrap_or_default() }
    /// Drop the queues whose label `keep` rejects (windows that were destroyed before draining).
    pub fn prune(&self, keep: impl Fn(&str) -> bool) { self.0.lock().retain(|l, _| keep(l)); }
    pub fn pending(&self, label: &str) -> usize { self.0.lock().get(label).map(|v| v.len()).unwrap_or(0) }
    /// Labels with queued requests (sorted).
    pub fn labels(&self) -> Vec<String> { let mut v: Vec<String> = self.0.lock().keys().cloned().collect(); v.sort(); v }
}

// ───────────────────────────── routing ─────────────────────────────

/// What the router needs to know about one window: its label and the folder it shows (`None` = empty window).
/// A plain copy of the registry so [`route_open`] is a pure function (see `registry_view`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowView { pub label: String, pub root: Option<PathBuf> }
impl WindowView {
    pub fn new(label: &str, root: Option<&str>) -> Self { Self { label: label.to_string(), root: root.map(PathBuf::from) } }
    pub fn is_empty(&self) -> bool { self.root.is_none() }
}
impl From<&WindowEntry> for WindowView {
    fn from(e: &WindowEntry) -> Self { Self { label: e.label.clone(), root: e.root.clone() } }
}
/// The registry's windows in creation order (the router's "oldest" tie-break).
pub fn registry_view(registry: &WindowRegistry) -> Vec<WindowView> { registry.entries().iter().map(WindowView::from).collect() }

/// Where a group of requests goes: an existing window, or a window to create (showing `folder`, or empty for
/// loose files when no Caret window exists).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target { Window(String), NewWindow(Option<PathBuf>) }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routed { pub target: Target, pub reqs: Vec<OpenRequest> }

/// Decide which window opens each request. Pure: `windows` is the registry in creation order (`registry_view`),
/// `focused` the focused label (unknown or `None` → `main`, then the oldest window), `open_folders_in` the
/// `window.openFoldersIn` setting (`"current"`; anything else means `"new"`), `is_dir` tells folders from files.
/// Per request, in order:
///
/// 1. a folder that a window already shows (same root) → that window (focused first, then oldest);
/// 2. a file under an open root → the window with the deepest such root (focused first, then oldest);
/// 3. a folder while the focused window is empty → the focused window;
/// 4. any other folder → a new window, or the focused window (replacing its folder) when the setting is
///    `"current"`; a window is claimed by at most one folder per call, so `caret a b` never replaces twice;
/// 5. a loose file → the focused window, or one new empty window when there is no window at all.
///
/// Folders claimed in this call count as roots for the requests after them, so `caret ~/proj ~/proj/a.ts` keeps
/// `a.ts` with its folder; requests that resolve to the same new folder share one `NewWindow`. Groups come out in
/// first-use order with the requests in input order.
pub fn route_open(reqs: &[OpenRequest], windows: &[WindowView], focused: Option<&str>, open_folders_in: &str, is_dir: impl Fn(&Path) -> bool) -> Vec<Routed> {
    // Effective roots: the registry's, updated as folder requests claim windows during this call.
    let mut roots: Vec<(String, Option<PathBuf>)> = windows.iter().map(|w| (w.label.clone(), w.root.clone())).collect();
    let focused: Option<String> = focused.filter(|f| roots.iter().any(|(l, _)| l == f)).map(str::to_string)
        .or_else(|| roots.iter().find(|(l, _)| l == MAIN_LABEL).map(|(l, _)| l.clone()))
        .or_else(|| roots.first().map(|(l, _)| l.clone()));
    let replace_in_place = open_folders_in == "current";
    let mut claimed: HashSet<String> = HashSet::new();
    let mut out: Vec<Routed> = Vec::new();
    for req in reqs {
        let path = PathBuf::from(&req.path);
        let target = if is_dir(&path) {
            if let Some(label) = window_with_root(&roots, focused.as_deref(), &path) { Target::Window(label) }
            else if out.iter().any(|g| g.target == Target::NewWindow(Some(path.clone()))) { Target::NewWindow(Some(path.clone())) }
            else {
                let claim = focused.as_ref().filter(|f| !claimed.contains(*f)).filter(|f| root_of(&roots, f).is_none() || replace_in_place).cloned();
                match claim {
                    Some(f) => {
                        if let Some(slot) = roots.iter_mut().find(|(l, _)| *l == f) { slot.1 = Some(path.clone()); }
                        claimed.insert(f.clone());
                        Target::Window(f)
                    }
                    None => Target::NewWindow(Some(path.clone())),
                }
            }
        } else if let Some(t) = containing(&roots, &out, focused.as_deref(), &path) { t }
        else if let Some(f) = &focused { Target::Window(f.clone()) }
        else { Target::NewWindow(None) };
        match out.iter_mut().find(|g| g.target == target) {
            Some(g) => g.reqs.push(req.clone()),
            None => out.push(Routed { target, reqs: vec![req.clone()] }),
        }
    }
    out
}

fn root_of<'a>(roots: &'a [(String, Option<PathBuf>)], label: &str) -> Option<&'a Path> {
    roots.iter().find(|(l, _)| l == label).and_then(|(_, r)| r.as_deref())
}

/// Rule 1: a window whose root is exactly `path` (the focused one first, then the oldest).
fn window_with_root(roots: &[(String, Option<PathBuf>)], focused: Option<&str>, path: &Path) -> Option<String> {
    let mut matches = roots.iter().filter(|(_, r)| r.as_deref() == Some(path)).map(|(l, _)| l.as_str());
    let first = matches.next()?;
    if focused.map(|f| roots.iter().any(|(l, r)| l == f && r.as_deref() == Some(path))).unwrap_or(false) { return focused.map(str::to_string); }
    Some(first.to_string())
}

/// Rule 2: the deepest root that is `path` or an ancestor of it, among the windows (focused first, then oldest) and
/// the folders this call is already opening in new windows (a window beats a new group at equal depth).
fn containing(roots: &[(String, Option<PathBuf>)], out: &[Routed], focused: Option<&str>, path: &Path) -> Option<Target> {
    // key: (depth, window-over-new and focused-over-others, newer-is-worse) — the maximum wins
    let mut best: Option<((usize, u8, std::cmp::Reverse<usize>), Target)> = None;
    let mut consider = |key: (usize, u8, std::cmp::Reverse<usize>), t: Target| { if best.as_ref().map(|(k, _)| key > *k).unwrap_or(true) { best = Some((key, t)); } };
    for (i, (label, root)) in roots.iter().enumerate() {
        let Some(r) = root.as_deref().filter(|r| path.starts_with(r)) else { continue };
        let prio = if focused == Some(label.as_str()) { 2 } else { 1 };
        consider((r.components().count(), prio, std::cmp::Reverse(i)), Target::Window(label.clone()));
    }
    for g in out {
        if let Target::NewWindow(Some(f)) = &g.target { if path.starts_with(f) { consider((f.components().count(), 0, std::cmp::Reverse(usize::MAX)), g.target.clone()); } }
    }
    best.map(|(_, t)| t)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    /// A `caret` link on the user's PATH (or `/usr/local/bin`) points at Caret's script.
    pub installed: bool,
    /// The symlink path when it exists.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub path: Option<String>,
    /// What the symlink points at (may be a foreign `caret` binary).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub target: Option<String>,
    /// `~/.caret/bin/caret` (always usable by adding `~/.caret/bin` to PATH).
    pub script_path: String,
    /// A `caret` executable is reachable through the current PATH.
    pub on_path: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub error: Option<String>,
}

// ───────────────────────────── parsing ─────────────────────────────

fn is_num(s: &str) -> bool { !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) }

/// Split `file:line[:col]` from the right; returns `(file, line, col)`; `line` is `None` when there is no numeric suffix.
pub fn split_goto(spec: &str) -> (&str, Option<u32>, Option<u32>) {
    let Some((head, tail)) = spec.rsplit_once(':') else { return (spec, None, None) };
    if !is_num(tail) { return (spec, None, None); }
    if let Some((head2, tail2)) = head.rsplit_once(':') {
        if is_num(tail2) { return (head2, tail2.parse().ok(), tail.parse().ok()); }
    }
    (head, tail.parse().ok(), None)
}

fn expand_home(p: &str) -> PathBuf {
    if p == "~" { return crate::paths::home_dir(); }
    if let Some(rest) = p.strip_prefix("~/") { return crate::paths::home_dir().join(rest); }
    PathBuf::from(p)
}

fn absolutize(p: &str, cwd: &Path) -> PathBuf {
    let p = expand_home(p);
    let abs = if p.is_absolute() { p } else { cwd.join(p) };
    std::fs::canonicalize(&abs).unwrap_or(abs)
}

/// Resolve one positional argument. `goto` forces the `file:line[:col]` interpretation (`-g`); otherwise an
/// existing path wins, then a numeric suffix on an existing prefix, then the literal path.
fn resolve_spec(arg: &str, cwd: &Path, goto: bool) -> OpenRequest {
    let (file, line, col) = split_goto(arg);
    if !goto && (line.is_none() || cwd.join(expand_home(arg)).exists() || expand_home(arg).exists()) {
        let lit = absolutize(arg, cwd);
        if lit.exists() || line.is_none() { return OpenRequest { path: lit.to_string_lossy().to_string(), line: None, col: None }; }
    }
    if line.is_some() {
        let f = absolutize(file, cwd);
        if goto || f.exists() { return OpenRequest { path: f.to_string_lossy().to_string(), line, col }; }
    }
    OpenRequest { path: absolutize(arg, cwd).to_string_lossy().to_string(), line: None, col: None }
}

/// Parse launcher-style argv (without the program name) relative to `cwd`. A leading `serve` and its options
/// (`--host`, `--port`, `--no-window`, `--print-url`) belong to `remote::init` and are skipped here.
pub fn parse_open_args_in(args: &[String], cwd: &Path) -> Vec<OpenRequest> {
    let (_, args) = crate::remote::strip_serve_args(args);
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-g" | "--goto" => { if let Some(spec) = it.next() { out.push(resolve_spec(spec, cwd, true)); } }
            s if s.starts_with("--goto=") => out.push(resolve_spec(&s["--goto=".len()..], cwd, true)),
            "--" => {}
            s if s.starts_with('-') => {} // -r/-n/-w/-psn_…: accepted and ignored
            s => out.push(resolve_spec(s, cwd, false)),
        }
    }
    out
}

/// Parse the process argv (skipping the executable) relative to the current directory.
pub fn parse_open_args(args: &[String]) -> Vec<OpenRequest> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    parse_open_args_in(args, &cwd)
}

/// Parse macOS open-documents (`file://`) and `caret://open?path=&line=&col=` URLs.
pub fn parse_opened_urls(urls: &[tauri::Url]) -> Vec<OpenRequest> {
    let mut out = Vec::new();
    for u in urls {
        match u.scheme() {
            "file" => {
                if let Ok(p) = u.to_file_path() {
                    let p = std::fs::canonicalize(&p).unwrap_or(p);
                    out.push(OpenRequest { path: p.to_string_lossy().to_string(), line: None, col: None });
                }
            }
            "caret" => {
                let action = u.host_str().map(|h| h.to_string()).unwrap_or_default();
                let action = if action.is_empty() { u.path().trim_matches('/').to_string() } else { action };
                if action != "open" { continue; }
                let mut path = None; let mut line = None; let mut col = None;
                for (k, v) in u.query_pairs() {
                    match &*k { "path" | "file" => path = Some(v.to_string()), "line" => line = v.parse().ok(), "col" | "column" => col = v.parse().ok(), _ => {} }
                }
                if let Some(p) = path {
                    let p = expand_home(&p);
                    let p = std::fs::canonicalize(&p).unwrap_or(p);
                    out.push(OpenRequest { path: p.to_string_lossy().to_string(), line, col });
                }
            }
            _ => {}
        }
    }
    out
}

// ───────────────────────────── app wiring ─────────────────────────────

/// Called from `setup`: route argv paths to the (still booting) `main` window and refresh an already-installed
/// launcher script. Nothing is focused or shown here (`caret serve --no-window` hides `main` right after).
pub fn init(app: &AppHandle) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let reqs = parse_open_args(&args);
    if !reqs.is_empty() { tracing::info!(count = reqs.len(), "cli: open requests from argv"); }
    dispatch(app, reqs, false);
    refresh_installed_script(app);
}

/// macOS `RunEvent::Opened` (`caret <path>`, Finder "Open with", `caret://open?…`): route the requests to their
/// windows, creating windows as needed, and bring the target windows to the front.
pub fn on_opened(app: &AppHandle, urls: &[tauri::Url]) {
    let reqs = parse_opened_urls(urls);
    tracing::info!(urls = urls.len(), parsed = reqs.len(), "cli: open event");
    dispatch(app, reqs, true);
}

/// Run [`route_open`] over the registry and the `window.openFoldersIn` setting, then deliver every group: an
/// existing window gets the requests queued under its label, an `OPEN_PATHS` poke and (with `focus`) the front;
/// a `NewWindow` group creates the window with its folder already open and queues the remaining requests (the
/// files) for the new label, which drains them when its frontend boots.
pub fn dispatch(app: &AppHandle, reqs: Vec<OpenRequest>, focus: bool) {
    if reqs.is_empty() { return; }
    let registry = app.state::<WindowRegistry>();
    let view = registry_view(&registry);
    let focused = registry.focused();
    let open_folders_in = app.state::<crate::core::settings::SettingsState>().effective().window.open_folders_in;
    let routed = route_open(&reqs, &view, focused.as_deref(), &open_folders_in, |p| p.is_dir());
    for group in routed {
        match group.target {
            Target::Window(label) => {
                tracing::info!(label, count = group.reqs.len(), "cli: routed to window");
                deliver(app, &label, group.reqs);
                if focus { if let Err(e) = windows::focus_window(app, &label) { tracing::debug!(label, error = %e.message, "cli: could not focus window"); } }
            }
            Target::NewWindow(folder) => {
                // The folder request itself is satisfied by creating the window on that folder; only files remain.
                let rest: Vec<OpenRequest> = match &folder {
                    Some(f) => group.reqs.into_iter().filter(|r| Path::new(&r.path) != f.as_path()).collect(),
                    None => group.reqs,
                };
                match windows::create_window(app, folder.clone()) {
                    Ok(label) => { tracing::info!(label, folder = ?folder, count = rest.len(), "cli: routed to new window"); deliver(app, &label, rest); }
                    Err(e) => {
                        tracing::warn!(error = %e.message, folder = ?folder, "cli: could not create a window; using the focused one");
                        // Do not lose the user's paths: the focused window gets them (the folder opens in place).
                        if let Some(label) = windows::focused_label(app) {
                            let mut all = rest;
                            if let Some(f) = folder { all.insert(0, OpenRequest::folder(&f)); }
                            deliver(app, &label, all);
                            if focus { let _ = windows::focus_window(app, &label); }
                        }
                    }
                }
            }
        }
    }
}

/// Queue `reqs` for window `label` and poke its frontend. Queues of windows that no longer exist are dropped first.
fn deliver(app: &AppHandle, label: &str, reqs: Vec<OpenRequest>) {
    if reqs.is_empty() { return; }
    let registry = app.state::<WindowRegistry>();
    let queue = app.state::<OpenQueue>();
    queue.prune(|l| registry.contains(l));
    queue.push(label, reqs.clone());
    crate::events::emit_to(app, label, OPEN_PATHS_EVENT, serde_json::json!({ "paths": reqs }));
}

// ───────────────────────────── install ─────────────────────────────

pub fn script_path() -> PathBuf { crate::paths::config_dir().join("bin").join("caret") }

fn bundle_id(app: &AppHandle) -> String {
    let id = app.config().identifier.clone();
    if id.is_empty() { FALLBACK_BUNDLE_ID.into() } else { id }
}

/// `…/Caret.app` for a bundled build; the executable itself otherwise (dev builds).
pub fn app_path() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/Applications/Caret.app"));
    let mut p = exe.as_path();
    for _ in 0..3 { match p.parent() { Some(x) => p = x, None => return exe.clone() } }
    if p.extension().map(|e| e == "app").unwrap_or(false) { p.to_path_buf() } else { exe }
}

pub fn render_script(bundle_id: &str, app_path: &str, version: &str) -> String {
    SCRIPT_TEMPLATE.replace("__CARET_BUNDLE_ID__", bundle_id).replace("__CARET_APP_PATH__", app_path).replace("__CARET_VERSION__", version)
}

fn rendered_for(app: &AppHandle) -> String { render_script(&bundle_id(app), &app_path().to_string_lossy(), env!("CARGO_PKG_VERSION")) }

/// Write `~/.caret/bin/caret` (0755) when its content differs. Returns the script path.
fn write_script(content: &str) -> Result<PathBuf> { let path = script_path(); write_script_at(&path, content)?; Ok(path) }

/// Write `content` to `path` (0755) unless the file already has exactly that content.
fn write_script_at(path: &Path, content: &str) -> Result<()> {
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir).map_err(|e| CaretError::io(format!("create {}: {e}", dir.display())))?; }
    let same = std::fs::read_to_string(path).map(|c| c == content).unwrap_or(false);
    if !same { std::fs::write(path, content).map_err(|e| CaretError::io(format!("write {}: {e}", path.display())))?; }
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)); }
    Ok(())
}

/// Self-heal on every start: when `~/.caret/bin/caret` exists, re-render the embedded template and rewrite the file
/// unless it is byte-for-byte identical. The comparison is against the whole rendered script, not a version field,
/// so a template change in a newer build (or a moved app, or a dev build with another `APP_PATH`) reaches the
/// installed launcher the first time that build starts. No separate revision marker is needed for that; the
/// `Generated by Caret <version>` header line is informational only.
fn refresh_installed_script(app: &AppHandle) {
    let path = script_path();
    if !path.exists() { return; }
    // Only a bundled build may refresh the user's launcher: a `tauri dev` binary would otherwise point `caret` at
    // `target/debug/caret`, which stops working the moment the dev session ends.
    if app_path().extension().and_then(|e| e.to_str()) != Some("app") { tracing::debug!(path = %app_path().display(), "cli: not a bundled build, leaving the installed launcher alone"); return; }
    if let Err(e) = write_script(&rendered_for(app)) { tracing::warn!(error = %e, "cli: could not refresh launcher script"); }
}

/// The login shell's PATH (GUI apps inherit launchd's minimal PATH, so `~/.local/bin`, Homebrew etc. are invisible
/// from inside the app). Computed once; falls back to the process PATH if the shell does not answer in time.
fn user_shell_path() -> &'static [PathBuf] {
    static CACHE: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        let shell = std::env::var("SHELL").ok().filter(|s| Path::new(s).exists()).unwrap_or_else(|| "/bin/zsh".into());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let out = std::process::Command::new(&shell).args(["-lic", "printf %s \"$PATH\""]).stdin(std::process::Stdio::null()).output();
            let _ = tx.send(out.ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).to_string()));
        });
        let mut dirs: Vec<PathBuf> = rx.recv_timeout(std::time::Duration::from_secs(6)).ok().flatten().map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        if let Some(p) = std::env::var_os("PATH") { dirs.extend(std::env::split_paths(&p)); }
        let mut seen = std::collections::HashSet::new(); dirs.retain(|d| seen.insert(d.clone()));
        dirs
    })
}

/// Can this user create files in `dir` right now (no admin rights)?
fn dir_writable(dir: &Path) -> bool {
    if !dir.is_dir() { return false; }
    let probe = dir.join(format!(".caret-write-probe-{}", std::process::id()));
    match std::fs::File::create(&probe) { Ok(_) => { let _ = std::fs::remove_file(&probe); true } Err(_) => false }
}

/// Where to link `caret`, best first: `/usr/local/bin` when it is writable without admin rights (it is root-owned on
/// most modern Macs), then user-writable directories that are already on the login shell's PATH — no password dialog.
fn link_candidates() -> Vec<PathBuf> {
    let home = crate::paths::home_dir();
    let on_path = user_shell_path();
    let mut v = vec![];
    let usr_local = PathBuf::from(LINK_PATH).parent().map(|p| p.to_path_buf()).unwrap_or_default();
    if dir_writable(&usr_local) { v.push(usr_local); }
    for d in [home.join(".local/bin"), PathBuf::from("/opt/homebrew/bin"), home.join("bin")] {
        if on_path.iter().any(|p| p == &d) && dir_writable(&d) { v.push(d); }
    }
    v
}

/// Every directory a `caret` link could live in (search order for status/uninstall).
fn link_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = vec![PathBuf::from(LINK_PATH).parent().map(|p| p.to_path_buf()).unwrap_or_default()];
    dirs.extend(link_candidates());
    dirs.extend(user_shell_path().iter().cloned());
    let mut seen = std::collections::HashSet::new(); dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

pub fn status(app: &AppHandle) -> CliStatus {
    let _ = app;
    let script = script_path();
    let script_s = script.to_string_lossy().to_string();
    // Prefer a link that points at our script; otherwise report the first foreign `caret` we find.
    let mut found: Option<(PathBuf, Option<String>)> = None;
    for d in link_dirs() {
        let l = d.join("caret");
        if std::fs::symlink_metadata(&l).is_err() { continue; }
        let t = std::fs::read_link(&l).ok().map(|t| t.to_string_lossy().to_string());
        let ours = t.as_deref() == Some(&*script_s);
        if ours { found = Some((l, t)); break; }
        if found.is_none() { found = Some((l, t)); }
    }
    let installed = found.as_ref().map(|(_, t)| t.as_deref() == Some(&*script_s)).unwrap_or(false);
    let on_path = installed && found.as_ref().and_then(|(l, _)| l.parent().map(|p| user_shell_path().iter().any(|d| d == p))).unwrap_or(false);
    CliStatus { installed, path: found.as_ref().map(|(l, _)| l.to_string_lossy().to_string()), target: found.and_then(|(_, t)| t), script_path: script_s, on_path, error: None }
}

fn shell_single_quote(s: &str) -> String { format!("'{}'", s.replace('\'', "'\\''")) }
fn applescript_quote(s: &str) -> String { format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")) }

/// Run a shell command with administrator privileges through the macOS authorisation dialog (what VS Code does).
fn admin_shell(cmd: &str) -> Result<()> {
    let script = format!("do shell script {} with administrator privileges", applescript_quote(cmd));
    let out = std::process::Command::new("osascript").arg("-e").arg(&script).output().map_err(|e| CaretError::io(format!("osascript: {e}")))?;
    if out.status.success() { return Ok(()); }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if err.contains("-128") { return Err(CaretError::denied("administrator authorisation was cancelled")); }
    Err(CaretError::denied(format!("administrator authorisation failed: {err}")))
}

#[cfg(unix)]
fn try_link_in(dir: &Path, script: &Path) -> std::io::Result<()> {
    if !dir.exists() { std::fs::create_dir_all(dir)?; }
    let link = dir.join("caret");
    if std::fs::symlink_metadata(&link).is_ok() { std::fs::remove_file(&link)?; }
    std::os::unix::fs::symlink(script, &link)
}
#[cfg(not(unix))]
fn try_link_in(_dir: &Path, _script: &Path) -> std::io::Result<()> { Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "symlinks unsupported")) }

pub fn install(app: &AppHandle) -> Result<CliStatus> {
    let script = write_script(&rendered_for(app))?;
    // 1) A writable directory that is already on the user's PATH — no password dialog.
    for dir in link_candidates() {
        match try_link_in(&dir, &script) {
            Ok(()) => return Ok(status(app)),
            Err(e) => tracing::warn!(dir = %dir.display(), error = %e, "cli: could not link here, trying the next location"),
        }
    }
    // 2) /usr/local/bin through the macOS administrator dialog (what VS Code does).
    let cmd = format!("mkdir -p /usr/local/bin && ln -sf {} {}", shell_single_quote(&script.to_string_lossy()), LINK_PATH);
    if let Err(e) = admin_shell(&cmd) {
        let mut s = status(app);
        s.error = Some(format!("{}. The launcher is still available at {}; add {} to your PATH.", e.message, script.display(), script.parent().map(|p| p.display().to_string()).unwrap_or_default()));
        return Ok(s);
    }
    Ok(status(app))
}

pub fn uninstall(app: &AppHandle) -> Result<CliStatus> {
    let script_s = script_path().to_string_lossy().to_string();
    for d in link_dirs() {
        let link = d.join("caret");
        let ours = std::fs::read_link(&link).ok().map(|t| t.to_string_lossy() == script_s).unwrap_or(false);
        if !ours { continue; }
        if let Err(e) = std::fs::remove_file(&link) {
            if e.kind() == std::io::ErrorKind::PermissionDenied && link == Path::new(LINK_PATH) {
                if let Err(e) = admin_shell(&format!("rm -f {LINK_PATH}")) { let mut s = status(app); s.error = Some(e.message); return Ok(s); }
            } else { let mut s = status(app); s.error = Some(format!("could not remove {}: {e}", link.display())); return Ok(s); }
        }
    }
    Ok(status(app))
}

// ───────────────────────────── commands ─────────────────────────────

/// Drain the calling window's queue only (another window's requests stay where the router put them).
#[tauri::command]
pub fn cli_take_pending_opens(window: tauri::Window, q: State<'_, OpenQueue>) -> Result<Vec<OpenRequest>> { Ok(q.take(window.label())) }

#[tauri::command(async)]
pub fn cli_status(app: AppHandle) -> Result<CliStatus> { Ok(status(&app)) }

#[tauri::command]
pub async fn cli_install(app: AppHandle) -> Result<CliStatus> {
    tokio::task::spawn_blocking(move || install(&app)).await.map_err(|e| CaretError::internal(format!("install task: {e}")))?
}

#[tauri::command]
pub async fn cli_uninstall(app: AppHandle) -> Result<CliStatus> {
    tokio::task::spawn_blocking(move || uninstall(&app)).await.map_err(|e| CaretError::internal(format!("uninstall task: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }

    #[test]
    fn split_goto_parses_suffixes() {
        assert_eq!(split_goto("a.ts"), ("a.ts", None, None));
        assert_eq!(split_goto("a.ts:10"), ("a.ts", Some(10), None));
        assert_eq!(split_goto("a.ts:10:5"), ("a.ts", Some(10), Some(5)));
        assert_eq!(split_goto("c:dir/a.ts:3"), ("c:dir/a.ts", Some(3), None));
        assert_eq!(split_goto("a.ts:x"), ("a.ts:x", None, None));
    }

    #[test]
    fn parse_args_resolves_paths_positions_and_ignores_flags() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.ts"), "x").unwrap();
        std::fs::write(root.join("weird:1"), "x").unwrap();
        let r = parse_open_args_in(&s(&["-r", "--new-window", ".", "src/a.ts:10:5", "-g", "src/a.ts:3", "--goto=src/a.ts:7:2", "weird:1", "--wait"]), &root);
        assert_eq!(r.len(), 5, "{r:?}");
        assert_eq!(r[0], OpenRequest { path: root.to_string_lossy().to_string(), line: None, col: None });
        assert_eq!(r[1], OpenRequest { path: root.join("src/a.ts").to_string_lossy().to_string(), line: Some(10), col: Some(5) });
        assert_eq!(r[2].line, Some(3)); assert_eq!(r[2].col, None);
        assert_eq!(r[3], OpenRequest { path: root.join("src/a.ts").to_string_lossy().to_string(), line: Some(7), col: Some(2) });
        // an existing file whose name ends in `:1` is opened literally
        assert_eq!(r[4], OpenRequest { path: root.join("weird:1").to_string_lossy().to_string(), line: None, col: None });
    }

    #[test]
    fn parse_args_nonexistent_with_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        // neither `nope.ts:4` nor `nope.ts` exist: bare arg stays literal, -g splits
        let r = parse_open_args_in(&s(&["nope.ts:4", "-g", "nope.ts:4"]), &root);
        assert_eq!(r[0], OpenRequest { path: root.join("nope.ts:4").to_string_lossy().to_string(), line: None, col: None });
        assert_eq!(r[1], OpenRequest { path: root.join("nope.ts").to_string_lossy().to_string(), line: Some(4), col: None });
        assert!(parse_open_args_in(&s(&["-g"]), &root).is_empty());
    }

    #[test]
    fn parse_args_skips_serve_and_its_options() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        // `serve` and its options never become paths; a folder after them still opens
        let r = parse_open_args_in(&s(&["serve", "--host", "0.0.0.0", "--port", "17431", "--no-window", "--print-url", "."]), &root);
        assert_eq!(r, vec![OpenRequest { path: root.to_string_lossy().to_string(), line: None, col: None }]);
        assert!(parse_open_args_in(&s(&["serve"]), &root).is_empty());
        assert!(parse_open_args_in(&s(&["serve", "--host=127.0.0.1", "--port=1"]), &root).is_empty());
        // only a leading `serve` is special
        let r = parse_open_args_in(&s(&["serve.ts"]), &root);
        assert_eq!(r[0].path, root.join("serve.ts").to_string_lossy().to_string());
    }

    #[test]
    fn parse_urls_handles_file_and_caret_schemes() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::create_dir_all(root.join("x y")).unwrap();
        std::fs::write(root.join("x y/a.ts"), "x").unwrap();
        let file_url = tauri::Url::from_file_path(root.join("x y/a.ts")).unwrap();
        assert!(file_url.as_str().contains("x%20y"));
        let enc = file_url.path().to_string();
        let caret_url = tauri::Url::parse(&format!("caret://open?path={enc}&line=12&col=3")).unwrap();
        let other = tauri::Url::parse("https://example.com/a.ts").unwrap();
        let noise = tauri::Url::parse("caret://settings?x=1").unwrap();
        let r = parse_opened_urls(&[file_url, caret_url, other, noise]);
        assert_eq!(r.len(), 2, "{r:?}");
        assert_eq!(r[0], OpenRequest { path: root.join("x y/a.ts").to_string_lossy().to_string(), line: None, col: None });
        assert_eq!(r[1], OpenRequest { path: root.join("x y/a.ts").to_string_lossy().to_string(), line: Some(12), col: Some(3) });
    }

    #[test]
    fn rendered_script_dry_run_emits_open_commands() {
        if std::process::Command::new("sh").arg("-c").arg("true").status().map(|s| !s.success()).unwrap_or(true) { eprintln!("sh unavailable; skipping"); return; }
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(root.join("foo.ts"), "x").unwrap();
        std::fs::write(root.join("b c.ts"), "x").unwrap();
        let script = root.join("caret");
        // APP_PATH points nowhere and CARET_CLI_RUNNING_APP is pinned empty, so the dry run never consults LaunchServices
        // or the disk and the output is the same on a developer Mac with Caret running and on CI.
        let app = format!("{}/missing/Caret.app", root.to_string_lossy());
        let rendered = render_script("dev.caret.editor", &app, "9.9.9");
        assert!(!rendered.contains("__CARET_"), "all placeholders substituted");
        std::fs::write(&script, rendered).unwrap();
        let out = std::process::Command::new("sh").arg(&script).args([".", "foo.ts:12:3", "-g", "b c.ts:4", "-r"]).current_dir(&root).env("CARET_CLI_DRY_RUN", "1").env("CARET_CLI_RUNNING_APP", "").output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
        let r = root.to_string_lossy();
        assert_eq!(stdout.lines().next().unwrap(), format!("open -b dev.caret.editor {r}"));
        assert!(stdout.contains(&format!("open -b dev.caret.editor caret://open?path={r}/foo.ts&line=12&col=3")), "{stdout}");
        assert!(stdout.contains(&format!("caret://open?path={r}/b%20c.ts&line=4")), "{stdout}");
        // the goto URL round-trips through the URL parser
        let url = stdout.lines().find(|l| l.contains("foo.ts")).unwrap().rsplit(' ').next().unwrap().to_string();
        let parsed = parse_opened_urls(&[tauri::Url::parse(&url).unwrap()]);
        assert_eq!(parsed[0], OpenRequest { path: root.join("foo.ts").to_string_lossy().to_string(), line: Some(12), col: Some(3) });
        // version / usage / unknown flag
        let v = std::process::Command::new("sh").arg(&script).arg("-v").output().unwrap();
        assert_eq!(String::from_utf8_lossy(&v.stdout).trim(), "caret 9.9.9");
        let bad = std::process::Command::new("sh").arg(&script).arg("--bogus").output().unwrap();
        assert_eq!(bad.status.code(), Some(2));
        let missing = std::process::Command::new("sh").arg(&script).arg("missing.ts").current_dir(&root).env("CARET_CLI_DRY_RUN", "1").env("CARET_CLI_RUNNING_APP", "").output().unwrap();
        assert_eq!(missing.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&missing.stderr).contains("no such file"));
        // `caret serve …` execs the app binary directly (no LaunchServices), forwarding its options verbatim
        let serve = std::process::Command::new("sh").arg(&script).args(["serve", "--host", "0.0.0.0", "--port", "17431", "--no-window"]).env("CARET_CLI_DRY_RUN", "1").output().unwrap();
        assert!(serve.status.success(), "stderr: {}", String::from_utf8_lossy(&serve.stderr));
        assert_eq!(String::from_utf8_lossy(&serve.stdout).trim(), format!("exec {app}/Contents/MacOS/caret serve --host 0.0.0.0 --port 17431 --no-window"));
        let help = std::process::Command::new("sh").arg(&script).args(["serve", "--help"]).output().unwrap();
        assert!(help.status.success());
        assert!(String::from_utf8_lossy(&help.stdout).contains("caret serve"));
        assert!(String::from_utf8_lossy(&std::process::Command::new("sh").arg(&script).arg("-h").output().unwrap().stdout).contains("--no-window"));
    }

    /// With several Caret.app copies sharing the bundle id, `open -b` may cold-start a copy that is not running.
    /// The launcher therefore targets, in order: the running copy, the rendered APP_PATH when it is a bundle on
    /// disk, the bundle id. `CARET_CLI_RUNNING_APP` stands in for the LaunchServices lookup.
    #[test]
    fn rendered_script_prefers_running_instance_then_app_path_then_bundle_id() {
        if std::process::Command::new("sh").arg("-c").arg("true").status().map(|s| !s.success()).unwrap_or(true) { eprintln!("sh unavailable; skipping"); return; }
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let r = root.to_string_lossy().to_string();
        std::fs::write(root.join("a.ts"), "x").unwrap();
        let run = |app_path: &str, running: Option<&str>, args: &[&str]| -> String {
            let script = root.join("caret");
            std::fs::write(&script, render_script("dev.caret.editor", app_path, "9.9.9")).unwrap();
            let mut cmd = std::process::Command::new("sh");
            cmd.arg(&script).args(args).current_dir(&root).env("CARET_CLI_DRY_RUN", "1");
            match running { Some(v) => { cmd.env("CARET_CLI_RUNNING_APP", v); } None => { cmd.env_remove("CARET_CLI_RUNNING_APP"); } }
            let out = cmd.output().unwrap();
            assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).to_string()
        };
        let bundle = root.join("Caret.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        let bundle_s = bundle.to_string_lossy().to_string();
        // 1) a running copy wins even when the rendered APP_PATH exists and differs
        let out = run(&bundle_s, Some("/Volumes/Elsewhere/Caret.app"), &[".", "-g", "a.ts:3"]);
        assert_eq!(out.lines().next().unwrap(), format!("open -a /Volumes/Elsewhere/Caret.app {r}"));
        assert_eq!(out.lines().nth(1).unwrap(), format!("open -a /Volumes/Elsewhere/Caret.app caret://open?path={r}/a.ts&line=3"));
        // plain `caret` focuses the running copy
        assert_eq!(run(&bundle_s, Some("/Volumes/Elsewhere/Caret.app"), &[]).trim(), "open -a /Volumes/Elsewhere/Caret.app");
        // 2) nothing running: the copy this script was rendered for, when it is a bundle on disk
        assert_eq!(run(&bundle_s, Some(""), &["."]).trim(), format!("open -a {bundle_s} {r}"));
        // 3) nothing running and APP_PATH is gone or a bare dev binary: fall back to the bundle id
        assert_eq!(run(&format!("{r}/gone/Caret.app"), Some(""), &["."]).trim(), format!("open -b dev.caret.editor {r}"));
        let dev_bin = root.join("target/debug/caret");
        std::fs::create_dir_all(dev_bin.parent().unwrap()).unwrap();
        std::fs::write(&dev_bin, "").unwrap();
        assert_eq!(run(&dev_bin.to_string_lossy(), Some(""), &["."]).trim(), format!("open -b dev.caret.editor {r}"));
    }

    /// The installed launcher is rewritten whenever the freshly rendered template differs byte-for-byte, which is
    /// how a template fix reaches `~/.caret/bin/caret` on the next app start; identical content is left alone.
    #[test]
    fn write_script_at_rewrites_stale_content_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bin").join("caret");
        let stale = "#!/bin/sh\n# Generated by Caret 0.2.0.\nopen -b dev.caret.editor \"$@\"\n";
        write_script_at(&path, stale).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), stale);
        let fresh = render_script("dev.caret.editor", "/Applications/Caret.app", "0.2.0");
        assert_ne!(fresh, stale);
        write_script_at(&path, &fresh).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), fresh, "an older template is replaced even at the same version");
        assert!(std::fs::read_to_string(&path).unwrap().contains("running_app()"), "the rewritten launcher carries the running-instance logic");
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o755); }
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_script_at(&path, &fresh).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), before, "identical content is not rewritten");
    }

    #[test]
    fn quoting_helpers() {
        assert_eq!(shell_single_quote("a'b"), "'a'\\''b'");
        assert_eq!(applescript_quote("say \"hi\" \\ there"), "\"say \\\"hi\\\" \\\\ there\"");
    }

    // ───────────── routing ─────────────

    fn req(p: &str) -> OpenRequest { OpenRequest { path: p.into(), line: None, col: None } }
    fn req_at(p: &str, line: u32, col: Option<u32>) -> OpenRequest { OpenRequest { path: p.into(), line: Some(line), col } }
    fn win(label: &str, root: Option<&str>) -> WindowView { WindowView::new(label, root) }
    /// Folder classifier for the pure router: exactly these paths are directories.
    fn dirs(d: &[&str]) -> impl Fn(&Path) -> bool { let set: Vec<PathBuf> = d.iter().map(PathBuf::from).collect(); move |p| set.iter().any(|x| x == p) }
    fn new_window(folder: &str) -> Target { Target::NewWindow(Some(PathBuf::from(folder))) }
    fn window(label: &str) -> Target { Target::Window(label.into()) }
    fn targets(r: &[Routed]) -> Vec<Target> { r.iter().map(|g| g.target.clone()).collect() }
    fn paths(g: &Routed) -> Vec<&str> { g.reqs.iter().map(|r| r.path.as_str()).collect() }

    #[test]
    fn route_folder_already_open_goes_to_that_window() {
        let is_dir = dirs(&["/a", "/b"]);
        let w = [win("main", Some("/a")), win("w2", Some("/b"))];
        assert_eq!(targets(&route_open(&[req("/b")], &w, Some("main"), "new", &is_dir)), vec![window("w2")]);
        assert_eq!(targets(&route_open(&[req("/b/")], &w, Some("main"), "new", &is_dir)), vec![window("w2")], "a trailing slash is the same folder (component-wise comparison)");
        // the setting does not matter for a folder that is already open
        assert_eq!(targets(&route_open(&[req("/a")], &w, Some("w2"), "current", &is_dir)), vec![window("main")]);
        // two windows on the same root: the focused one, else the oldest
        let two = [win("main", Some("/a")), win("w2", Some("/b")), win("w3", Some("/b"))];
        assert_eq!(targets(&route_open(&[req("/b")], &two, Some("w3"), "new", &is_dir)), vec![window("w3")]);
        assert_eq!(targets(&route_open(&[req("/b")], &two, Some("main"), "new", &is_dir)), vec![window("w2")]);
    }

    #[test]
    fn route_file_under_open_root_picks_deepest_then_focused_then_oldest() {
        let is_dir = dirs(&["/repo", "/repo/pkg/app"]);
        let w = [win("main", Some("/repo")), win("w2", Some("/repo/pkg/app")), win("w3", None)];
        let r = route_open(&[req_at("/repo/pkg/app/src/x.ts", 3, Some(7)), req("/repo/lib/y.ts"), req("/repository/z.ts")], &w, Some("main"), "new", &is_dir);
        assert_eq!(targets(&r), vec![window("w2"), window("main")]);
        assert_eq!(r[0].reqs, vec![req_at("/repo/pkg/app/src/x.ts", 3, Some(7))], "line/column travel with the request");
        assert_eq!(paths(&r[1]), vec!["/repo/lib/y.ts", "/repository/z.ts"], "a string prefix is not an ancestor: z.ts is a loose file for the focused window");
        // the same root in two windows: the focused one wins, else the oldest
        let two = [win("main", Some("/repo")), win("w2", Some("/repo"))];
        assert_eq!(targets(&route_open(&[req("/repo/f.ts")], &two, Some("w2"), "new", &is_dir)), vec![window("w2")]);
        assert_eq!(targets(&route_open(&[req("/repo/f.ts")], &two, Some("main"), "new", &is_dir)), vec![window("main")]);
        assert_eq!(targets(&route_open(&[req("/repo/f.ts")], &two, None, "new", &is_dir)), vec![window("main")]);
        let no_main = [win("w3", Some("/repo")), win("w2", Some("/repo"))];
        assert_eq!(targets(&route_open(&[req("/repo/f.ts")], &no_main, None, "new", &is_dir)), vec![window("w3")], "oldest without focus and without main");
        // a file in a window's root beats a loose-file destination even when that window is not focused
        assert_eq!(targets(&route_open(&[req("/repo/pkg/app/a.ts")], &w, Some("w3"), "new", &is_dir)), vec![window("w2")]);
    }

    #[test]
    fn route_folder_to_focused_empty_window_and_files_follow_it() {
        let is_dir = dirs(&["/a", "/c"]);
        let w = [win("main", Some("/a")), win("w2", None)];
        let r = route_open(&[req("/c"), req("/c/x.ts"), req("/elsewhere/l.ts")], &w, Some("w2"), "new", &is_dir);
        assert_eq!(targets(&r), vec![window("w2")]);
        assert_eq!(paths(&r[0]), vec!["/c", "/c/x.ts", "/elsewhere/l.ts"], "the claimed folder is a root for later files; loose files go to the focused window");
        // the empty window is not focused: the folder goes to a new window (setting "new") and the empty one stays empty
        assert_eq!(targets(&route_open(&[req("/c")], &w, Some("main"), "new", &is_dir)), vec![new_window("/c")]);
    }

    #[test]
    fn route_folder_new_window_or_replace_depending_on_setting() {
        let is_dir = dirs(&["/a", "/c", "/d"]);
        let w = [win("main", Some("/a"))];
        let r = route_open(&[req("/c"), req_at("/c/x.ts", 10, None), req("/a/k.ts")], &w, Some("main"), "new", &is_dir);
        assert_eq!(targets(&r), vec![new_window("/c"), window("main")]);
        assert_eq!(paths(&r[0]), vec!["/c", "/c/x.ts"]);
        assert_eq!(paths(&r[1]), vec!["/a/k.ts"]);
        let r = route_open(&[req("/c"), req_at("/c/x.ts", 10, None), req("/a/k.ts")], &w, Some("main"), "current", &is_dir);
        assert_eq!(targets(&r), vec![window("main")], "\"current\" replaces the focused window's folder");
        assert_eq!(paths(&r[0]), vec!["/c", "/c/x.ts", "/a/k.ts"], "/a/k.ts is now a loose file in the focused window");
        // a window is claimed by one folder per call: the second folder opens a new window even with "current"
        let r = route_open(&[req("/c"), req("/d"), req("/d/y.ts")], &w, Some("main"), "current", &is_dir);
        assert_eq!(targets(&r), vec![window("main"), new_window("/d")]);
        assert_eq!(paths(&r[1]), vec!["/d", "/d/y.ts"]);
        // an unknown setting value behaves like "new"
        assert_eq!(targets(&route_open(&[req("/c")], &w, Some("main"), "", &is_dir)), vec![new_window("/c")]);
        // a subfolder of an open root is a folder request, not a file inside that window
        let is_dir = dirs(&["/a", "/a/sub"]);
        assert_eq!(targets(&route_open(&[req("/a/sub")], &w, Some("main"), "new", &is_dir)), vec![new_window("/a/sub")]);
        assert_eq!(targets(&route_open(&[req("/a/sub")], &w, Some("main"), "current", &is_dir)), vec![window("main")]);
    }

    #[test]
    fn route_loose_files_to_focused_window_or_one_new_empty_window() {
        let is_dir = dirs(&["/a", "/c"]);
        let w = [win("main", Some("/a")), win("w2", None)];
        assert_eq!(targets(&route_open(&[req("/z/f.ts")], &w, Some("main"), "new", &is_dir)), vec![window("main")]);
        assert_eq!(targets(&route_open(&[req("/z/f.ts")], &w, Some("w2"), "new", &is_dir)), vec![window("w2")], "a focused empty window takes loose files and stays empty");
        // no window at all: loose files share one new empty window; a folder gets its own window regardless of the setting
        let r = route_open(&[req("/z/f.ts"), req("/y/g.ts"), req("/c"), req("/c/h.ts")], &[], None, "current", &is_dir);
        assert_eq!(targets(&r), vec![Target::NewWindow(None), new_window("/c")]);
        assert_eq!(paths(&r[0]), vec!["/z/f.ts", "/y/g.ts"]);
        assert_eq!(paths(&r[1]), vec!["/c", "/c/h.ts"]);
        assert!(route_open(&[], &w, Some("main"), "new", &is_dir).is_empty());
    }

    #[test]
    fn route_groups_several_files_under_one_new_folder() {
        let is_dir = dirs(&["/a", "/c"]);
        let w = [win("main", Some("/a"))];
        let r = route_open(&[req("/c/x.ts"), req("/c"), req_at("/c/y.ts", 3, Some(4)), req("/c"), req("/c/deep/z.ts")], &w, Some("main"), "new", &is_dir);
        // the first file came before its folder and is a loose file at that point: it stays with the focused window
        assert_eq!(targets(&r), vec![window("main"), new_window("/c")]);
        assert_eq!(paths(&r[0]), vec!["/c/x.ts"]);
        assert_eq!(r[1].reqs, vec![req("/c"), req_at("/c/y.ts", 3, Some(4)), req("/c"), req("/c/deep/z.ts")], "one group per new folder, requests in input order");
    }

    #[test]
    fn route_focus_falls_back_to_main_then_the_oldest_window() {
        let is_dir = dirs(&["/a", "/b", "/c"]);
        let w = [win("w2", Some("/b")), win("main", Some("/a"))];
        assert_eq!(targets(&route_open(&[req("/z.ts")], &w, None, "new", &is_dir)), vec![window("main")]);
        assert_eq!(targets(&route_open(&[req("/z.ts")], &w, Some("ghost"), "new", &is_dir)), vec![window("main")], "an unregistered focused label is ignored");
        assert_eq!(targets(&route_open(&[req("/c")], &w, None, "current", &is_dir)), vec![window("main")]);
        let no_main = [win("w3", Some("/b")), win("w2", None)];
        assert_eq!(targets(&route_open(&[req("/z.ts")], &no_main, None, "new", &is_dir)), vec![window("w3")]);
        assert_eq!(targets(&route_open(&[req("/c")], &no_main, Some("w2"), "new", &is_dir)), vec![window("w2")], "focused empty window wins over the oldest");
    }

    #[test]
    fn window_view_from_registry_keeps_creation_order() {
        let registry = WindowRegistry::default();
        registry.register("main"); registry.set_root("main", Some(PathBuf::from("/a")));
        std::thread::sleep(std::time::Duration::from_millis(2));
        registry.register("w2");
        let v = registry_view(&registry);
        assert_eq!(v, vec![win("main", Some("/a")), win("w2", None)]);
        assert!(!v[0].is_empty()); assert!(v[1].is_empty());
    }

    #[test]
    fn open_queue_is_per_label() {
        let q = OpenQueue::default();
        q.push("main", vec![req("/a")]);
        q.push("w2", vec![req("/b")]);
        q.push("main", vec![req_at("/c", 1, None)]);
        q.push("w3", vec![]);
        assert_eq!(q.labels(), vec!["main", "w2"], "an empty push creates no queue");
        assert_eq!(q.pending("main"), 2);
        assert_eq!(q.take("w2"), vec![req("/b")]);
        assert!(q.take("w2").is_empty(), "drained");
        assert!(q.take("nope").is_empty(), "unknown label");
        assert_eq!(q.take("main"), vec![req("/a"), req_at("/c", 1, None)], "order kept across pushes");
        q.push("w4", vec![req("/d")]); q.push("main", vec![req("/e")]);
        q.prune(|l| l == "main");
        assert_eq!(q.labels(), vec!["main"], "queues of destroyed windows are dropped");
        assert_eq!(q.pending("w4"), 0);
    }
}

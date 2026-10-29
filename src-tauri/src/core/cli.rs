//! `caret` command-line launcher.
//!
//! * Parses argv (`caret . src/a.ts:10:5 -g b.ts:3`) and macOS open-documents / `caret://open?...` URLs into
//!   [`OpenRequest`]s, queued in [`OpenQueue`] until the UI drains them (`cli_take_pending_opens`). The UI is
//!   poked with the `caret://open-paths` event and always drains the queue, so each request is delivered once.
//! * Installs the launcher the way VS Code does: the script in `src-tauri/cli/caret.sh` is rendered to
//!   `~/.caret/bin/caret` and symlinked into `/usr/local/bin` when writable, else a user-writable PATH dir
//!   (`~/.local/bin`, Homebrew, `~/bin`), else `/usr/local/bin` through the macOS admin prompt (osascript).
//!   The rendered script is refreshed on every start so a moved or updated app keeps working.
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

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

/// Open requests received before the UI was ready (or while it was busy); drained by `cli_take_pending_opens`.
#[derive(Default)]
pub struct OpenQueue(Mutex<Vec<OpenRequest>>);
impl OpenQueue {
    pub fn push(&self, reqs: Vec<OpenRequest>) { if !reqs.is_empty() { self.0.lock().extend(reqs); } }
    pub fn drain(&self) -> Vec<OpenRequest> { std::mem::take(&mut *self.0.lock()) }
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

/// Parse launcher-style argv (without the program name) relative to `cwd`.
pub fn parse_open_args_in(args: &[String], cwd: &Path) -> Vec<OpenRequest> {
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

/// Called from `setup`: queue argv paths and refresh an already-installed launcher script.
pub fn init(app: &AppHandle) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let reqs = parse_open_args(&args);
    if !reqs.is_empty() { tracing::info!(count = reqs.len(), "cli: queued open requests from argv"); }
    app.state::<OpenQueue>().push(reqs);
    refresh_installed_script(app);
}

/// macOS `RunEvent::Opened`: queue the requests and poke the UI (which drains the queue).
pub fn on_opened(app: &AppHandle, urls: &[tauri::Url]) {
    let reqs = parse_opened_urls(urls);
    tracing::info!(urls = urls.len(), parsed = reqs.len(), "cli: open event");
    if reqs.is_empty() { return; }
    app.state::<OpenQueue>().push(reqs.clone());
    crate::events::emit(app, OPEN_PATHS_EVENT, serde_json::json!({ "paths": reqs }));
    if let Some(w) = app.get_webview_window("main") { let _ = w.show(); let _ = w.set_focus(); }
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
fn write_script(content: &str) -> Result<PathBuf> {
    let path = script_path();
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir).map_err(|e| CaretError::io(format!("create {}: {e}", dir.display())))?; }
    let same = std::fs::read_to_string(&path).map(|c| c == content).unwrap_or(false);
    if !same { std::fs::write(&path, content).map_err(|e| CaretError::io(format!("write {}: {e}", path.display())))?; }
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)); }
    Ok(path)
}

fn refresh_installed_script(app: &AppHandle) {
    let path = script_path();
    if !path.exists() { return; }
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

#[tauri::command]
pub fn cli_take_pending_opens(q: State<'_, OpenQueue>) -> Result<Vec<OpenRequest>> { Ok(q.drain()) }

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
        let rendered = render_script("dev.caret.editor", "/Applications/Caret.app", "9.9.9");
        assert!(!rendered.contains("__CARET_"), "all placeholders substituted");
        std::fs::write(&script, rendered).unwrap();
        let out = std::process::Command::new("sh").arg(&script).args([".", "foo.ts:12:3", "-g", "b c.ts:4", "-r"]).current_dir(&root).env("CARET_CLI_DRY_RUN", "1").output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
        let r = root.to_string_lossy();
        assert_eq!(stdout.lines().next().unwrap(), format!("open -b dev.caret.editor {r}"));
        assert!(stdout.contains(&format!("open -b dev.caret.editor caret://open?path={r}/foo.ts&line=12&col=3")), "{stdout}");
        assert!(stdout.contains(&format!("caret://open?path={r}/b%20c.ts&line=4")), "{stdout}");
        // the goto URL round-trips through the URL parser
        let url = stdout.lines().find(|l| l.contains("foo.ts")).unwrap().trim_start_matches("open -b dev.caret.editor ").to_string();
        let parsed = parse_opened_urls(&[tauri::Url::parse(&url).unwrap()]);
        assert_eq!(parsed[0], OpenRequest { path: root.join("foo.ts").to_string_lossy().to_string(), line: Some(12), col: Some(3) });
        // version / usage / unknown flag
        let v = std::process::Command::new("sh").arg(&script).arg("-v").output().unwrap();
        assert_eq!(String::from_utf8_lossy(&v.stdout).trim(), "caret 9.9.9");
        let bad = std::process::Command::new("sh").arg(&script).arg("--bogus").output().unwrap();
        assert_eq!(bad.status.code(), Some(2));
        let missing = std::process::Command::new("sh").arg(&script).arg("missing.ts").current_dir(&root).env("CARET_CLI_DRY_RUN", "1").output().unwrap();
        assert_eq!(missing.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&missing.stderr).contains("no such file"));
    }

    #[test]
    fn quoting_helpers() {
        assert_eq!(shell_single_quote("a'b"), "'a'\\''b'");
        assert_eq!(applescript_quote("say \"hi\" \\ there"), "\"say \\\"hi\\\" \\\\ there\"");
    }
}

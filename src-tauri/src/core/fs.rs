//! File system commands + the quick-open fuzzy path index.
use crate::core::generation::Generation;
use crate::core::settings::SettingsState;
use crate::core::workspace::{WorkspaceInfo, WorkspaceState};
use crate::error::{CaretError, Result};
use crate::paths;
use base64::Engine;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::{BinaryHeap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Listener, Manager, State};

// ───────────────────────── types ─────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileKind { File, Dir, Symlink, Other }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub kind: FileKind,
    #[serde(skip_serializing_if = "Option::is_none")] pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub mtime: Option<u64>,
    pub is_hidden: bool,
    pub is_ignored: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub has_children: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub is_symlink: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    pub path: String,
    pub content: String,
    pub encoding: &'static str,
    pub size: u64,
    pub mtime: u64,
    pub truncated: bool,
    pub is_binary: bool,
    pub line_ending: &'static str,
    pub readonly: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStat { pub path: String, pub kind: FileKind, pub size: u64, pub mtime: u64, pub readonly: bool, pub exists: bool }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteResult { pub path: String, pub mtime: u64, pub size: u64 }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickOpenResult { pub path: String, pub rel_path: String, pub score: i32, #[serde(skip_serializing_if = "Option::is_none")] pub indices: Option<Vec<u32>>, /// Lives in a git-ignored (or .caretignore'd) directory: still openable, ranked below regular files.
    pub ignored: bool }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageData { pub data_base64: String, pub mime: String }

// ───────────────────────── helpers ─────────────────────────

pub fn mtime_ms(md: &std::fs::Metadata) -> u64 {
    md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn kind_of(md: &std::fs::Metadata) -> FileKind {
    let ft = md.file_type();
    if ft.is_dir() { FileKind::Dir } else if ft.is_file() { FileKind::File } else if ft.is_symlink() { FileKind::Symlink } else { FileKind::Other }
}

fn resolve(app: &AppHandle, p: &str) -> PathBuf {
    let root = app.state::<WorkspaceState>().root();
    paths::resolve(root.as_deref(), p)
}

fn is_readonly(md: &std::fs::Metadata) -> bool { md.permissions().readonly() }

const DEFAULT_READ_CAP: u64 = 20 * 1024 * 1024;

/// Build gitignore matchers covering `dir`: root-level ignore files + nested .gitignore files between root and dir.
struct IgnoreSet { matchers: Vec<ignore::gitignore::Gitignore> }
impl IgnoreSet {
    fn build(root: Option<&Path>, dir: &Path, global: &[String]) -> Self {
        let mut matchers = vec![];
        let base = root.filter(|r| dir.starts_with(r)).unwrap_or(dir);
        // ancestors from base down to dir (inclusive)
        let mut chain: Vec<PathBuf> = vec![];
        let mut cur = Some(dir.to_path_buf());
        while let Some(c) = cur { chain.push(c.clone()); if c == base { break; } cur = c.parent().map(Path::to_path_buf); }
        for d in chain.iter().rev() {
            let mut b = ignore::gitignore::GitignoreBuilder::new(d);
            let mut any = false;
            for name in [".gitignore", ".caretignore", ".cursorignore"] { let f = d.join(name); if f.is_file() { b.add(f); any = true; } }
            if d == base {
                let excl = d.join(".git").join("info").join("exclude"); if excl.is_file() { b.add(excl); any = true; }
                for g in global { if b.add_line(None, g).is_ok() { any = true; } }
            }
            if any { if let Ok(gi) = b.build() { matchers.push(gi); } }
        }
        Self { matchers }
    }
    fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        if path.file_name().map(|n| n == ".git").unwrap_or(false) { return true; }
        // deepest matcher wins (iterate in reverse)
        for m in self.matchers.iter().rev() {
            match m.matched_path_or_any_parents(path, is_dir) {
                ignore::Match::Ignore(_) => return true,
                ignore::Match::Whitelist(_) => return false,
                ignore::Match::None => {}
            }
        }
        false
    }
}

fn detect_line_ending(s: &str) -> &'static str {
    let mut crlf = 0usize; let mut lf = 0usize;
    let b = s.as_bytes();
    for (i, &c) in b.iter().enumerate() { if c == b'\n' { if i > 0 && b[i - 1] == b'\r' { crlf += 1 } else { lf += 1 } } }
    if crlf > 0 && lf > 0 { "mixed" } else if crlf > 0 { "crlf" } else { "lf" }
}

/// Decode bytes → (text, encoding, is_binary).
pub fn decode_bytes(buf: &[u8]) -> (String, &'static str, bool) {
    use content_inspector::ContentType;
    let probe = &buf[..buf.len().min(8192)];
    match content_inspector::inspect(probe) {
        ContentType::BINARY => (String::new(), "binary", true),
        ContentType::UTF_16LE => { let (s, _, _) = encoding_rs::UTF_16LE.decode(buf); (s.into_owned(), "utf16le", false) }
        ContentType::UTF_16BE => { let (s, _, _) = encoding_rs::UTF_16BE.decode(buf); (s.into_owned(), "utf16le", false) }
        _ => match std::str::from_utf8(buf) {
            Ok(s) => { let s = s.strip_prefix('\u{feff}').unwrap_or(s); (s.to_string(), "utf8", false) }
            Err(_) => { let (s, _, _) = encoding_rs::WINDOWS_1252.decode(buf); (s.into_owned(), "latin1", false) }
        },
    }
}

/// Atomic write: temp file in the same directory + rename; preserves the original mode.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| CaretError::invalid("invalid path"))?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    let tmp = parent.join(format!(".{name}.caret-{}.tmp", std::process::id()));
    let existing_mode = std::fs::metadata(path).ok().map(|m| m.permissions());
    std::fs::write(&tmp, bytes).map_err(|e| { let _ = std::fs::remove_file(&tmp); CaretError::from(e) })?;
    if let Some(p) = existing_mode { let _ = std::fs::set_permissions(&tmp, p); }
    std::fs::rename(&tmp, path).map_err(|e| { let _ = std::fs::remove_file(&tmp); CaretError::from(e) })?;
    Ok(())
}

fn copy_recursive(from: &Path, to: &Path) -> Result<()> {
    let md = std::fs::metadata(from)?;
    if md.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)? { let e = e?; copy_recursive(&e.path(), &to.join(e.file_name()))?; }
    } else {
        if let Some(p) = to.parent() { std::fs::create_dir_all(p)?; }
        std::fs::copy(from, to)?;
    }
    Ok(())
}

// ───────────────────────── commands ─────────────────────────

pub fn read_dir_impl(dir: &Path, root: Option<&Path>, global: &[String], show_hidden: bool) -> Result<Vec<DirEntry>> {
    let ig = IgnoreSet::build(root, dir, global);
    let mut out = vec![];
    for e in std::fs::read_dir(dir)? {
        let e = match e { Ok(e) => e, Err(_) => continue };
        let name = e.file_name().to_string_lossy().to_string();
        let is_hidden = paths::is_hidden(&name);
        if is_hidden && !show_hidden { continue; }
        let p = e.path();
        let link_md = e.metadata().ok();
        let is_symlink = link_md.as_ref().map(|m| m.file_type().is_symlink()).unwrap_or(false);
        let md = if is_symlink { std::fs::metadata(&p).ok().or(link_md) } else { link_md };
        let Some(md) = md else { continue };
        let kind = if md.is_dir() { FileKind::Dir } else if md.is_file() { FileKind::File } else if is_symlink { FileKind::Symlink } else { FileKind::Other };
        let is_dir = kind == FileKind::Dir;
        let has_children = if is_dir { Some(std::fs::read_dir(&p).map(|mut r| r.next().is_some()).unwrap_or(false)) } else { None };
        out.push(DirEntry {
            name, path: p.to_string_lossy().to_string(), kind,
            size: if is_dir { None } else { Some(md.len()) },
            mtime: Some(mtime_ms(&md)),
            is_hidden, is_ignored: ig.is_ignored(&p, is_dir), has_children,
            is_symlink: if is_symlink { Some(true) } else { None },
        });
    }
    out.sort_by(|a, b| {
        let da = a.kind == FileKind::Dir; let db = b.kind == FileKind::Dir;
        db.cmp(&da).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

#[tauri::command]
pub async fn fs_read_dir(app: AppHandle, path: String, show_hidden: Option<bool>) -> Result<Vec<DirEntry>> {
    let dir = resolve(&app, &path);
    let root = app.state::<WorkspaceState>().root();
    let global = app.state::<SettingsState>().effective().general.global_ignore;
    let show_hidden = show_hidden.unwrap_or(true);
    tokio::task::spawn_blocking(move || read_dir_impl(&dir, root.as_deref(), &global, show_hidden)).await.map_err(|e| CaretError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn fs_read_file(app: AppHandle, path: String, max_bytes: Option<u64>) -> Result<FileContent> {
    let p = resolve(&app, &path);
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        let md = std::fs::metadata(&p)?;
        if md.is_dir() { return Err(CaretError::invalid(format!("{} is a directory", p.display()))); }
        let cap = max_bytes.unwrap_or(DEFAULT_READ_CAP);
        let size = md.len();
        let to_read = size.min(cap) as usize;
        let mut buf = Vec::with_capacity(to_read);
        std::fs::File::open(&p)?.take(to_read as u64).read_to_end(&mut buf)?;
        let (content, encoding, is_binary) = decode_bytes(&buf);
        let line_ending = detect_line_ending(&content);
        Ok(FileContent { path: p.to_string_lossy().to_string(), content, encoding, size, mtime: mtime_ms(&md), truncated: size > cap, is_binary, line_ending, readonly: is_readonly(&md) })
    }).await.map_err(|e| CaretError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn fs_write_file(app: AppHandle, path: String, content: String, create_dirs: Option<bool>, line_ending: Option<String>) -> Result<WriteResult> {
    let p = resolve(&app, &path);
    tokio::task::spawn_blocking(move || {
        if create_dirs.unwrap_or(true) { if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; } }
        let bytes: Vec<u8> = match line_ending.as_deref() {
            Some("crlf") => content.replace("\r\n", "\n").replace('\n', "\r\n").into_bytes(),
            Some("lf") => content.replace("\r\n", "\n").into_bytes(),
            _ => content.into_bytes(),
        };
        write_atomic(&p, &bytes)?;
        let md = std::fs::metadata(&p)?;
        Ok(WriteResult { path: p.to_string_lossy().to_string(), mtime: mtime_ms(&md), size: md.len() })
    }).await.map_err(|e| CaretError::internal(e.to_string()))?
}

#[tauri::command(async)]
pub fn fs_stat(app: AppHandle, path: String) -> Result<FileStat> {
    let p = resolve(&app, &path);
    let p = std::fs::canonicalize(&p).unwrap_or(p);
    match std::fs::metadata(&p) {
        Ok(md) => Ok(FileStat { path: p.to_string_lossy().to_string(), kind: kind_of(&md), size: md.len(), mtime: mtime_ms(&md), readonly: is_readonly(&md), exists: true }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileStat { path: p.to_string_lossy().to_string(), kind: FileKind::Other, size: 0, mtime: 0, readonly: false, exists: false }),
        Err(e) => Err(e.into()),
    }
}

#[tauri::command(async)]
pub fn fs_exists(app: AppHandle, path: String) -> Result<bool> { Ok(resolve(&app, &path).exists()) }

#[tauri::command(async)]
pub fn fs_create(app: AppHandle, path: String, kind: String) -> Result<()> {
    let p = resolve(&app, &path);
    if kind == "dir" { std::fs::create_dir_all(&p)?; }
    else {
        if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
        std::fs::OpenOptions::new().write(true).create_new(true).open(&p).map_err(|e| if e.kind() == std::io::ErrorKind::AlreadyExists { CaretError::new("already_exists", format!("{} already exists", p.display())) } else { e.into() })?;
    }
    index_add(&app, &p);
    Ok(())
}

#[tauri::command(async)]
pub fn fs_rename(app: AppHandle, from: String, to: String) -> Result<()> {
    let f = resolve(&app, &from); let t = resolve(&app, &to);
    if t.exists() && f != t { return Err(CaretError::new("already_exists", format!("{} already exists", t.display()))); }
    if let Some(parent) = t.parent() { std::fs::create_dir_all(parent)?; }
    std::fs::rename(&f, &t)?;
    index_remove(&app, &f); index_add(&app, &t);
    Ok(())
}

#[tauri::command]
pub async fn fs_delete(app: AppHandle, paths: Vec<String>, trash: Option<bool>) -> Result<()> {
    let ps: Vec<PathBuf> = paths.iter().map(|p| resolve(&app, p)).collect();
    let app2 = app.clone();
    let use_trash = trash.unwrap_or(true);
    tokio::task::spawn_blocking(move || {
        if use_trash {
            if let Err(e) = trash::delete_all(&ps) {
                tracing::warn!(error = %e, "trash failed; falling back to permanent delete");
                for p in &ps { remove_permanently(p)?; }
            }
        } else { for p in &ps { remove_permanently(p)?; } }
        for p in &ps { index_remove(&app2, p); }
        Ok(())
    }).await.map_err(|e| CaretError::internal(e.to_string()))?
}

fn remove_permanently(p: &Path) -> Result<()> {
    match std::fs::symlink_metadata(p) {
        Ok(md) if md.is_dir() => Ok(std::fs::remove_dir_all(p)?),
        Ok(_) => Ok(std::fs::remove_file(p)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[tauri::command]
pub async fn fs_copy(app: AppHandle, from: String, to: String) -> Result<()> {
    let f = resolve(&app, &from); let t = resolve(&app, &to);
    if t.exists() { return Err(CaretError::new("already_exists", format!("{} already exists", t.display()))); }
    let app2 = app.clone();
    tokio::task::spawn_blocking(move || { copy_recursive(&f, &t)?; index_add(&app2, &t); Ok(()) }).await.map_err(|e| CaretError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn fs_move(app: AppHandle, from: String, to: String) -> Result<()> {
    let f = resolve(&app, &from); let t = resolve(&app, &to);
    if t.exists() { return Err(CaretError::new("already_exists", format!("{} already exists", t.display()))); }
    let app2 = app.clone();
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = t.parent() { std::fs::create_dir_all(parent)?; }
        if std::fs::rename(&f, &t).is_err() { copy_recursive(&f, &t)?; remove_permanently(&f)?; }
        index_remove(&app2, &f); index_add(&app2, &t);
        Ok(())
    }).await.map_err(|e| CaretError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn fs_read_image(app: AppHandle, path: String) -> Result<ImageData> {
    let p = resolve(&app, &path);
    tokio::task::spawn_blocking(move || {
        let md = std::fs::metadata(&p)?;
        if md.len() > 50 * 1024 * 1024 { return Err(CaretError::invalid("image larger than 50 MiB")); }
        let bytes = std::fs::read(&p)?;
        let mime = mime_guess::from_path(&p).first_or_octet_stream().essence_str().to_string();
        Ok(ImageData { data_base64: base64::engine::general_purpose::STANDARD.encode(bytes), mime })
    }).await.map_err(|e| CaretError::internal(e.to_string()))?
}

// ───────────────────────── quick-open index ─────────────────────────
//
// Two phases. Tier 1 walks the tree honouring ignore files and is published the moment it finishes; it is the only
// thing Quick Open, @-mention pickers and the agent's file list may ever wait for. Tier 2 walks again with ignore
// files off (heavy-directory skip list still on) to surface files that only the ignore rules hide, and merges its
// results into the published index from the same background thread. Both walks stop at an entry cap, a wall-clock
// budget, or when the workspace changes under them (generation ticket); a cap/budget stop publishes a `partial` list.

pub const INDEX_CAP: usize = 200_000;

struct Entry { rel: String, lower: String, ignored: bool }

#[derive(Default)]
pub struct FileIndexState {
    root: RwLock<Option<PathBuf>>,
    entries: RwLock<Vec<Entry>>,
    /// Tier 1 is published for `root`.
    ready: AtomicBool,
    /// Tier 1 stopped at its cap or budget: the list is a prefix of the tree.
    tracked_partial: AtomicBool,
    /// Tier 2 has merged (or was skipped) for `root`.
    ignored_done: AtomicBool,
    ignored_partial: AtomicBool,
    /// Serialises tier-1 builds; a query that finds the index stale waits here, and only here.
    build_lock: Mutex<()>,
    /// Bumped per workspace change so a walk in progress for the previous root stops and discards its results.
    generation: Generation,
}

/// What the UI needs to label results ("showing the first N files").
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickOpenIndexStatus { pub root: Option<String>, pub ready: bool, pub files: usize, pub ignored_files: usize, pub partial: bool, pub ignored_done: bool, pub ignored_partial: bool }

impl FileIndexState {
    pub fn root(&self) -> Option<PathBuf> { self.root.read().clone() }
    pub fn len(&self) -> usize { self.entries.read().len() }
    pub fn status(&self) -> QuickOpenIndexStatus {
        let e = self.entries.read();
        let ignored = e.iter().filter(|x| x.ignored).count();
        QuickOpenIndexStatus { root: self.root().map(|r| r.to_string_lossy().to_string()), ready: self.ready.load(Ordering::Acquire), files: e.len() - ignored, ignored_files: ignored, partial: self.tracked_partial.load(Ordering::Acquire), ignored_done: self.ignored_done.load(Ordering::Acquire), ignored_partial: self.ignored_partial.load(Ordering::Acquire) }
    }
}

/// Stop conditions for a walk: an entry cap, a wall-clock deadline, and a generation ticket that goes stale when
/// the workspace changes mid-walk.
#[derive(Clone, Copy)]
pub struct WalkLimits<'a> { pub cap: usize, pub deadline: Option<Instant>, pub ticket: Option<(&'a Generation, u64)> }
impl WalkLimits<'static> {
    pub fn cap_only(cap: usize) -> Self { Self { cap, deadline: None, ticket: None } }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct WalkOutcome {
    /// Sorted workspace-relative paths.
    pub files: Vec<String>,
    /// The cap or the deadline stopped the walk: `files` is incomplete.
    pub partial: bool,
    /// The workspace changed mid-walk: `files` belongs to a root that is no longer open and must be discarded.
    pub stale: bool,
}

/// Drive a parallel walk under `limits`, collecting regular files (skipping `known` paths) relative to `root`.
fn run_walk(wb: ignore::WalkBuilder, root: &Path, limits: &WalkLimits<'_>, known: Option<&HashSet<String>>) -> WalkOutcome {
    let out = Mutex::new(Vec::<String>::new());
    let stop = AtomicBool::new(false);
    let partial = AtomicBool::new(false);
    let stale = AtomicBool::new(false);
    wb.build_parallel().run(|| {
        Box::new(|res| {
            if stop.load(Ordering::Relaxed) { return ignore::WalkState::Quit; }
            if let Some(dl) = limits.deadline { if Instant::now() >= dl { partial.store(true, Ordering::Relaxed); stop.store(true, Ordering::Relaxed); return ignore::WalkState::Quit; } }
            if let Some((g, t)) = limits.ticket { if !g.is_current(t) { stale.store(true, Ordering::Relaxed); stop.store(true, Ordering::Relaxed); return ignore::WalkState::Quit; } }
            let Ok(e) = res else { return ignore::WalkState::Continue };
            if !e.file_type().map(|t| t.is_file()).unwrap_or(false) { return ignore::WalkState::Continue; }
            let rel = paths::rel_to(root, e.path());
            if known.map(|k| k.contains(&rel)).unwrap_or(false) { return ignore::WalkState::Continue; }
            let mut g = out.lock();
            if g.len() >= limits.cap { partial.store(true, Ordering::Relaxed); stop.store(true, Ordering::Relaxed); return ignore::WalkState::Quit; }
            g.push(rel);
            ignore::WalkState::Continue
        })
    });
    let mut files = out.into_inner();
    files.sort_unstable();
    WalkOutcome { files, partial: partial.load(Ordering::Relaxed), stale: stale.load(Ordering::Relaxed) }
}

/// Tier 1: walk the workspace honouring ignore files + settings and collect relative file paths.
pub fn collect_workspace_files(root: &Path, global_ignore: &[String], exclude_globs: &[String], limits: &WalkLimits<'_>) -> WalkOutcome {
    let mut wb = ignore::WalkBuilder::new(root);
    wb.hidden(false).git_ignore(true).git_global(true).git_exclude(true).ignore(true).parents(true).require_git(false).follow_links(false).same_file_system(false)
        .threads(std::thread::available_parallelism().map(|n| n.get().min(8)).unwrap_or(4));
    wb.add_custom_ignore_filename(".caretignore");
    wb.add_custom_ignore_filename(".cursorignore");
    let mut ob = ignore::overrides::OverrideBuilder::new(root);
    for g in global_ignore.iter().chain(exclude_globs.iter()) {
        let g = g.trim(); if g.is_empty() { continue; }
        let pat = if let Some(rest) = g.strip_prefix('!') { rest.to_string() } else { format!("!{g}") };
        let _ = ob.add(&pat);
    }
    if let Ok(ov) = ob.build() { wb.overrides(ov); }
    wb.filter_entry(|e| !(e.file_type().map(|t| t.is_dir()).unwrap_or(false) && e.file_name() == ".git"));
    run_walk(wb, root, limits, None)
}

/// Directories never surfaced even as low-priority "ignored" results: generated output, dependency trees and tool state.
/// Bare names match at any depth (`website/node_modules`); entries with a slash are anchored to the workspace root.
/// This list is what keeps the second-tier walk cheap on a repository whose `target/` holds tens of thousands of files.
pub const IGNORED_TIER_SKIP: &[&str] = &[".git", ".hg", ".svn", ".caret", "node_modules", "bower_components", "__pycache__", ".venv", "venv", ".next", ".nuxt", ".turbo", ".cache", ".pytest_cache", ".mypy_cache", ".gradle", ".idea", "target", "dist", "build", "out", "coverage", ".terraform", ".tox", ".parcel-cache", ".yarn", ".pnpm-store", "Pods", "DerivedData", ".dart_tool", "vendor/bundle"];
pub const IGNORED_CAP: usize = 50_000;
/// Default wall-clock budgets; `indexing.quickOpenBudgetMs` / `indexing.quickOpenIgnoredBudgetMs` override them.
pub const DEFAULT_TRACKED_BUDGET_MS: u64 = 8_000;
pub const DEFAULT_IGNORED_BUDGET_MS: u64 = 1_500;

/// Tier 2: files that only the ignore rules hide (a repo's own `tmp/`, scratch dirs, logs): a second walk with ignore
/// files off but the heavy-directory skip list, settings excludes and global ignores still applied. `known` are the
/// first-tier paths.
pub fn collect_ignored_files(root: &Path, global_ignore: &[String], exclude_globs: &[String], known: &HashSet<String>, limits: &WalkLimits<'_>) -> WalkOutcome {
    let mut wb = ignore::WalkBuilder::new(root);
    wb.hidden(false).git_ignore(false).git_global(false).git_exclude(false).ignore(false).parents(false).require_git(false).follow_links(false).same_file_system(false)
        .threads(std::thread::available_parallelism().map(|n| n.get().min(8)).unwrap_or(4));
    let mut ob = ignore::overrides::OverrideBuilder::new(root);
    for g in IGNORED_TIER_SKIP.iter().map(|s| s.to_string()).chain(global_ignore.iter().cloned()).chain(exclude_globs.iter().cloned()) {
        let g = g.trim(); if g.is_empty() { continue; }
        let pat = if let Some(rest) = g.strip_prefix('!') { rest.to_string() } else { format!("!{g}") };
        let _ = ob.add(&pat);
        if !pat.contains('/') && !pat.contains('*') { let _ = ob.add(&format!("{pat}/**")); }
    }
    if let Ok(ov) = ob.build() { wb.overrides(ov); }
    wb.filter_entry(|e| { let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false); let n = e.file_name().to_string_lossy(); !(is_dir && IGNORED_TIER_SKIP.iter().any(|s| *s == n.as_ref())) });
    run_walk(wb, root, limits, Some(known))
}

fn budget(ms: u64, default_ms: u64) -> Duration { Duration::from_millis(if ms == 0 { default_ms } else { ms }) }

/// Tier 1 under `build_lock`: builds (or finds already built) the ignore-aware index for the current root and publishes
/// it. Returns `false` when there is no root or the workspace changed meanwhile. This is the only wait a caller sees.
fn ensure_tracked(app: &AppHandle, ticket: u64) -> bool {
    let st = app.state::<FileIndexState>();
    let t_lock = Instant::now();
    let _g = st.build_lock.lock();
    let lock_wait_ms = t_lock.elapsed().as_millis() as u64;
    if lock_wait_ms > 1000 { tracing::warn!(lock_wait_ms, "quick-open index build waited for a previous build to finish"); }
    if !st.generation.is_current(ticket) { return false; }
    let Some(root) = app.state::<WorkspaceState>().root() else { *st.root.write() = None; st.entries.write().clear(); st.ready.store(false, Ordering::Release); return false; };
    // The listener thread and a query racing on a folder switch both arrive here; the second finds the first's index.
    if st.ready.load(Ordering::Acquire) && st.root().as_deref() == Some(root.as_path()) { return true; }
    let settings = app.state::<SettingsState>().effective();
    let t0 = Instant::now();
    let limits = WalkLimits { cap: INDEX_CAP, deadline: Some(t0 + budget(settings.indexing.quick_open_budget_ms, DEFAULT_TRACKED_BUDGET_MS)), ticket: Some((&st.generation, ticket)) };
    let out = collect_workspace_files(&root, &settings.general.global_ignore, &settings.indexing.exclude_globs, &limits);
    if out.stale { tracing::debug!(root = %root.display(), "quick-open index build superseded by a newer workspace"); return false; }
    let n = out.files.len();
    let entries: Vec<Entry> = out.files.into_iter().map(|rel| Entry { lower: rel.to_lowercase(), rel, ignored: false }).collect();
    {
        // Publish atomically with respect to readers: root, entries, flags, then `ready`.
        let mut e = st.entries.write();
        *st.root.write() = Some(root.clone());
        *e = entries;
        st.tracked_partial.store(out.partial, Ordering::Release);
        st.ignored_done.store(false, Ordering::Release);
        st.ignored_partial.store(false, Ordering::Release);
        st.ready.store(true, Ordering::Release);
    }
    tracing::info!(files = n, partial = out.partial, lock_wait_ms, ms = t0.elapsed().as_millis() as u64, "quick-open index built");
    true
}

/// Tier 2, never on a caller's path: walk the ignore-only files and merge them into the published index. Skipped
/// when the workspace changed, and discarded if it changes during the walk.
fn merge_ignored(app: &AppHandle, ticket: u64) {
    let st = app.state::<FileIndexState>();
    if !st.generation.is_current(ticket) { return; }
    let Some(root) = st.root() else { return };
    let settings = app.state::<SettingsState>().effective();
    let known: HashSet<String> = st.entries.read().iter().map(|e| e.rel.clone()).collect();
    let t0 = Instant::now();
    let limits = WalkLimits { cap: IGNORED_CAP, deadline: Some(t0 + budget(settings.indexing.quick_open_ignored_budget_ms, DEFAULT_IGNORED_BUDGET_MS)), ticket: Some((&st.generation, ticket)) };
    let out = collect_ignored_files(&root, &settings.general.global_ignore, &settings.indexing.exclude_globs, &known, &limits);
    if out.stale || !st.generation.is_current(ticket) { return; }
    let n = out.files.len();
    {
        let mut e = st.entries.write();
        if st.root().as_deref() != Some(root.as_path()) { return; }
        let extra = out.files.into_iter().map(|rel| Entry { lower: rel.to_lowercase(), rel, ignored: true });
        *e = merge_sorted(std::mem::take(&mut *e), extra);
        st.ignored_partial.store(out.partial, Ordering::Release);
        st.ignored_done.store(true, Ordering::Release);
    }
    tracing::info!(ignored = n, partial = out.partial, ms = t0.elapsed().as_millis() as u64, "quick-open ignored tier merged");
}

/// Merge two `rel`-sorted lists; on a duplicate path the existing entry wins (the watcher may have inserted it since).
fn merge_sorted(base: Vec<Entry>, extra: impl Iterator<Item = Entry>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::with_capacity(base.len() + extra.size_hint().0);
    let mut base = base.into_iter().peekable();
    let mut extra = extra.peekable();
    loop {
        match (base.peek(), extra.peek()) {
            (Some(b), Some(x)) => match b.rel.cmp(&x.rel) {
                std::cmp::Ordering::Less => out.push(base.next().unwrap()),
                std::cmp::Ordering::Greater => out.push(extra.next().unwrap()),
                std::cmp::Ordering::Equal => { out.push(base.next().unwrap()); extra.next(); }
            },
            (Some(_), None) => out.push(base.next().unwrap()),
            (None, Some(_)) => out.push(extra.next().unwrap()),
            (None, None) => break,
        }
    }
    out
}

/// Make sure tier 1 is published for the current root (waiting for it if a build is in progress). Joins the current
/// generation rather than starting one, so it never cancels the listener's build.
fn ensure_index(app: &AppHandle) {
    let st = app.state::<FileIndexState>();
    let ws_root = app.state::<WorkspaceState>().root();
    if st.ready.load(Ordering::Acquire) && st.root() == ws_root { return; }
    let ticket = st.generation.current();
    ensure_tracked(app, ticket);
}

/// Called by the watcher / fs ops: add a file to the index if it lives in the workspace.
pub fn index_add(app: &AppHandle, path: &Path) {
    let st = app.state::<FileIndexState>();
    if !st.ready.load(Ordering::Acquire) { return; }
    let Some(root) = st.root() else { return };
    if !path.starts_with(&root) || !path.is_file() { return; }
    let rel = paths::rel_to(&root, path);
    if rel.split('/').any(|seg| IGNORED_TIER_SKIP.contains(&seg)) { return; }
    // Classify like the builder would, so a watcher-added file in `tmp/` ranks as ignored instead of jumping the queue.
    let global = app.state::<SettingsState>().effective().general.global_ignore.clone();
    let ignored = path.parent().map(|d| IgnoreSet::build(Some(&root), d, &global).is_ignored(path, false)).unwrap_or(false);
    let mut e = st.entries.write();
    if e.len() >= INDEX_CAP + IGNORED_CAP { return; }
    match e.binary_search_by(|x| x.rel.as_str().cmp(rel.as_str())) { Ok(_) => {}, Err(pos) => e.insert(pos, Entry { lower: rel.to_lowercase(), rel, ignored }) }
}

/// Remove a path (file or directory subtree) from the index.
pub fn index_remove(app: &AppHandle, path: &Path) {
    let st = app.state::<FileIndexState>();
    if !st.ready.load(Ordering::Acquire) { return; }
    let Some(root) = st.root() else { return };
    if !path.starts_with(&root) { return; }
    let rel = paths::rel_to(&root, path);
    let prefix = format!("{rel}/");
    st.entries.write().retain(|e| e.rel != rel && !e.rel.starts_with(&prefix));
}

pub fn init(app: &AppHandle) {
    let h = app.clone();
    app.listen(crate::events::WORKSPACE_CHANGED, move |_ev| {
        let h2 = h.clone();
        let info: Option<WorkspaceInfo> = serde_json::from_str(_ev.payload()).ok().flatten();
        // New generation: any walk still running for the previous root stops and discards its results. Mark the index
        // stale before the build starts so queries wait for the new root (and re-opening the same folder still
        // refreshes); the build itself runs off the emitting thread, tier 1 first, tier 2 merged afterwards.
        let st = h2.state::<FileIndexState>();
        let ticket = st.generation.begin();
        st.ready.store(false, Ordering::Release);
        if info.is_none() { return; }
        std::thread::Builder::new().name("caret-file-index".into()).spawn(move || { if ensure_tracked(&h2, ticket) { merge_ignored(&h2, ticket); } }).ok();
    });
}

// ───────────────────────── fuzzy matching ─────────────────────────

/// Subsequence fuzzy match (fzf-like): returns (score, indices) or None.
pub fn fuzzy_match(query_lower: &[char], cand: &str, cand_lower: &[char]) -> Option<(i32, Vec<u32>)> {
    if query_lower.is_empty() { return Some((0, vec![])); }
    if cand_lower.len() < query_lower.len() { return None; }
    // forward greedy pass
    let mut qi = 0usize; let mut first_end = None;
    for (i, &c) in cand_lower.iter().enumerate() { if c == query_lower[qi] { qi += 1; if qi == query_lower.len() { first_end = Some(i); break; } } }
    let end = first_end?;
    // backward pass: tighten the window from the end
    let mut positions = vec![0usize; query_lower.len()];
    let mut qi = query_lower.len();
    let mut i = end as isize;
    while qi > 0 && i >= 0 { if cand_lower[i as usize] == query_lower[qi - 1] { qi -= 1; positions[qi] = i as usize; } i -= 1; }
    // score
    let chars: Vec<char> = cand.chars().collect();
    let basename_start = cand.rfind('/').map(|p| cand[..p].chars().count() + 1).unwrap_or(0);
    let mut score: i32 = 0;
    let mut prev: Option<usize> = None;
    for &p in &positions {
        score += 16;
        let in_basename = p >= basename_start;
        let after_sep = p > 0 && cand_lower[p - 1] == '/';
        let word_boundary = p == 0 || matches!(cand_lower[p - 1], '_' | '-' | '.' | ' ') || (p > 0 && chars[p - 1].is_lowercase() && chars[p].is_uppercase());
        if p == basename_start { score += 48; } else if after_sep { score += 20; } else if word_boundary { score += 16; }
        if in_basename { score += 12; }
        if let Some(pp) = prev { if p == pp + 1 { score += 24; } else { let gap = (p - pp - 1) as i32; score -= (3 + (gap - 1)).min(12); } }
        prev = Some(p);
    }
    // whole query contiguous inside basename
    if positions.len() > 1 && positions.windows(2).all(|w| w[1] == w[0] + 1) && positions[0] >= basename_start { score += 40; }
    score -= (cand_lower.len() as i32) / 8;
    Some((score, positions.into_iter().map(|p| p as u32).collect()))
}

/// Score handicap for ignored-tier files: a strong match in `tmp/` still beats a weak one elsewhere, but ties go to tracked files.
const IGNORED_PENALTY: i32 = 80;

#[derive(PartialEq, Eq)]
struct Scored { score: i32, idx: usize }
impl Ord for Scored { fn cmp(&self, o: &Self) -> std::cmp::Ordering { o.score.cmp(&self.score).then_with(|| self.idx.cmp(&o.idx)) } }
impl PartialOrd for Scored { fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(o)) } }

#[tauri::command]
pub async fn fs_quick_open(app: AppHandle, query: String, limit: Option<usize>) -> Result<Vec<QuickOpenResult>> {
    let limit = limit.unwrap_or(50).clamp(1, 500);
    tokio::task::spawn_blocking(move || {
        ensure_index(&app);
        let st = app.state::<FileIndexState>();
        let Some(root) = st.root() else { return Ok(vec![]) };
        let q = query.trim();
        // strip ":line[:col]" and "#..." suffixes
        let q = q.split(':').next().unwrap_or(q).split('#').next().unwrap_or(q);
        let q: String = q.chars().filter(|c| !c.is_whitespace()).collect();
        let ql: Vec<char> = q.to_lowercase().chars().collect();
        let entries = st.entries.read();
        let mut heap: BinaryHeap<Scored> = BinaryHeap::with_capacity(limit + 1);
        if ql.is_empty() {
            return Ok(entries.iter().filter(|e| !e.ignored).take(limit).map(|e| QuickOpenResult { path: root.join(&e.rel).to_string_lossy().to_string(), rel_path: e.rel.clone(), score: 0, indices: None, ignored: false }).collect());
        }
        let mut scores: Vec<(usize, i32, Vec<u32>)> = Vec::new();
        for (i, e) in entries.iter().enumerate() {
            let lower: Vec<char> = e.lower.chars().collect();
            if let Some((s, idx)) = fuzzy_match(&ql, &e.rel, &lower) {
                let s = if e.ignored { s - IGNORED_PENALTY } else { s };
                heap.push(Scored { score: s, idx: scores.len() });
                scores.push((i, s, idx));
                if heap.len() > limit { heap.pop(); }
            }
        }
        let mut picked: Vec<Scored> = heap.into_vec();
        picked.sort();
        Ok(picked.into_iter().map(|sc| { let (ei, s, idx) = &scores[sc.idx]; let e = &entries[*ei]; QuickOpenResult { path: root.join(&e.rel).to_string_lossy().to_string(), rel_path: e.rel.clone(), score: *s, indices: Some(idx.clone()), ignored: e.ignored } }).collect())
    }).await.map_err(|e| CaretError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn fs_list_files(app: AppHandle, limit: Option<usize>) -> Result<Vec<String>> {
    let limit = limit.unwrap_or(20_000);
    tokio::task::spawn_blocking(move || { ensure_index(&app); let st = app.state::<FileIndexState>(); let v: Vec<String> = st.entries.read().iter().filter(|e| !e.ignored).take(limit).map(|e| e.rel.clone()).collect(); Ok(v) }).await.map_err(|e| CaretError::internal(e.to_string()))?
}

/// Current state of the quick-open index (never waits for a build).
#[tauri::command(async)]
pub fn fs_index_status(st: State<'_, FileIndexState>) -> Result<QuickOpenIndexStatus> { Ok(st.status()) }

// ───────────────────────── tests ─────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ignored_tier_surfaces_gitignored_files_but_not_heavy_dirs() {
        let d = tempfile::tempdir().unwrap(); let r = d.path();
        std::fs::create_dir_all(r.join("src")).unwrap(); std::fs::create_dir_all(r.join("tmp/node_modules")).unwrap(); std::fs::create_dir_all(r.join("node_modules")).unwrap();
        std::fs::write(r.join(".gitignore"), "tmp/\nnode_modules\n").unwrap();
        std::fs::write(r.join("src/a.txt"), "a").unwrap(); std::fs::write(r.join("tmp/x.txt"), "x").unwrap();
        std::fs::write(r.join("tmp/node_modules/n.js"), "n").unwrap(); std::fs::write(r.join("node_modules/m.js"), "m").unwrap();
        let tier0 = collect_workspace_files(r, &[], &[], &WalkLimits::cap_only(1000));
        assert!(!tier0.partial && !tier0.stale);
        assert!(tier0.files.iter().any(|p| p == "src/a.txt") && !tier0.files.iter().any(|p| p.starts_with("tmp/")));
        let known: HashSet<String> = tier0.files.iter().cloned().collect();
        let tier1 = collect_ignored_files(r, &[], &[], &known, &WalkLimits::cap_only(1000));
        assert_eq!(tier1.files, vec!["tmp/x.txt".to_string()]);
    }
    #[test]
    fn walk_limits_cap_and_deadline_publish_a_partial_list() {
        let d = tempfile::tempdir().unwrap(); let r = d.path();
        for i in 0..20 { std::fs::write(r.join(format!("f{i:02}.txt")), "x").unwrap(); }
        let capped = collect_workspace_files(r, &[], &[], &WalkLimits::cap_only(5));
        assert_eq!(capped.files.len(), 5);
        assert!(capped.partial && !capped.stale, "hitting the cap is reported as partial");
        let expired = collect_workspace_files(r, &[], &[], &WalkLimits { cap: 1000, deadline: Some(Instant::now() - Duration::from_millis(1)), ticket: None });
        assert!(expired.partial && expired.files.is_empty(), "an exhausted budget stops before the first entry");
        let full = collect_workspace_files(r, &[], &[], &WalkLimits { cap: 1000, deadline: Some(Instant::now() + Duration::from_secs(30)), ticket: None });
        assert_eq!(full.files.len(), 20);
        assert!(!full.partial);
    }
    #[test]
    fn a_stale_generation_ticket_stops_the_walk_and_marks_it_stale() {
        let d = tempfile::tempdir().unwrap(); let r = d.path();
        for i in 0..5 { std::fs::write(r.join(format!("f{i}.txt")), "x").unwrap(); }
        let g = Generation::new();
        let ticket = g.begin();
        let fresh = collect_workspace_files(r, &[], &[], &WalkLimits { cap: 1000, deadline: None, ticket: Some((&g, ticket)) });
        assert_eq!(fresh.files.len(), 5);
        assert!(!fresh.stale);
        g.begin(); // the workspace changed
        let stale = collect_workspace_files(r, &[], &[], &WalkLimits { cap: 1000, deadline: None, ticket: Some((&g, ticket)) });
        assert!(stale.stale && !stale.partial && stale.files.is_empty(), "a superseded walk quits without results");
    }
    #[test]
    fn merge_sorted_keeps_order_and_prefers_existing_entries() {
        let e = |rel: &str, ignored: bool| Entry { rel: rel.into(), lower: rel.to_lowercase(), ignored };
        let base = vec![e("a.txt", false), e("src/main.rs", false), e("tmp/x.txt", false)];
        let extra = vec![e("b.txt", true), e("tmp/x.txt", true), e("tmp/y.txt", true), e("zz.log", true)];
        let merged = merge_sorted(base, extra.into_iter());
        let rels: Vec<&str> = merged.iter().map(|x| x.rel.as_str()).collect();
        assert_eq!(rels, vec!["a.txt", "b.txt", "src/main.rs", "tmp/x.txt", "tmp/y.txt", "zz.log"]);
        assert!(!merged.iter().find(|x| x.rel == "tmp/x.txt").unwrap().ignored, "the watcher-inserted tracked entry wins over the tier-2 duplicate");
        assert!(merged.windows(2).all(|w| w[0].rel < w[1].rel));
    }
    #[test]
    fn heavy_build_dirs_are_skipped_by_the_ignored_tier_walk() {
        for name in ["target", "build", "dist", "out", ".gradle", "Pods", "DerivedData", ".terraform", "vendor/bundle", "node_modules"] {
            assert!(IGNORED_TIER_SKIP.contains(&name), "{name} must be on the heavy-directory skip list");
        }
        let d = tempfile::tempdir().unwrap(); let r = d.path();
        // A Rust + Node repository: everything below is gitignored, only `scratch/` is small enough to be worth surfacing.
        for dir in ["src", "target/debug/deps", "target/node-src/pkg", "dist", "build", "vendor/bundle/ruby", "website/node_modules/lib", "scratch"] { std::fs::create_dir_all(r.join(dir)).unwrap(); }
        std::fs::write(r.join(".gitignore"), "target/\ndist/\nbuild/\nvendor/bundle/\nnode_modules/\nscratch/\n").unwrap();
        for f in ["src/main.rs", "target/debug/deps/libx.rlib", "target/node-src/pkg/index.js", "dist/app.js", "build/out.o", "vendor/bundle/ruby/gem.rb", "website/node_modules/lib/i.js", "scratch/keep.txt"] { std::fs::write(r.join(f), "x").unwrap(); }
        let tier0 = collect_workspace_files(r, &[], &[], &WalkLimits::cap_only(1000));
        assert_eq!(tier0.files, vec![".gitignore".to_string(), "src/main.rs".to_string()]);
        let known: HashSet<String> = tier0.files.iter().cloned().collect();
        let tier1 = collect_ignored_files(r, &[], &[], &known, &WalkLimits::cap_only(1000));
        assert_eq!(tier1.files, vec!["scratch/keep.txt".to_string()], "target/, dist/, build/, vendor/bundle/ and nested node_modules/ are never walked");
        assert!(!tier1.partial);
    }
    #[test]
    fn line_endings() {
        assert_eq!(detect_line_ending("a\nb\n"), "lf");
        assert_eq!(detect_line_ending("a\r\nb\r\n"), "crlf");
        assert_eq!(detect_line_ending("a\r\nb\n"), "mixed");
        assert_eq!(detect_line_ending("abc"), "lf");
    }
    #[test]
    fn decode_variants() {
        assert_eq!(decode_bytes("héllo".as_bytes()).1, "utf8");
        assert!(decode_bytes(&[0, 159, 146, 150, 0, 0, 1]).2);
        let (s, enc, bin) = decode_bytes(&[0xE9, b'a', b'b']); assert_eq!(enc, "latin1"); assert!(!bin); assert!(s.starts_with('é'));
        let mut u16 = vec![0xFF, 0xFE]; for c in "hi".encode_utf16() { u16.extend_from_slice(&c.to_le_bytes()); }
        let (s, enc, _) = decode_bytes(&u16); assert_eq!(enc, "utf16le"); assert!(s.ends_with("hi"));
    }
    #[test]
    fn atomic_write_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        write_atomic(&p, b"hello").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "hello");
        write_atomic(&p, b"world").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "world");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "no temp files left behind");
    }
    #[test]
    fn fuzzy_scoring_prefers_basename_and_boundaries() {
        let q: Vec<char> = "fsrs".chars().collect();
        let a = "src/core/fs.rs"; let b = "src/frontend/some/random/stuff.ts";
        let sa = fuzzy_match(&q, a, &a.to_lowercase().chars().collect::<Vec<_>>()).unwrap();
        let sb = fuzzy_match(&q, b, &b.to_lowercase().chars().collect::<Vec<_>>()).unwrap();
        assert!(sa.0 > sb.0, "{} vs {}", sa.0, sb.0);
        assert_eq!(sa.1, vec![9, 10, 12, 13]);
        assert!(fuzzy_match(&"zzz".chars().collect::<Vec<_>>(), a, &a.chars().collect::<Vec<_>>()).is_none());
        let q2: Vec<char> = "appts".chars().collect();
        let c1 = "src/App.tsx"; let c2 = "src/components/apple/ts/x.ts";
        let s1 = fuzzy_match(&q2, c1, &c1.to_lowercase().chars().collect::<Vec<_>>()).unwrap().0;
        let s2 = fuzzy_match(&q2, c2, &c2.to_lowercase().chars().collect::<Vec<_>>()).unwrap().0;
        assert!(s1 > s2);
    }
    #[test]
    fn read_dir_sorts_flags_and_peeks_children() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("zeta/inner")).unwrap();
        std::fs::create_dir_all(r.join("empty")).unwrap();
        std::fs::create_dir_all(r.join("dist")).unwrap();
        std::fs::write(r.join("Alpha.txt"), "a").unwrap();
        std::fs::write(r.join(".hidden"), "h").unwrap();
        std::fs::write(r.join(".gitignore"), "dist/\n").unwrap();
        let entries = read_dir_impl(r, Some(r), &[], true).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["dist", "empty", "zeta", ".gitignore", ".hidden", "Alpha.txt"]);
        let by = |n: &str| entries.iter().find(|e| e.name == n).unwrap();
        assert_eq!(by("zeta").has_children, Some(true));
        assert_eq!(by("empty").has_children, Some(false));
        assert!(by("dist").is_ignored && !by("zeta").is_ignored);
        assert!(by(".hidden").is_hidden && !by("Alpha.txt").is_hidden);
        assert_eq!(by("Alpha.txt").size, Some(1));
        assert_eq!(read_dir_impl(r, Some(r), &[], false).unwrap().iter().filter(|e| e.is_hidden).count(), 0);
    }
    #[test]
    fn collect_files_respects_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("src")).unwrap();
        std::fs::create_dir_all(r.join("node_modules/x")).unwrap();
        std::fs::write(r.join("src/a.ts"), "a").unwrap();
        std::fs::write(r.join("node_modules/x/i.js"), "x").unwrap();
        std::fs::write(r.join("secret.env"), "x").unwrap();
        std::fs::write(r.join(".gitignore"), "node_modules\n").unwrap();
        std::fs::write(r.join(".cursorignore"), "*.env\n").unwrap();
        let files = collect_workspace_files(r, &[], &[], &WalkLimits::cap_only(1000)).files;
        assert!(files.contains(&"src/a.ts".to_string()));
        assert!(!files.iter().any(|f| f.contains("node_modules")));
        assert!(!files.iter().any(|f| f.ends_with(".env")));
        let ig = IgnoreSet::build(Some(r), r, &[]);
        assert!(ig.is_ignored(&r.join("node_modules"), true));
        assert!(!ig.is_ignored(&r.join("src"), true));
        assert!(ig.is_ignored(&r.join(".git"), true));
    }
}

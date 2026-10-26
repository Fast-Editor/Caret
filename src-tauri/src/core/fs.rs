//! File system commands + the quick-open fuzzy path index.
use crate::core::settings::SettingsState;
use crate::core::workspace::{WorkspaceInfo, WorkspaceState};
use crate::error::{CaretError, Result};
use crate::paths;
use base64::Engine;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::BinaryHeap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Listener, Manager};

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
pub struct QuickOpenResult { pub path: String, pub rel_path: String, pub score: i32, #[serde(skip_serializing_if = "Option::is_none")] pub indices: Option<Vec<u32>> }

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

#[tauri::command]
pub fn fs_stat(app: AppHandle, path: String) -> Result<FileStat> {
    let p = resolve(&app, &path);
    let p = std::fs::canonicalize(&p).unwrap_or(p);
    match std::fs::metadata(&p) {
        Ok(md) => Ok(FileStat { path: p.to_string_lossy().to_string(), kind: kind_of(&md), size: md.len(), mtime: mtime_ms(&md), readonly: is_readonly(&md), exists: true }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileStat { path: p.to_string_lossy().to_string(), kind: FileKind::Other, size: 0, mtime: 0, readonly: false, exists: false }),
        Err(e) => Err(e.into()),
    }
}

#[tauri::command]
pub fn fs_exists(app: AppHandle, path: String) -> Result<bool> { Ok(resolve(&app, &path).exists()) }

#[tauri::command]
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

#[tauri::command]
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

pub const INDEX_CAP: usize = 200_000;

struct Entry { rel: String, lower: String }

#[derive(Default)]
pub struct FileIndexState {
    root: RwLock<Option<PathBuf>>,
    entries: RwLock<Vec<Entry>>,
    ready: AtomicBool,
    build_lock: Mutex<()>,
}

impl FileIndexState {
    pub fn root(&self) -> Option<PathBuf> { self.root.read().clone() }
    pub fn len(&self) -> usize { self.entries.read().len() }
}

/// Walk the workspace (honouring ignore files + settings) and collect relative file paths.
pub fn collect_workspace_files(root: &Path, global_ignore: &[String], exclude_globs: &[String], cap: usize) -> Vec<String> {
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
    let out = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let root_b = root.to_path_buf();
    wb.build_parallel().run(|| {
        let out = out.clone(); let stop = stop.clone(); let root_b = root_b.clone();
        Box::new(move |res| {
            if stop.load(Ordering::Relaxed) { return ignore::WalkState::Quit; }
            let Ok(e) = res else { return ignore::WalkState::Continue };
            if !e.file_type().map(|t| t.is_file()).unwrap_or(false) { return ignore::WalkState::Continue; }
            let rel = paths::rel_to(&root_b, e.path());
            let mut g = out.lock();
            if g.len() >= cap { stop.store(true, Ordering::Relaxed); return ignore::WalkState::Quit; }
            g.push(rel);
            ignore::WalkState::Continue
        })
    });
    let mut v = std::mem::take(&mut *out.lock());
    v.sort_unstable();
    v
}

fn rebuild_index(app: &AppHandle) {
    let st = app.state::<FileIndexState>();
    let _g = st.build_lock.lock();
    let Some(root) = app.state::<WorkspaceState>().root() else { *st.root.write() = None; st.entries.write().clear(); st.ready.store(false, Ordering::Release); return; };
    let settings = app.state::<SettingsState>().effective();
    let t0 = std::time::Instant::now();
    let files = collect_workspace_files(&root, &settings.general.global_ignore, &settings.indexing.exclude_globs, INDEX_CAP);
    let entries: Vec<Entry> = files.into_iter().map(|rel| Entry { lower: rel.to_lowercase(), rel }).collect();
    tracing::info!(files = entries.len(), ms = t0.elapsed().as_millis() as u64, "quick-open index built");
    *st.root.write() = Some(root);
    *st.entries.write() = entries;
    st.ready.store(true, Ordering::Release);
}

fn ensure_index(app: &AppHandle) {
    let st = app.state::<FileIndexState>();
    let ws_root = app.state::<WorkspaceState>().root();
    if !st.ready.load(Ordering::Acquire) || st.root() != ws_root { rebuild_index(app); }
}

/// Called by the watcher / fs ops: add a file to the index if it lives in the workspace.
pub fn index_add(app: &AppHandle, path: &Path) {
    let st = app.state::<FileIndexState>();
    if !st.ready.load(Ordering::Acquire) { return; }
    let Some(root) = st.root() else { return };
    if !path.starts_with(&root) || !path.is_file() { return; }
    let rel = paths::rel_to(&root, path);
    if rel.split('/').any(|seg| seg == ".git") { return; }
    let mut e = st.entries.write();
    if e.len() >= INDEX_CAP { return; }
    match e.binary_search_by(|x| x.rel.as_str().cmp(rel.as_str())) { Ok(_) => {}, Err(pos) => e.insert(pos, Entry { lower: rel.to_lowercase(), rel }) }
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
        if info.is_none() { h2.state::<FileIndexState>().ready.store(false, Ordering::Release); return; }
        std::thread::Builder::new().name("caret-file-index".into()).spawn(move || rebuild_index(&h2)).ok();
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
            return Ok(entries.iter().take(limit).map(|e| QuickOpenResult { path: root.join(&e.rel).to_string_lossy().to_string(), rel_path: e.rel.clone(), score: 0, indices: None }).collect());
        }
        let mut scores: Vec<(usize, i32, Vec<u32>)> = Vec::new();
        for (i, e) in entries.iter().enumerate() {
            let lower: Vec<char> = e.lower.chars().collect();
            if let Some((s, idx)) = fuzzy_match(&ql, &e.rel, &lower) {
                heap.push(Scored { score: s, idx: scores.len() });
                scores.push((i, s, idx));
                if heap.len() > limit { heap.pop(); }
            }
        }
        let mut picked: Vec<Scored> = heap.into_vec();
        picked.sort();
        Ok(picked.into_iter().map(|sc| { let (ei, s, idx) = &scores[sc.idx]; let e = &entries[*ei]; QuickOpenResult { path: root.join(&e.rel).to_string_lossy().to_string(), rel_path: e.rel.clone(), score: *s, indices: Some(idx.clone()) } }).collect())
    }).await.map_err(|e| CaretError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn fs_list_files(app: AppHandle, limit: Option<usize>) -> Result<Vec<String>> {
    let limit = limit.unwrap_or(20_000);
    tokio::task::spawn_blocking(move || { ensure_index(&app); let st = app.state::<FileIndexState>(); let v: Vec<String> = st.entries.read().iter().take(limit).map(|e| e.rel.clone()).collect(); Ok(v) }).await.map_err(|e| CaretError::internal(e.to_string()))?
}

// ───────────────────────── tests ─────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
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
        let files = collect_workspace_files(r, &[], &[], 1000);
        assert!(files.contains(&"src/a.ts".to_string()));
        assert!(!files.iter().any(|f| f.contains("node_modules")));
        assert!(!files.iter().any(|f| f.ends_with(".env")));
        let ig = IgnoreSet::build(Some(r), r, &[]);
        assert!(ig.is_ignored(&r.join("node_modules"), true));
        assert!(!ig.is_ignored(&r.join("src"), true));
        assert!(ig.is_ignored(&r.join(".git"), true));
    }
}

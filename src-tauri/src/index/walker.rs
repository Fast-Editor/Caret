//! Ignore-aware file enumeration + content-hash manifest for incremental indexing.
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct WalkOptions {
    pub root: PathBuf,
    pub exclude_globs: Vec<String>,
    pub global_ignore: Vec<String>,
    pub max_file_size: u64,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub rel_path: String,
    pub abs_path: PathBuf,
    pub mtime: u64,
    pub size: u64,
}

/// Directories never worth indexing even when not ignored by VCS files.
const HARD_EXCLUDES: &[&str] = &[".git", ".hg", ".svn", ".caret", ".cursor/worktrees", "node_modules", "bower_components", "__pycache__", ".venv", "venv", ".next", ".nuxt", ".turbo", ".cache", ".pytest_cache", ".mypy_cache", ".gradle", ".idea", ".DS_Store"];

pub fn build_walker(opts: &WalkOptions) -> WalkBuilder {
    let mut b = WalkBuilder::new(&opts.root);
    b.hidden(false)
        .follow_links(false)
        .require_git(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .parents(true)
        .max_filesize(Some(opts.max_file_size))
        .sort_by_file_name(|a, b| a.cmp(b));
    for name in [".caretignore", ".cursorignore", ".cursorindexingignore"] {
        b.add_custom_ignore_filename(name);
    }
    let mut ov = OverrideBuilder::new(&opts.root);
    for g in HARD_EXCLUDES.iter().map(|s| s.to_string()).chain(opts.exclude_globs.iter().cloned()).chain(opts.global_ignore.iter().map(|n| format!("**/{n}"))) {
        let pat = if g.starts_with('!') { g } else { format!("!{g}") };
        let _ = ov.add(&pat);
        // also match the bare directory name anywhere (e.g. "node_modules")
        if !pat.contains('/') && !pat.contains('*') { let _ = ov.add(&format!("{pat}/**")); }
    }
    if let Ok(o) = ov.build() { b.overrides(o); }
    b
}

/// Heuristic text check on the first 8 KiB.
pub fn is_probably_text(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let mut buf = [0u8; 8192];
    let n = f.read(&mut buf).unwrap_or(0);
    if n == 0 { return true; }
    !content_inspector::inspect(&buf[..n]).is_binary()
}

pub fn mtime_of(md: &std::fs::Metadata) -> u64 {
    md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Walk the root; `on_file` returns `false` to stop early. Returns number of files visited.
pub fn walk(opts: &WalkOptions, mut on_file: impl FnMut(FileEntry) -> bool) -> usize {
    let mut n = 0usize;
    for entry in build_walker(opts).build().flatten() {
        let Some(ft) = entry.file_type() else { continue };
        if !ft.is_file() { continue; }
        let Ok(md) = entry.metadata() else { continue };
        if md.len() > opts.max_file_size { continue; }
        let abs = entry.into_path();
        let rel = match abs.strip_prefix(&opts.root) { Ok(r) => r.to_string_lossy().replace('\\', "/"), Err(_) => continue };
        if !is_probably_text(&abs) { continue; }
        n += 1;
        if !on_file(FileEntry { rel_path: rel, abs_path: abs, mtime: mtime_of(&md), size: md.len() }) { break; }
    }
    n
}

/// Single-file indexability check (used for incremental updates from fs events).
pub fn is_indexable(opts: &WalkOptions, abs: &Path) -> bool {
    let Ok(md) = std::fs::symlink_metadata(abs) else { return false };
    if !md.is_file() || md.len() > opts.max_file_size { return false; }
    let rel = abs.strip_prefix(&opts.root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_default();
    if rel.is_empty() { return false; }
    if HARD_EXCLUDES.iter().any(|h| rel.split('/').any(|c| c == *h) || rel.starts_with(&format!("{h}/"))) { return false; }
    // Walk from the workspace root but only descend along this file's ancestor chain, so every
    // ignore file / override is applied exactly as in a full walk while visiting O(depth) entries.
    let target = abs.to_path_buf();
    let mut w = build_walker(opts);
    w.filter_entry(move |e| target.starts_with(e.path()));
    let found = w.build().flatten().any(|e| e.path() == abs);
    found && is_probably_text(abs)
}

// ───────────────────────── manifest ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileRecord { pub hash: String, pub mtime: u64, pub size: u64, pub chunks: u32 }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    pub root: String,
    pub files: BTreeMap<String, FileRecord>,
    pub last_indexed_at: Option<u64>,
    pub embedding_model: Option<String>,
}

impl Manifest {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(p) = path.parent() { std::fs::create_dir_all(p)?; }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(tmp, path)
    }
    /// Files present in `current` but absent/changed vs manifest (by mtime+size fast path, else hash).
    pub fn diff<'a>(&self, current: &'a [FileEntry]) -> (Vec<&'a FileEntry>, Vec<String>) {
        let mut changed = vec![];
        let mut seen = std::collections::HashSet::with_capacity(current.len());
        for f in current {
            seen.insert(f.rel_path.as_str());
            match self.files.get(&f.rel_path) {
                Some(r) if r.mtime == f.mtime && r.size == f.size => {}
                _ => changed.push(f),
            }
        }
        let removed = self.files.keys().filter(|k| !seen.contains(k.as_str())).cloned().collect();
        (changed, removed)
    }
}

pub fn hash_bytes(b: &[u8]) -> String { blake3::hash(b).to_hex()[..32].to_string() }

#[cfg(test)]
mod tests {
    use super::*;
    fn opts(root: &Path) -> WalkOptions { WalkOptions { root: root.to_path_buf(), exclude_globs: vec!["**/dist/**".into()], global_ignore: vec![".DS_Store".into()], max_file_size: 1 << 20 } }

    #[test]
    fn respects_gitignore_and_cursorignore() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("src")).unwrap();
        std::fs::create_dir_all(r.join("node_modules/x")).unwrap();
        std::fs::create_dir_all(r.join("secret")).unwrap();
        std::fs::create_dir_all(r.join("dist")).unwrap();
        std::fs::write(r.join(".gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(r.join(".cursorignore"), "secret/\n").unwrap();
        std::fs::write(r.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(r.join("ignored.txt"), "x").unwrap();
        std::fs::write(r.join("secret/key.txt"), "x").unwrap();
        std::fs::write(r.join("node_modules/x/i.js"), "x").unwrap();
        std::fs::write(r.join("dist/b.js"), "x").unwrap();
        std::fs::write(r.join("bin.dat"), [0u8, 1, 2, 3, 0, 0, 255]).unwrap();
        let mut seen = vec![];
        walk(&opts(r), |f| { seen.push(f.rel_path); true });
        seen.sort();
        assert_eq!(seen, vec![".cursorignore", ".gitignore", "src/main.rs"]);
        assert!(is_indexable(&opts(r), &r.join("src/main.rs")));
        assert!(!is_indexable(&opts(r), &r.join("secret/key.txt")));
        assert!(!is_indexable(&opts(r), &r.join("node_modules/x/i.js")));
    }

    #[test]
    fn manifest_diff_detects_changes() {
        let mut m = Manifest::default();
        m.files.insert("a.rs".into(), FileRecord { hash: "h".into(), mtime: 1, size: 10, chunks: 1 });
        m.files.insert("gone.rs".into(), FileRecord { hash: "h".into(), mtime: 1, size: 10, chunks: 1 });
        let cur = vec![
            FileEntry { rel_path: "a.rs".into(), abs_path: "/a.rs".into(), mtime: 1, size: 10 },
            FileEntry { rel_path: "b.rs".into(), abs_path: "/b.rs".into(), mtime: 2, size: 3 },
        ];
        let (changed, removed) = m.diff(&cur);
        assert_eq!(changed.iter().map(|f| f.rel_path.as_str()).collect::<Vec<_>>(), vec!["b.rs"]);
        assert_eq!(removed, vec!["gone.rs".to_string()]);
    }
}

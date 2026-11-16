//! Workspace sync between the local folder and a thread's Warren instance.
//!
//! * [`scan`] builds a [`Manifest`] of the local files that belong in the instance: gitignore-aware (`ignore` crate,
//!   `.gitignore`, `.ignore`, `.caretignore`, global excludes), `.git` **included** so `git status` / `git diff` work
//!   remotely, `warren.syncExcludes` directory names skipped at any depth, symlinks and special files skipped.
//! * [`plan_push`] diffs the manifest the instance holds against a fresh scan.
//! * [`push`] uploads one gzip tarball of the changed files to `~/.caret-sync/push.tgz` on the instance and runs one
//!   command: extract into the remote root (`tar -m`, so extracted files are never newer than the marker that is
//!   touched right after), `rm -f` the deleted paths, `touch ~/.caret-sync/marker`.
//! * [`pull`] runs one command that lists files under the remote root newer than the marker (minus `.git` and the
//!   excludes), tars them to `~/.caret-sync/pull.tgz`, prints the tarball size and the full file listing; then
//!   downloads the tarball (50 MiB cap), extracts it in memory and returns the files whose bytes differ from the local
//!   copy plus the pushed files that are gone remotely. Nothing is written locally: the executor turns the result into
//!   pending changes.
//!
//! Remote calls go through [`RemoteOps`] (implemented by [`WarrenClient`]); tests use a fake that runs the very same
//! shell commands locally.
use super::client::WarrenClient;
use super::types::{wire, SandboxBinding};
use crate::error::{CaretError, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path};
use std::time::{Instant, UNIX_EPOCH};

/// A pull larger than this is refused with a clear error (exclude build output with `warren.syncExcludes`).
pub const MAX_PULL_BYTES: u64 = 50 * 1024 * 1024;
/// Where sync state lives inside the instance, relative to the persistent home.
pub const SYNC_DIR: &str = ".caret-sync";
const PUSH_TGZ: &str = ".caret-sync/push.tgz";
const PULL_TGZ: &str = ".caret-sync/pull.tgz";
/// Command timeout for the sync execs (extracting or tarring a large tree).
const SYNC_TIMEOUT_SECS: u32 = 600;
/// `rm -f` arguments per command segment.
const RM_CHUNK: usize = 200;

// ───────────────────────── manifest ─────────────────────────

/// Size and mtime of one pushed file; a change in either means a re-upload.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileStamp { pub size: u64, pub mtime_ms: u64 }

/// Relative path (slash separated) → stamp of every file the instance holds (or, from [`scan`], that it should hold).
/// The executor persists the pushed manifest per thread (see [`Manifest::load`] / [`Manifest::save`]).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Manifest { pub files: BTreeMap<String, FileStamp> }

impl Manifest {
    pub fn len(&self) -> usize { self.files.len() }
    pub fn is_empty(&self) -> bool { self.files.is_empty() }
    /// Read a saved manifest; a missing file is an empty manifest.
    pub fn load(path: &Path) -> Result<Manifest> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::default()),
            Err(e) => Err(e.into()),
        }
    }
    /// Write atomically (temp file + rename), creating parent directories.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// Directory names to skip, normalised (trimmed, no slashes).
fn exclude_set(excludes: &[String]) -> BTreeSet<String> {
    excludes.iter().map(|s| s.trim().trim_matches('/').to_string()).filter(|s| !s.is_empty()).collect()
}

/// `rel` lies under `.git/` or under a directory named in `excludes` (at any depth).
pub fn is_excluded(rel: &str, excludes: &[String]) -> bool {
    let ex = exclude_set(excludes);
    let mut parts = rel.split('/').peekable();
    while let Some(p) = parts.next() {
        let is_dir = parts.peek().is_some();
        if is_dir && (p == ".git" || ex.contains(p)) { return true; }
    }
    false
}

fn mtime_ms(md: &std::fs::Metadata) -> u64 {
    md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn rel_string(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel.components().filter_map(|c| match c { Component::Normal(s) => Some(s.to_string_lossy().into_owned()), _ => None }).collect();
    if parts.is_empty() { None } else { Some(parts.join("/")) }
}

/// Walk `local_root` and stamp every regular file that belongs in the instance (see the module docs for the rules).
pub fn scan(local_root: &Path, excludes: &[String]) -> Result<Manifest> {
    if !local_root.is_dir() { return Err(CaretError::not_found(format!("workspace folder {}", local_root.display()))); }
    let ex = exclude_set(excludes);
    let mut wb = ignore::WalkBuilder::new(local_root);
    wb.hidden(false).git_ignore(true).git_global(true).git_exclude(true).ignore(true).parents(true).require_git(false).follow_links(false).same_file_system(false);
    wb.add_custom_ignore_filename(".caretignore");
    wb.filter_entry(move |e| {
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        !(is_dir && e.depth() > 0 && ex.contains(e.file_name().to_string_lossy().as_ref()))
    });
    let mut files = BTreeMap::new();
    for entry in wb.build() {
        let entry = match entry { Ok(e) => e, Err(e) => { tracing::debug!(error = %e, "warren sync: scan entry skipped"); continue } };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) { continue; }
        let md = match entry.metadata() { Ok(m) => m, Err(_) => continue };
        if let Some(rel) = rel_string(local_root, entry.path()) { files.insert(rel, FileStamp { size: md.len(), mtime_ms: mtime_ms(&md) }); }
    }
    Ok(Manifest { files })
}

// ───────────────────────── planning ─────────────────────────

/// What a push has to do: files to (re)upload and remote paths to remove. Both sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushPlan { pub upload: Vec<String>, pub delete: Vec<String> }

impl PushPlan {
    pub fn is_empty(&self) -> bool { self.upload.is_empty() && self.delete.is_empty() }
}

/// Diff the manifest the instance holds (`prev`) against a fresh [`scan`] (`now`).
pub fn plan_push(prev: &Manifest, now: &Manifest) -> PushPlan {
    let upload = now.files.iter().filter(|(rel, stamp)| prev.files.get(*rel) != Some(*stamp)).map(|(rel, _)| rel.clone()).collect();
    let delete = prev.files.keys().filter(|rel| !now.files.contains_key(*rel)).cloned().collect();
    PushPlan { upload, delete }
}

// ───────────────────────── remote operations ─────────────────────────

/// The three instance operations sync needs. [`WarrenClient`] implements it; tests substitute a local shell.
#[async_trait]
pub trait RemoteOps: Send + Sync {
    /// Run `command` through the instance's shell.
    async fn exec(&self, instance_id: &str, command: &str, timeout_secs: u32) -> Result<wire::ExecResult>;
    /// Write `body` to `path` (relative to the persistent home), creating parents.
    async fn write_file(&self, instance_id: &str, path: &str, body: Vec<u8>) -> Result<()>;
    /// Read `path` (relative to the persistent home).
    async fn read_file(&self, instance_id: &str, path: &str) -> Result<Vec<u8>>;
}

#[async_trait]
impl RemoteOps for WarrenClient {
    async fn exec(&self, instance_id: &str, command: &str, timeout_secs: u32) -> Result<wire::ExecResult> {
        WarrenClient::exec(self, instance_id, &wire::ExecRequest { timeout_seconds: timeout_secs, ..wire::ExecRequest::new(command) }).await
    }
    async fn write_file(&self, instance_id: &str, path: &str, body: Vec<u8>) -> Result<()> { self.files_write(instance_id, path, body).await.map(|_| ()) }
    async fn read_file(&self, instance_id: &str, path: &str) -> Result<Vec<u8>> { self.files_read(instance_id, path).await.map(|b| b.to_vec()) }
}

/// Single-quote `s` for `sh` (embedded quotes become `'\''`).
pub fn sh_quote(s: &str) -> String { format!("'{}'", s.replace('\'', "'\\''")) }

fn remote_path(root: &str, rel: &str) -> String { format!("{}/{}", root.trim_end_matches('/'), rel) }

/// The command [`push`] runs after uploading the tarball.
pub fn push_command(remote_root: &str, deletes: &[String]) -> String {
    let root = sh_quote(remote_root);
    // `-m`: extracted files get the extraction time, not the local mtime, so a clock ahead of the instance (or a
    // file stamped in the future) never makes a pushed file look "newer than the marker" to the next pull.
    let mut cmd = format!("mkdir -p {root} && tar -xzmf ~/{PUSH_TGZ} -C {root}");
    for chunk in deletes.chunks(RM_CHUNK) {
        cmd.push_str(" && rm -f");
        for d in chunk { cmd.push(' '); cmd.push_str(&sh_quote(&remote_path(remote_root, d))); }
    }
    cmd.push_str(&format!(" && touch ~/{SYNC_DIR}/marker"));
    cmd
}

const OUT_NO_ROOT: &str = "--caret-sync-no-root--";
const OUT_CHANGED: &str = "--caret-sync-changed--";
const OUT_SIZE: &str = "--caret-sync-size--";
const OUT_ALL: &str = "--caret-sync-all--";

/// The command [`pull`] runs. Output: `--caret-sync-no-root--` alone when the root does not exist; otherwise
/// `--caret-sync-changed--`, the changed paths, `--caret-sync-size--`, the tarball size (`0` none, `-1` tar failed),
/// `--caret-sync-all--`, every file under the root (`.git` and excludes left out of both listings).
pub fn pull_command(remote_root: &str, excludes: &[String]) -> String {
    let root = sh_quote(remote_root);
    let mut filter = String::from(" -not -path './.git/*'");
    for ex in exclude_set(excludes) { filter.push_str(&format!(" -not -path {}", sh_quote(&format!("*/{ex}/*")))); }
    let sync = format!("~/{SYNC_DIR}");
    format!(
        "mkdir -p {sync} && rm -f {sync}/pull.tgz; if [ ! -d {root} ]; then echo '{OUT_NO_ROOT}'; exit 0; fi; cd {root} || exit 1; \
         : > {sync}/pull-list; if [ -f {sync}/marker ]; then find . -type f -newer {sync}/marker{filter} > {sync}/pull-list; fi; \
         echo '{OUT_CHANGED}'; cat {sync}/pull-list; echo '{OUT_SIZE}'; \
         if [ -s {sync}/pull-list ]; then tar -czf {sync}/pull.tgz -T {sync}/pull-list 2>/dev/null; if [ -f {sync}/pull.tgz ]; then wc -c < {sync}/pull.tgz; else echo -1; fi; else echo 0; fi; \
         touch {sync}/marker; echo '{OUT_ALL}'; find . -type f{filter}"
    )
}

/// Parsed output of [`pull_command`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PullListing {
    /// The remote root does not exist (nothing was ever pushed).
    pub no_root: bool,
    /// Files newer than the marker, relative to the root.
    pub changed: Vec<String>,
    /// Bytes of `pull.tgz`; `0` when nothing changed, `-1` when tar failed.
    pub tarball_size: i64,
    /// Every file under the root (minus `.git` and excludes); `None` when the output was cut before or inside it.
    pub all: Option<BTreeSet<String>>,
}

fn strip_dot(line: &str) -> String { line.strip_prefix("./").unwrap_or(line).to_string() }

/// Parse what [`pull_command`] printed. `truncated` is Warren's flag that stdout hit its 1 MiB cap: the full listing
/// at the end is then incomplete and `all` is `None`; the earlier sections are still intact.
pub fn parse_pull_output(stdout: &str, truncated: bool) -> Result<PullListing> {
    let mut lines = stdout.lines();
    let first = lines.next().unwrap_or("").trim();
    if first == OUT_NO_ROOT { return Ok(PullListing { no_root: true, ..Default::default() }); }
    if first != OUT_CHANGED { return Err(CaretError::new("warren_sync", format!("unexpected pull output: {}", first.chars().take(120).collect::<String>()))); }
    let mut changed = Vec::new();
    for line in lines.by_ref() {
        if line == OUT_SIZE { break; }
        if !line.is_empty() { changed.push(strip_dot(line)); }
    }
    let size_line = lines.next().ok_or_else(|| CaretError::new("warren_sync", "pull output ended before the tarball size (output truncated?)"))?;
    let tarball_size: i64 = size_line.trim().parse().map_err(|_| CaretError::new("warren_sync", format!("bad tarball size in pull output: {size_line:?}")))?;
    let mut saw_all = false;
    let mut all = BTreeSet::new();
    for line in lines {
        if line == OUT_ALL { saw_all = true; continue; }
        if saw_all && !line.is_empty() { all.insert(strip_dot(line)); }
    }
    if !saw_all { return Err(CaretError::new("warren_sync", "pull output ended before the file listing")); }
    Ok(PullListing { no_root: false, changed, tarball_size, all: if truncated { None } else { Some(all) } })
}

// ───────────────────────── tarballs ─────────────────────────

/// Gzip tarball of `rels` under `local_root` (regular files only; files that vanished are skipped). Returns the bytes
/// and how many files went in.
pub fn build_tarball(local_root: &Path, rels: &[String]) -> Result<(Vec<u8>, usize)> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut count = 0usize;
    {
        let mut tar = tar::Builder::new(&mut enc);
        tar.follow_symlinks(false);
        for rel in rels {
            let p = local_root.join(rel);
            let md = match std::fs::symlink_metadata(&p) { Ok(m) if m.is_file() => m, _ => continue };
            let mut f = match std::fs::File::open(&p) { Ok(f) => f, Err(_) => continue };
            let mut header = tar::Header::new_gnu();
            header.set_size(md.len());
            header.set_mode(file_mode(&md));
            header.set_mtime(md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0));
            header.set_entry_type(tar::EntryType::Regular);
            tar.append_data(&mut header, rel, &mut f).map_err(|e| CaretError::io(format!("tar {rel}: {e}")))?;
            count += 1;
        }
        tar.finish().map_err(|e| CaretError::io(format!("tar finish: {e}")))?;
    }
    let bytes = enc.finish().map_err(|e| CaretError::io(format!("gzip: {e}")))?;
    Ok((bytes, count))
}

#[cfg(unix)]
fn file_mode(md: &std::fs::Metadata) -> u32 { use std::os::unix::fs::PermissionsExt; md.permissions().mode() & 0o777 }
#[cfg(not(unix))]
fn file_mode(_md: &std::fs::Metadata) -> u32 { 0o644 }

/// Regular files of a gzip tarball as `(relative path, bytes)`; `./` prefixes dropped, entries escaping the root
/// (`..`, absolute) skipped.
pub fn extract_tarball(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    let mut out = Vec::new();
    for entry in archive.entries().map_err(|e| CaretError::io(format!("tarball: {e}")))? {
        let mut entry = entry.map_err(|e| CaretError::io(format!("tarball entry: {e}")))?;
        if !entry.header().entry_type().is_file() { continue; }
        let path = entry.path().map_err(|e| CaretError::io(format!("tarball path: {e}")))?.into_owned();
        let mut parts = Vec::new();
        let mut bad = false;
        for c in path.components() {
            match c { Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()), Component::CurDir => {}, _ => { bad = true; break } }
        }
        if bad || parts.is_empty() { continue; }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| CaretError::io(format!("tarball read: {e}")))?;
        out.push((parts.join("/"), buf));
    }
    Ok(out)
}

// ───────────────────────── push / pull ─────────────────────────

/// What a push did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushReport {
    /// Files uploaded.
    pub files: usize,
    /// Tarball bytes sent.
    pub bytes: u64,
    /// Remote paths removed.
    pub deleted: usize,
    pub ms: u64,
}

/// Upload the planned files as one tarball and apply the plan in one command (see the module docs). An empty plan
/// makes no request. After success the instance mirrors the manifest the plan was computed from.
pub async fn push<R: RemoteOps + ?Sized>(remote: &R, binding: &SandboxBinding, local_root: &Path, plan: &PushPlan) -> Result<PushReport> {
    let start = Instant::now();
    if plan.is_empty() { return Ok(PushReport::default()); }
    let (tgz, files) = build_tarball(local_root, &plan.upload)?;
    if files == 0 && plan.delete.is_empty() { return Ok(PushReport { ms: start.elapsed().as_millis() as u64, ..Default::default() }); }
    let bytes = tgz.len() as u64;
    remote.write_file(&binding.instance_id, PUSH_TGZ, tgz).await?;
    let r = remote.exec(&binding.instance_id, &push_command(&binding.remote_root, &plan.delete), SYNC_TIMEOUT_SECS).await?;
    if r.exit_code != 0 {
        return Err(CaretError::new("warren_sync", format!("push into {} failed (exit {}): {}", binding.remote_root, r.exit_code, tail(&r.stderr))));
    }
    Ok(PushReport { files, bytes, deleted: plan.delete.len(), ms: start.elapsed().as_millis() as u64 })
}

/// What a pull found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PullReport {
    /// Files created or changed remotely since the last sync whose bytes differ from the local copy, sorted by path.
    pub changed: Vec<(String, Vec<u8>)>,
    /// Pushed files that no longer exist remotely (never `.git/` or excluded paths), sorted.
    pub deleted: Vec<String>,
    /// Tarball bytes downloaded.
    pub bytes: u64,
    pub ms: u64,
    /// Things worth telling the user (e.g. the remote listing was cut, so deletions were not checked).
    pub warnings: Vec<String>,
}

/// Bring back what commands changed under `binding.remote_root` (see the module docs). `pushed` is the manifest the
/// instance holds (for deletion detection); `excludes` is `warren.syncExcludes`. Nothing is written to `local_root`;
/// it is only read to drop files whose bytes are unchanged.
pub async fn pull<R: RemoteOps + ?Sized>(remote: &R, binding: &SandboxBinding, local_root: &Path, pushed: &Manifest, excludes: &[String]) -> Result<PullReport> {
    let start = Instant::now();
    let r = remote.exec(&binding.instance_id, &pull_command(&binding.remote_root, excludes), SYNC_TIMEOUT_SECS).await?;
    if r.exit_code != 0 {
        return Err(CaretError::new("warren_sync", format!("listing {} failed (exit {}): {}", binding.remote_root, r.exit_code, tail(&r.stderr))));
    }
    let listing = parse_pull_output(&r.stdout, r.truncated)?;
    let mut report = PullReport::default();
    if listing.no_root { report.ms = start.elapsed().as_millis() as u64; return Ok(report); }
    if listing.tarball_size < 0 { return Err(CaretError::new("warren_sync", format!("the instance could not tar the changed files under {}", binding.remote_root))); }
    if listing.tarball_size as u64 > MAX_PULL_BYTES {
        return Err(CaretError::new("warren_sync", format!("pull too large: {} MiB of changed files (cap {} MiB); add generated directories to warren.syncExcludes", listing.tarball_size / (1024 * 1024), MAX_PULL_BYTES / (1024 * 1024))));
    }
    if !listing.changed.is_empty() && listing.tarball_size > 0 {
        let tgz = remote.read_file(&binding.instance_id, PULL_TGZ).await?;
        if tgz.len() as u64 > MAX_PULL_BYTES { return Err(CaretError::new("warren_sync", format!("pull too large: {} MiB (cap {} MiB)", tgz.len() / (1024 * 1024), MAX_PULL_BYTES / (1024 * 1024)))); }
        report.bytes = tgz.len() as u64;
        for (rel, bytes) in extract_tarball(&tgz)? {
            if is_excluded(&rel, excludes) { continue; }
            if std::fs::read(local_root.join(&rel)).map(|local| local == bytes).unwrap_or(false) { continue; }
            report.changed.push((rel, bytes));
        }
        report.changed.sort_by(|a, b| a.0.cmp(&b.0));
    }
    match &listing.all {
        Some(all) => report.deleted = pushed.files.keys().filter(|rel| !is_excluded(rel, excludes) && !all.contains(*rel)).cloned().collect(),
        None => report.warnings.push("remote file listing was cut at Warren's output cap; deletions were not checked".into()),
    }
    report.ms = start.elapsed().as_millis() as u64;
    Ok(report)
}

fn tail(s: &str) -> String { let t = s.trim(); if t.len() > 400 { format!("…{}", &t[t.len() - 400..]) } else { t.to_string() } }

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime};

    fn write(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }
    /// Stamp a local file in the future (a clock ahead of the instance): a pushed file must still never look changed.
    fn future_mtime(root: &Path, rel: &str) {
        let f = std::fs::OpenOptions::new().write(true).open(root.join(rel)).unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(5)).unwrap();
    }
    /// Pretend the last sync happened a while ago, so files a "command" writes now are newer than the marker even on
    /// file systems with coarse timestamps.
    fn age_marker(home: &Path) {
        let f = std::fs::OpenOptions::new().write(true).open(home.join(".caret-sync/marker")).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(10)).unwrap();
    }
    fn keys(m: &Manifest) -> Vec<&str> { m.files.keys().map(String::as_str).collect() }
    fn strs(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }

    /// Runs the sync commands with `sh` against a temp "home": the same shell, tar and find the instance would use.
    struct LocalShell { home: std::path::PathBuf, execs: AtomicUsize, writes: AtomicUsize, reads: AtomicUsize }
    impl LocalShell {
        fn new(home: &Path) -> Self { Self { home: home.to_path_buf(), execs: AtomicUsize::new(0), writes: AtomicUsize::new(0), reads: AtomicUsize::new(0) } }
        fn counts(&self) -> (usize, usize, usize) { (self.execs.load(Ordering::SeqCst), self.writes.load(Ordering::SeqCst), self.reads.load(Ordering::SeqCst)) }
    }
    #[async_trait]
    impl RemoteOps for LocalShell {
        async fn exec(&self, _id: &str, command: &str, _timeout: u32) -> Result<wire::ExecResult> {
            self.execs.fetch_add(1, Ordering::SeqCst);
            let out = tokio::process::Command::new("sh").arg("-c").arg(command).env("HOME", &self.home).current_dir(&self.home).output().await?;
            Ok(wire::ExecResult { exit_code: out.status.code().unwrap_or(-1), stdout: String::from_utf8_lossy(&out.stdout).into_owned(), stderr: String::from_utf8_lossy(&out.stderr).into_owned(), duration_ms: 0, truncated: false })
        }
        async fn write_file(&self, _id: &str, path: &str, body: Vec<u8>) -> Result<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            let p = self.home.join(path);
            std::fs::create_dir_all(p.parent().unwrap())?;
            Ok(std::fs::write(p, body)?)
        }
        async fn read_file(&self, _id: &str, path: &str) -> Result<Vec<u8>> { self.reads.fetch_add(1, Ordering::SeqCst); Ok(std::fs::read(self.home.join(path))?) }
    }

    #[test]
    fn scan_honours_gitignore_and_excludes_and_includes_git() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, ".gitignore", "ignored.txt\nbuild/\n");
        write(root, "src/a.rs", "fn a() {}");
        write(root, "ignored.txt", "x");
        write(root, "build/out.bin", "x");
        write(root, "node_modules/x/index.js", "x");
        write(root, "nested/node_modules/y.js", "x");
        write(root, ".git/HEAD", "ref: refs/heads/main");
        write(root, ".git/objects/ab/cd", "blob");
        write(root, ".env", "SECRET=1");
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("src/a.rs"), root.join("link.rs")).unwrap();
        let m = scan(root, &strs(&["node_modules", "target"])).unwrap();
        assert_eq!(keys(&m), vec![".env", ".git/HEAD", ".git/objects/ab/cd", ".gitignore", "src/a.rs"]);
        assert_eq!(m.files["src/a.rs"].size, 9);
        assert!(m.files["src/a.rs"].mtime_ms > 0);
        assert!(scan(&root.join("nope"), &[]).is_err());
        assert!(is_excluded(".git/HEAD", &[]) && is_excluded("a/node_modules/b.js", &strs(&["node_modules"])) && !is_excluded("node_modules", &strs(&["node_modules"])) && !is_excluded("src/a.rs", &strs(&["src2"])));
    }

    #[test]
    fn plan_push_detects_new_changed_and_deleted_files() {
        let st = |size, mtime_ms| FileStamp { size, mtime_ms };
        let prev = Manifest { files: BTreeMap::from([("same.rs".into(), st(1, 1)), ("changed.rs".into(), st(2, 2)), ("touched.rs".into(), st(3, 3)), ("gone.rs".into(), st(4, 4))]) };
        let now = Manifest { files: BTreeMap::from([("same.rs".into(), st(1, 1)), ("changed.rs".into(), st(5, 2)), ("touched.rs".into(), st(3, 9)), ("new.rs".into(), st(6, 6))]) };
        let plan = plan_push(&prev, &now);
        assert_eq!(plan.upload, strs(&["changed.rs", "new.rs", "touched.rs"]));
        assert_eq!(plan.delete, strs(&["gone.rs"]));
        assert!(plan_push(&now, &now).is_empty());
        let full = plan_push(&Manifest::default(), &now);
        assert_eq!(full.upload.len(), 4);
        assert!(full.delete.is_empty());
    }

    #[test]
    fn manifest_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("deep/manifest.json");
        assert!(Manifest::load(&p).unwrap().is_empty(), "missing file is empty");
        let m = Manifest { files: BTreeMap::from([("a/b.rs".into(), FileStamp { size: 3, mtime_ms: 1700000000000 })]) };
        m.save(&p).unwrap();
        assert_eq!(Manifest::load(&p).unwrap(), m);
        assert!(std::fs::read_to_string(&p).unwrap().contains("\"mtimeMs\":1700000000000"));
    }

    #[test]
    fn commands_quote_paths_and_filter_excludes() {
        let cmd = push_command("/home/agent/work/it's here", &strs(&["a b.txt", "q'uote.rs"]));
        assert_eq!(cmd, "mkdir -p '/home/agent/work/it'\\''s here' && tar -xzmf ~/.caret-sync/push.tgz -C '/home/agent/work/it'\\''s here' && rm -f '/home/agent/work/it'\\''s here/a b.txt' '/home/agent/work/it'\\''s here/q'\\''uote.rs' && touch ~/.caret-sync/marker");
        assert_eq!(push_command("/r/", &[]), "mkdir -p '/r/' && tar -xzmf ~/.caret-sync/push.tgz -C '/r/' && touch ~/.caret-sync/marker");
        let many: Vec<String> = (0..450).map(|i| format!("f{i}")).collect();
        assert_eq!(push_command("/r", &many).matches(" && rm -f ").count(), 3, "deletions are chunked");
        let pull = pull_command("/home/agent/work/proj", &strs(&["node_modules", " target/ ", ""]));
        assert!(pull.contains("find . -type f -newer ~/.caret-sync/marker -not -path './.git/*' -not -path '*/node_modules/*' -not -path '*/target/*' > ~/.caret-sync/pull-list"), "{pull}");
        assert!(pull.ends_with("find . -type f -not -path './.git/*' -not -path '*/node_modules/*' -not -path '*/target/*'"), "{pull}");
        assert!(pull.contains("cd '/home/agent/work/proj' || exit 1"));
    }

    #[test]
    fn pull_output_parses_including_truncation_and_missing_root() {
        let out = "--caret-sync-changed--\n./gen/out.txt\n./src/main.rs\n--caret-sync-size--\n    512\n--caret-sync-all--\n./src/main.rs\n./gen/out.txt\n./README.md\n";
        let l = parse_pull_output(out, false).unwrap();
        assert_eq!(l.changed, strs(&["gen/out.txt", "src/main.rs"]));
        assert_eq!(l.tarball_size, 512);
        assert_eq!(l.all.unwrap().into_iter().collect::<Vec<_>>(), strs(&["README.md", "gen/out.txt", "src/main.rs"]));
        let l = parse_pull_output(out, true).unwrap();
        assert!(l.all.is_none(), "a truncated listing is not trusted for deletions");
        assert_eq!(l.changed.len(), 2, "the head of the output is still used");
        assert!(parse_pull_output("--caret-sync-no-root--\n", false).unwrap().no_root);
        assert_eq!(parse_pull_output("--caret-sync-changed--\n--caret-sync-size--\n0\n--caret-sync-all--\n", false).unwrap(), PullListing { no_root: false, changed: vec![], tarball_size: 0, all: Some(BTreeSet::new()) });
        assert!(parse_pull_output("garbage", false).is_err());
        assert!(parse_pull_output("--caret-sync-changed--\n./a\n", false).is_err(), "cut before the size");
        assert!(parse_pull_output("--caret-sync-changed--\n--caret-sync-size--\n0\n", false).is_err(), "cut before the listing");
    }

    #[test]
    fn tarballs_round_trip_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a/b.txt", "hello");
        write(dir.path(), "c.bin", "bytes");
        let long = format!("{}/file.txt", "d".repeat(120));
        write(dir.path(), &long, "long");
        let (tgz, n) = build_tarball(dir.path(), &strs(&["a/b.txt", "c.bin", &long, "vanished.txt"])).unwrap();
        assert_eq!(n, 3, "a file that vanished is skipped");
        let files = extract_tarball(&tgz).unwrap();
        assert_eq!(files.iter().map(|(r, b)| (r.as_str(), std::str::from_utf8(b).unwrap())).collect::<Vec<_>>(), vec![("a/b.txt", "hello"), ("c.bin", "bytes"), (long.as_str(), "long")]);
    }

    #[tokio::test]
    async fn push_and_pull_round_trip_through_a_local_shell() {
        let home_dir = tempfile::tempdir().unwrap();
        let local_dir = tempfile::tempdir().unwrap();
        let (home, local) = (home_dir.path(), local_dir.path());
        let remote = LocalShell::new(home);
        let remote_root = home.join("work/proj");
        let binding = SandboxBinding::new("ab12cd34ef", "proj", remote_root.to_string_lossy().as_ref());
        let excludes = strs(&["node_modules", "target"]);
        write(local, "src/main.rs", "fn main() {}");
        write(local, "README.md", "# proj");
        write(local, ".gitignore", "target/\n");
        write(local, ".git/HEAD", "ref: refs/heads/main");
        write(local, "target/debug.bin", "x");
        write(local, "node_modules/x.js", "x");

        // Nothing to do costs nothing.
        assert_eq!(push(&remote, &binding, local, &PushPlan::default()).await.unwrap(), PushReport::default());
        assert_eq!(remote.counts(), (0, 0, 0));

        // First push: everything the scan found.
        let now = scan(local, &excludes).unwrap();
        assert_eq!(keys(&now), vec![".git/HEAD", ".gitignore", "README.md", "src/main.rs"]);
        let report = push(&remote, &binding, local, &plan_push(&Manifest::default(), &now)).await.unwrap();
        assert_eq!((report.files, report.deleted), (4, 0));
        assert!(report.bytes > 0);
        assert_eq!(remote.counts(), (1, 1, 0), "one upload, one exec");
        assert_eq!(std::fs::read_to_string(remote_root.join("src/main.rs")).unwrap(), "fn main() {}");
        assert_eq!(std::fs::read_to_string(remote_root.join(".git/HEAD")).unwrap(), "ref: refs/heads/main");
        assert!(home.join(".caret-sync/marker").is_file());
        assert!(!remote_root.join("target").exists() && !remote_root.join("node_modules").exists());

        // Second push: a change, an addition and a deletion.
        write(local, "src/main.rs", "fn main() { run() }");
        future_mtime(local, "src/main.rs");
        write(local, "docs/x.md", "docs");
        std::fs::remove_file(local.join("README.md")).unwrap();
        let now2 = scan(local, &excludes).unwrap();
        let plan = plan_push(&now, &now2);
        assert_eq!((plan.upload.clone(), plan.delete.clone()), (strs(&["docs/x.md", "src/main.rs"]), strs(&["README.md"])));
        let report = push(&remote, &binding, local, &plan).await.unwrap();
        assert_eq!((report.files, report.deleted), (2, 1));
        assert_eq!(std::fs::read_to_string(remote_root.join("src/main.rs")).unwrap(), "fn main() { run() }");
        assert_eq!(std::fs::read_to_string(remote_root.join("docs/x.md")).unwrap(), "docs");
        assert!(!remote_root.join("README.md").exists());

        // Pull with nothing changed remotely: one exec, no download (the future-stamped file was extracted with `-m`),
        // nothing reported.
        let before = remote.counts();
        let quiet = pull(&remote, &binding, local, &now2, &excludes).await.unwrap();
        assert!(quiet.changed.is_empty() && quiet.deleted.is_empty() && quiet.warnings.is_empty(), "{quiet:?}");
        assert_eq!(remote.counts(), (before.0 + 1, before.1, before.2));

        // The "command" creates, changes, touches and deletes things remotely (some time after the last sync).
        age_marker(home);
        write(&remote_root, "gen/out.txt", "generated");
        write(&remote_root, "src/main.rs", "fn main() { run(); formatted() }");
        write(&remote_root, "docs/x.md", "docs"); // same bytes, newer mtime: not a change
        write(&remote_root, "node_modules/y.js", "dep");
        write(&remote_root, "target/t.bin", "build");
        write(&remote_root, ".git/index", "idx");
        std::fs::remove_file(remote_root.join(".gitignore")).unwrap();
        let before = remote.counts();
        let got = pull(&remote, &binding, local, &now2, &excludes).await.unwrap();
        assert_eq!(got.changed.iter().map(|(r, b)| (r.as_str(), std::str::from_utf8(b).unwrap())).collect::<Vec<_>>(), vec![("gen/out.txt", "generated"), ("src/main.rs", "fn main() { run(); formatted() }")]);
        assert_eq!(got.deleted, strs(&[".gitignore"]));
        assert!(got.bytes > 0 && got.warnings.is_empty());
        assert_eq!(remote.counts(), (before.0 + 1, before.1, before.2 + 1), "one exec and one download");
        assert_eq!(std::fs::read_to_string(local.join("src/main.rs")).unwrap(), "fn main() { run() }", "pull never writes locally");

        // The marker moved: a second pull reports nothing new but still the deletion.
        let again = pull(&remote, &binding, local, &now2, &excludes).await.unwrap();
        assert!(again.changed.is_empty(), "{:?}", again.changed.iter().map(|c| &c.0).collect::<Vec<_>>());
        assert_eq!(again.deleted, strs(&[".gitignore"]));

        // A root that was never pushed pulls nothing and reports no deletions.
        let other = SandboxBinding::new("ab12cd34ef", "other", home.join("work/other").to_string_lossy().as_ref());
        let none = pull(&remote, &other, local, &now2, &excludes).await.unwrap();
        assert_eq!(none, PullReport { ms: none.ms, ..Default::default() });
    }
}

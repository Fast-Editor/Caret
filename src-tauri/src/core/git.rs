//! Git integration: libgit2 for local reads/writes, the `git` CLI for network/stash/worktree ops.
use crate::core::workspace::WorkspaceState;
use crate::error::{CaretError, Result};
use crate::events;
use git2::{BranchType, DiffFormat, DiffOptions, Oid, Repository, StatusOptions};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

// ───────────────────────── types ─────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GitFileStatus { Modified, Added, Deleted, Renamed, Copied, Untracked, Ignored, Conflicted, Typechange }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileChange { pub path: String, #[serde(skip_serializing_if = "Option::is_none")] pub old_path: Option<String>, pub status: GitFileStatus, pub staged: bool }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub staged: Vec<GitFileChange>,
    pub unstaged: Vec<GitFileChange>,
    pub untracked: Vec<GitFileChange>,
    pub conflicted: Vec<GitFileChange>,
    pub is_detached: bool,
    pub head_sha: Option<String>,
    pub state: &'static str,
    pub stash_count: usize,
}
impl GitStatus {
    fn empty() -> Self { Self { is_repo: false, branch: None, upstream: None, ahead: 0, behind: 0, staged: vec![], unstaged: vec![], untracked: vec![], conflicted: vec![], is_detached: false, head_sha: None, state: "normal", stash_count: 0 } }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranch { pub name: String, pub is_current: bool, pub is_remote: bool, #[serde(skip_serializing_if = "Option::is_none")] pub upstream: Option<String>, pub sha: String, #[serde(skip_serializing_if = "Option::is_none")] pub last_commit_time: Option<i64> }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommit { pub sha: String, pub short_sha: String, pub summary: String, pub body: String, pub author: String, pub email: String, pub time: i64, pub parents: Vec<String>, pub refs: Vec<String> }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBlameLine { pub line: u32, pub sha: String, pub short_sha: String, pub author: String, pub time: i64, pub summary: String, pub is_uncommitted: bool }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktree { pub path: String, pub branch: Option<String>, pub head: String, pub is_main: bool, pub is_locked: bool }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRemoteOpResult { pub ok: bool, pub output: String, pub command: String }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffStat { pub files: usize, pub additions: usize, pub deletions: usize }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitFile { pub path: String, pub status: GitFileStatus, pub additions: usize, pub deletions: usize }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRemote { pub name: String, pub url: String }

// ───────────────────────── helpers ─────────────────────────

fn root_of(app: &AppHandle) -> Result<PathBuf> { app.state::<WorkspaceState>().require_root() }

fn open(root: &Path) -> Result<Repository> {
    Repository::discover(root).map_err(|_| CaretError::new("not_a_repo", format!("{} is not inside a git repository", root.display())))
}
fn workdir(repo: &Repository) -> PathBuf { repo.workdir().map(Path::to_path_buf).unwrap_or_else(|| repo.path().to_path_buf()) }
fn rel(repo: &Repository, p: &str) -> PathBuf {
    let wd = workdir(repo);
    let pb = Path::new(p);
    if pb.is_absolute() { pb.strip_prefix(&wd).map(Path::to_path_buf).unwrap_or_else(|_| pb.to_path_buf()) } else { pb.to_path_buf() }
}
fn abs(repo: &Repository, p: &Path) -> String { workdir(repo).join(p).to_string_lossy().to_string() }

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f).await.map_err(|e| CaretError::internal(e.to_string()))?
}
fn changed(app: &AppHandle) { events::emit(app, events::GIT_CHANGED, serde_json::Value::Null); }

fn head_commit(repo: &Repository) -> Option<git2::Commit<'_>> { repo.head().ok().and_then(|h| h.peel_to_commit().ok()) }

fn signature(repo: &Repository) -> git2::Signature<'static> {
    repo.signature().or_else(|_| git2::Signature::now("Caret", "caret@localhost")).expect("static signature")
}

fn state_name(s: git2::RepositoryState) -> &'static str {
    use git2::RepositoryState::*;
    match s { Clean => "normal", Merge => "merge", Revert | RevertSequence => "revert", CherryPick | CherryPickSequence => "cherrypick", Bisect => "bisect", Rebase | RebaseInteractive | RebaseMerge | ApplyMailbox | ApplyMailboxOrRebase => "rebase" }
}

fn delta_status(d: git2::Delta) -> GitFileStatus {
    use git2::Delta::*;
    match d { Added => GitFileStatus::Added, Deleted => GitFileStatus::Deleted, Renamed => GitFileStatus::Renamed, Copied => GitFileStatus::Copied, Untracked => GitFileStatus::Untracked, Ignored => GitFileStatus::Ignored, Conflicted => GitFileStatus::Conflicted, Typechange => GitFileStatus::Typechange, _ => GitFileStatus::Modified }
}

// ───────────────────────── status ─────────────────────────

pub fn status_sync(root: &Path) -> Result<GitStatus> {
    let mut repo = match Repository::discover(root) { Ok(r) => r, Err(_) => return Ok(GitStatus::empty()) };
    let mut st = GitStatus { is_repo: true, ..GitStatus::empty() };
    st.state = state_name(repo.state());
    // HEAD / branch
    match repo.head() {
        Ok(h) => {
            st.head_sha = h.target().map(|o| o.to_string());
            if h.is_branch() { st.branch = h.shorthand().ok().map(String::from); } else { st.is_detached = true; st.branch = h.target().map(|o| o.to_string()[..7].to_string()); }
        }
        Err(_) => { // unborn
            if let Ok(r) = repo.find_reference("HEAD") { st.branch = r.symbolic_target().ok().flatten().and_then(|t| t.strip_prefix("refs/heads/")).map(String::from); }
        }
    }
    if let (Some(b), false) = (st.branch.clone(), st.is_detached) {
        if let Ok(local) = repo.find_branch(&b, BranchType::Local) {
            if let Ok(up) = local.upstream() {
                st.upstream = up.name().ok().flatten().map(String::from);
                if let (Some(l), Some(u)) = (local.get().target(), up.get().target()) { if let Ok((a, be)) = repo.graph_ahead_behind(l, u) { st.ahead = a; st.behind = be; } }
            }
        }
    }
    // file statuses
    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true).include_ignored(false).renames_head_to_index(true).renames_index_to_workdir(true).exclude_submodules(true);
    let statuses = repo.statuses(Some(&mut opts))?;
    for e in statuses.iter() {
        let s = e.status();
        let path = e.path().ok().map(String::from).or_else(|| e.index_to_workdir().and_then(|d| d.new_file().path().map(|p| p.to_string_lossy().to_string()))).or_else(|| e.head_to_index().and_then(|d| d.new_file().path().map(|p| p.to_string_lossy().to_string()))).unwrap_or_default();
        let apath = abs(&repo, Path::new(&path));
        if s.is_conflicted() { st.conflicted.push(GitFileChange { path: apath, old_path: None, status: GitFileStatus::Conflicted, staged: false }); continue; }
        if s.intersects(git2::Status::INDEX_NEW | git2::Status::INDEX_MODIFIED | git2::Status::INDEX_DELETED | git2::Status::INDEX_RENAMED | git2::Status::INDEX_TYPECHANGE) {
            let status = if s.is_index_new() { GitFileStatus::Added } else if s.is_index_deleted() { GitFileStatus::Deleted } else if s.is_index_renamed() { GitFileStatus::Renamed } else if s.is_index_typechange() { GitFileStatus::Typechange } else { GitFileStatus::Modified };
            let old_path = if s.is_index_renamed() { e.head_to_index().and_then(|d| d.old_file().path().map(|p| abs(&repo, p))) } else { None };
            st.staged.push(GitFileChange { path: apath.clone(), old_path, status, staged: true });
        }
        if s.is_wt_new() { st.untracked.push(GitFileChange { path: apath, old_path: None, status: GitFileStatus::Untracked, staged: false }); }
        else if s.intersects(git2::Status::WT_MODIFIED | git2::Status::WT_DELETED | git2::Status::WT_RENAMED | git2::Status::WT_TYPECHANGE) {
            let status = if s.is_wt_deleted() { GitFileStatus::Deleted } else if s.is_wt_renamed() { GitFileStatus::Renamed } else if s.is_wt_typechange() { GitFileStatus::Typechange } else { GitFileStatus::Modified };
            let old_path = if s.is_wt_renamed() { e.index_to_workdir().and_then(|d| d.old_file().path().map(|p| abs(&repo, p))) } else { None };
            st.unstaged.push(GitFileChange { path: apath, old_path, status, staged: false });
        }
    }
    drop(statuses);
    let mut n = 0usize; let _ = repo.stash_foreach(|_, _, _| { n += 1; true }); st.stash_count = n;
    Ok(st)
}

#[tauri::command]
pub async fn git_status(app: AppHandle) -> Result<GitStatus> {
    let root = match root_of(&app) { Ok(r) => r, Err(_) => return Ok(GitStatus::empty()) };
    blocking(move || status_sync(&root)).await
}

// ───────────────────────── diff ─────────────────────────

const MAX_DIFF_BYTES: usize = 4 * 1024 * 1024;

fn diff_to_text(diff: &git2::Diff) -> Result<String> {
    let mut out = String::new();
    let mut truncated = false;
    diff.print(DiffFormat::Patch, |_, _, line| {
        if out.len() > MAX_DIFF_BYTES { truncated = true; return false; }
        match line.origin() { '+' | '-' | ' ' => out.push(line.origin()), _ => {} }
        out.push_str(&String::from_utf8_lossy(line.content()));
        true
    }).or_else(|e| if truncated { Ok(()) } else { Err(e) })?;
    if truncated { out.push_str("\n… diff truncated (4 MiB limit) …\n"); }
    Ok(out)
}

fn make_diff<'r>(repo: &'r Repository, path: Option<&str>, staged: bool, base: Option<&str>) -> Result<git2::Diff<'r>> {
    let mut opts = DiffOptions::new();
    opts.context_lines(3).include_untracked(true).recurse_untracked_dirs(true).show_untracked_content(true).include_typechange(true);
    if let Some(p) = path { opts.pathspec(rel(repo, p)); }
    let head_tree = head_commit(repo).and_then(|c| c.tree().ok());
    let diff = if let Some(b) = base {
        let tree = repo.revparse_single(b)?.peel_to_tree()?;
        repo.diff_tree_to_workdir_with_index(Some(&tree), Some(&mut opts))?
    } else if staged {
        let index = repo.index()?;
        repo.diff_tree_to_index(head_tree.as_ref(), Some(&index), Some(&mut opts))?
    } else {
        repo.diff_index_to_workdir(None, Some(&mut opts))?
    };
    Ok(diff)
}

pub fn diff_sync(root: &Path, path: Option<&str>, staged: bool, base: Option<&str>) -> Result<String> {
    let repo = open(root)?;
    let mut diff = make_diff(&repo, path, staged, base)?;
    let _ = diff.find_similar(None);
    diff_to_text(&diff)
}

#[tauri::command]
pub async fn git_diff(app: AppHandle, path: Option<String>, staged: Option<bool>, base: Option<String>) -> Result<String> {
    let root = root_of(&app)?;
    blocking(move || diff_sync(&root, path.as_deref(), staged.unwrap_or(false), base.as_deref())).await
}

#[tauri::command]
pub async fn git_diff_stat(app: AppHandle, base: Option<String>) -> Result<GitDiffStat> {
    let root = root_of(&app)?;
    blocking(move || {
        let repo = open(&root)?;
        let mut opts = DiffOptions::new();
        opts.include_untracked(true).recurse_untracked_dirs(true).show_untracked_content(true);
        let tree = match base.as_deref() { Some(b) => Some(repo.revparse_single(b)?.peel_to_tree()?), None => head_commit(&repo).and_then(|c| c.tree().ok()) };
        let diff = repo.diff_tree_to_workdir_with_index(tree.as_ref(), Some(&mut opts))?;
        let s = diff.stats()?;
        Ok(GitDiffStat { files: s.files_changed(), additions: s.insertions(), deletions: s.deletions() })
    }).await
}

// ───────────────────────── index ops ─────────────────────────

pub fn stage_sync(root: &Path, paths: &[String]) -> Result<()> {
    let repo = open(root)?;
    let mut index = repo.index()?;
    let wd = workdir(&repo);
    for p in paths {
        let r = rel(&repo, p);
        if wd.join(&r).exists() {
            if wd.join(&r).is_dir() { index.add_all([r.to_string_lossy().to_string() + "/**"], git2::IndexAddOption::DEFAULT, None)?; index.update_all([r.to_string_lossy().to_string()], None)?; }
            else { index.add_path(&r)?; }
        } else { let _ = index.remove_path(&r); }
    }
    index.write()?;
    Ok(())
}

pub fn unstage_sync(root: &Path, paths: Option<&[String]>) -> Result<()> {
    let repo = open(root)?;
    let head = head_commit(&repo).map(|c| c.into_object());
    let rels: Vec<PathBuf> = match paths {
        Some(ps) => ps.iter().map(|p| rel(&repo, p)).collect(),
        None => { let st = status_sync(root)?; st.staged.iter().map(|c| rel(&repo, &c.path)).chain(st.staged.iter().filter_map(|c| c.old_path.as_ref().map(|o| rel(&repo, o)))).collect() }
    };
    if rels.is_empty() { return Ok(()); }
    match head {
        Some(obj) => repo.reset_default(Some(&obj), rels.iter())?,
        None => { let mut index = repo.index()?; for r in &rels { let _ = index.remove_path(r); } index.write()?; }
    }
    Ok(())
}

#[tauri::command]
pub async fn git_stage(app: AppHandle, paths: Vec<String>) -> Result<()> { let root = root_of(&app)?; blocking(move || stage_sync(&root, &paths)).await?; changed(&app); Ok(()) }
#[tauri::command]
pub async fn git_unstage(app: AppHandle, paths: Vec<String>) -> Result<()> { let root = root_of(&app)?; blocking(move || unstage_sync(&root, Some(&paths))).await?; changed(&app); Ok(()) }
#[tauri::command]
pub async fn git_stage_all(app: AppHandle) -> Result<()> {
    let root = root_of(&app)?;
    blocking(move || { let repo = open(&root)?; let mut index = repo.index()?; index.add_all(["*"], git2::IndexAddOption::DEFAULT, None)?; index.update_all(["*"], None)?; index.write()?; Ok(()) }).await?;
    changed(&app); Ok(())
}
#[tauri::command]
pub async fn git_unstage_all(app: AppHandle) -> Result<()> { let root = root_of(&app)?; blocking(move || unstage_sync(&root, None)).await?; changed(&app); Ok(()) }

#[tauri::command]
pub async fn git_discard(app: AppHandle, paths: Vec<String>) -> Result<()> {
    let root = root_of(&app)?;
    blocking(move || {
        let repo = open(&root)?;
        let wd = workdir(&repo);
        let st = status_sync(&root)?;
        let untracked: std::collections::HashSet<String> = st.untracked.iter().map(|c| c.path.clone()).collect();
        for p in &paths {
            let r = rel(&repo, p);
            let a = wd.join(&r);
            if untracked.contains(&a.to_string_lossy().to_string()) || untracked.contains(p) {
                if a.is_dir() { std::fs::remove_dir_all(&a)?; } else if a.exists() { std::fs::remove_file(&a)?; }
            } else {
                let mut cb = git2::build::CheckoutBuilder::new();
                cb.force().remove_untracked(false).path(&r);
                repo.checkout_index(None, Some(&mut cb))?;
            }
        }
        Ok(())
    }).await?;
    changed(&app); Ok(())
}

pub fn commit_sync(root: &Path, message: &str, amend: bool, signoff: bool) -> Result<String> {
    let repo = open(root)?;
    let sig = signature(&repo);
    let mut msg = message.trim_end().to_string();
    if signoff { msg.push_str(&format!("\n\nSigned-off-by: {} <{}>", sig.name().unwrap_or("Caret"), sig.email().unwrap_or("caret@localhost"))); }
    if msg.trim().is_empty() { return Err(CaretError::invalid("Commit message is empty")); }
    let mut index = repo.index()?;
    index.read(false)?;
    let tree_oid = index.write_tree()?;
    let tree = repo.find_tree(tree_oid)?;
    let head = head_commit(&repo);
    if amend {
        let h = head.ok_or_else(|| CaretError::invalid("Nothing to amend"))?;
        let oid = h.amend(Some("HEAD"), None, Some(&sig), None, Some(&msg), Some(&tree))?;
        return Ok(oid.to_string());
    }
    if let Some(h) = &head { if h.tree_id() == tree_oid { return Err(CaretError::invalid("No staged changes to commit")); } }
    let parents: Vec<&git2::Commit> = head.iter().collect();
    let oid = repo.commit(Some("HEAD"), &sig, &sig, &msg, &tree, &parents)?;
    Ok(oid.to_string())
}

#[tauri::command]
pub async fn git_commit(app: AppHandle, message: String, amend: Option<bool>, signoff: Option<bool>) -> Result<String> {
    let root = root_of(&app)?;
    let sha = blocking(move || commit_sync(&root, &message, amend.unwrap_or(false), signoff.unwrap_or(false))).await?;
    changed(&app);
    Ok(sha)
}

// ───────────────────────── history ─────────────────────────

fn ref_decorations(repo: &Repository) -> HashMap<Oid, Vec<String>> {
    let mut m: HashMap<Oid, Vec<String>> = HashMap::new();
    if let Ok(refs) = repo.references() {
        for r in refs.flatten() {
            let Ok(name) = r.name() else { continue };
            let label = if let Some(b) = name.strip_prefix("refs/heads/") { b.to_string() } else if let Some(b) = name.strip_prefix("refs/remotes/") { if b.ends_with("/HEAD") { continue; } b.to_string() } else if let Some(t) = name.strip_prefix("refs/tags/") { format!("tag: {t}") } else { continue };
            let oid = r.target().or_else(|| r.peel_to_commit().ok().map(|c| c.id()));
            if let Some(o) = oid { m.entry(o).or_default().push(label); }
        }
    }
    if let Ok(h) = repo.head() { if let Some(o) = h.target() { m.entry(o).or_default().insert(0, if h.is_branch() { "HEAD".into() } else { "HEAD (detached)".into() }); } }
    m
}

fn to_commit(c: &git2::Commit, decos: &HashMap<Oid, Vec<String>>) -> GitCommit {
    let msg = c.message().unwrap_or("").to_string();
    let (summary, body) = match msg.split_once('\n') { Some((s, b)) => (s.trim().to_string(), b.trim().to_string()), None => (msg.trim().to_string(), String::new()) };
    GitCommit { sha: c.id().to_string(), short_sha: c.id().to_string()[..7].to_string(), summary, body, author: c.author().name().unwrap_or("").to_string(), email: c.author().email().unwrap_or("").to_string(), time: c.time().seconds(), parents: c.parent_ids().map(|o| o.to_string()).collect(), refs: decos.get(&c.id()).cloned().unwrap_or_default() }
}

fn touches_path(repo: &Repository, c: &git2::Commit, rel_path: &Path) -> bool {
    let cur = c.tree().ok().and_then(|t| t.get_path(rel_path).ok()).map(|e| e.id());
    if c.parent_count() == 0 { return cur.is_some(); }
    for p in c.parents() {
        let prev = p.tree().ok().and_then(|t| t.get_path(rel_path).ok()).map(|e| e.id());
        if prev != cur { return true; }
    }
    let _ = repo; false
}

#[tauri::command]
pub async fn git_log(app: AppHandle, limit: Option<usize>, skip: Option<usize>, path: Option<String>, rev: Option<String>) -> Result<Vec<GitCommit>> {
    let root = root_of(&app)?;
    blocking(move || {
        let repo = open(&root)?;
        let mut walk = repo.revwalk()?;
        walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)?;
        match rev.as_deref() { Some(r) if !r.is_empty() => { let o = repo.revparse_single(r)?; walk.push(o.id())?; } _ => { if walk.push_head().is_err() { return Ok(vec![]); } } }
        let decos = ref_decorations(&repo);
        let limit = limit.unwrap_or(50).min(5000); let skip = skip.unwrap_or(0);
        let rel_path = path.as_deref().map(|p| rel(&repo, p));
        let mut out = Vec::with_capacity(limit);
        let mut seen = 0usize; let mut scanned = 0usize;
        for oid in walk.flatten() {
            scanned += 1; if scanned > 200_000 { break; }
            let Ok(c) = repo.find_commit(oid) else { continue };
            if let Some(rp) = &rel_path { if !touches_path(&repo, &c, rp) { continue; } }
            if seen < skip { seen += 1; continue; }
            out.push(to_commit(&c, &decos));
            if out.len() >= limit { break; }
        }
        Ok(out)
    }).await
}

#[tauri::command]
pub async fn git_commit_files(app: AppHandle, sha: String) -> Result<Vec<GitCommitFile>> {
    let root = root_of(&app)?;
    blocking(move || {
        let repo = open(&root)?;
        let commit = repo.revparse_single(&sha)?.peel_to_commit()?;
        let tree = commit.tree()?;
        let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
        let mut diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)?;
        let _ = diff.find_similar(None);
        let mut files: Vec<GitCommitFile> = vec![];
        for d in diff.deltas() {
            let p = d.new_file().path().or(d.old_file().path()).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            files.push(GitCommitFile { path: abs(&repo, Path::new(&p)), status: delta_status(d.status()), additions: 0, deletions: 0 });
        }
        let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
        diff.print(DiffFormat::Patch, |d, _, l| {
            let p = d.new_file().path().or(d.old_file().path()).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            let e = counts.entry(p).or_insert((0, 0));
            match l.origin() { '+' => e.0 += 1, '-' => e.1 += 1, _ => {} }
            true
        })?;
        let wd = workdir(&repo);
        for f in &mut files { let relp = Path::new(&f.path).strip_prefix(&wd).map(|p| p.to_string_lossy().to_string()).unwrap_or_default(); if let Some((a, d)) = counts.get(&relp) { f.additions = *a; f.deletions = *d; } }
        Ok(files)
    }).await
}

#[tauri::command]
pub async fn git_show_file(app: AppHandle, path: String, rev: String) -> Result<String> {
    let root = root_of(&app)?;
    blocking(move || {
        let repo = open(&root)?;
        let r = rel(&repo, &path);
        if rev == ":0" || rev.eq_ignore_ascii_case("INDEX") || rev == ":" {
            // staged (index) content
            let index = repo.index()?;
            let entry = index.get_path(&r, 0).ok_or_else(|| CaretError::not_found(format!("{} in index", r.display())))?;
            let blob = repo.find_blob(entry.id)?;
            if blob.is_binary() { return Err(CaretError::invalid("binary file")); }
            return Ok(String::from_utf8_lossy(blob.content()).to_string());
        }
        let spec = format!("{}:{}", if rev.is_empty() { "HEAD" } else { rev.as_str() }, r.to_string_lossy());
        let obj = repo.revparse_single(&spec).map_err(|_| CaretError::not_found(format!("{spec}")))?;
        let blob = obj.peel_to_blob()?;
        if blob.is_binary() { return Err(CaretError::invalid("binary file")); }
        Ok(String::from_utf8_lossy(blob.content()).to_string())
    }).await
}

#[tauri::command]
pub async fn git_blame(app: AppHandle, path: String) -> Result<Vec<GitBlameLine>> {
    let root = root_of(&app)?;
    blocking(move || {
        let repo = open(&root)?;
        let r = rel(&repo, &path);
        let absp = workdir(&repo).join(&r);
        let mut opts = git2::BlameOptions::new();
        let base = repo.blame_file(&r, Some(&mut opts))?;
        let content = std::fs::read(&absp)?;
        let blame = base.blame_buffer(&content)?;
        let mut cache: HashMap<Oid, (String, i64, String)> = HashMap::new();
        let mut out = vec![];
        for h in blame.iter() {
            let oid = h.final_commit_id();
            let unc = oid.is_zero();
            let (author, time, summary) = if unc { ("You".to_string(), 0, "Uncommitted changes".to_string()) } else {
                cache.entry(oid).or_insert_with(|| {
                    let c = repo.find_commit(oid).ok();
                    (c.as_ref().and_then(|c| c.author().name().ok().map(String::from)).unwrap_or_default(), c.as_ref().map(|c| c.time().seconds()).unwrap_or(0), c.as_ref().and_then(|c| c.summary().ok().flatten().map(String::from)).unwrap_or_default())
                }).clone()
            };
            let start = h.final_start_line();
            for i in 0..h.lines_in_hunk() {
                out.push(GitBlameLine { line: (start + i) as u32, sha: oid.to_string(), short_sha: if unc { "0000000".into() } else { oid.to_string()[..7].to_string() }, author: author.clone(), time, summary: summary.clone(), is_uncommitted: unc });
            }
        }
        out.sort_by_key(|l| l.line);
        Ok(out)
    }).await
}

// ───────────────────────── branches ─────────────────────────

#[tauri::command]
pub async fn git_branches(app: AppHandle) -> Result<Vec<GitBranch>> {
    let root = root_of(&app)?;
    blocking(move || {
        let repo = open(&root)?;
        let mut out = vec![];
        for (b, t) in repo.branches(None)?.flatten() {
            let Some(name) = b.name().ok().flatten().map(String::from) else { continue };
            let is_remote = t == BranchType::Remote;
            if is_remote && name.ends_with("/HEAD") { continue; }
            let commit = b.get().peel_to_commit().ok();
            let sha = commit.as_ref().map(|c| c.id().to_string()).unwrap_or_default();
            let upstream = if is_remote { None } else { b.upstream().ok().and_then(|u| u.name().ok().flatten().map(String::from)) };
            out.push(GitBranch { name, is_current: b.is_head(), is_remote, upstream, sha, last_commit_time: commit.map(|c| c.time().seconds()) });
        }
        out.sort_by(|a, b| b.is_current.cmp(&a.is_current).then(a.is_remote.cmp(&b.is_remote)).then(b.last_commit_time.cmp(&a.last_commit_time)));
        Ok(out)
    }).await
}

pub fn checkout_sync(root: &Path, reference: &str, create: bool) -> Result<()> {
    let repo = open(root)?;
    if create {
        let head = head_commit(&repo).ok_or_else(|| CaretError::invalid("Repository has no commits yet"))?;
        repo.branch(reference, &head, false)?;
    }
    let (object, refname): (git2::Object, Option<String>) = if let Ok(b) = repo.find_branch(reference, BranchType::Local) {
        (b.get().peel(git2::ObjectType::Commit)?, Some(format!("refs/heads/{reference}")))
    } else if let Ok(rb) = repo.find_branch(reference, BranchType::Remote) {
        let local_name = reference.split_once('/').map(|(_, n)| n.to_string()).unwrap_or_else(|| reference.to_string());
        let commit = rb.get().peel_to_commit()?;
        let mut lb = match repo.find_branch(&local_name, BranchType::Local) { Ok(b) => b, Err(_) => repo.branch(&local_name, &commit, false)? };
        let _ = lb.set_upstream(Some(reference));
        (lb.get().peel(git2::ObjectType::Commit)?, Some(format!("refs/heads/{local_name}")))
    } else {
        (repo.revparse_single(reference)?.peel(git2::ObjectType::Commit)?, None)
    };
    let mut cb = git2::build::CheckoutBuilder::new();
    cb.safe();
    repo.checkout_tree(&object, Some(&mut cb)).map_err(|e| CaretError::new("git_conflict", format!("Checkout failed: {}. Commit or stash your changes first.", e.message())))?;
    match refname { Some(r) => repo.set_head(&r)?, None => repo.set_head_detached(object.id())? }
    Ok(())
}

#[tauri::command]
pub async fn git_checkout(app: AppHandle, r#ref: String, create: Option<bool>) -> Result<()> {
    let root = root_of(&app)?;
    blocking(move || checkout_sync(&root, &r#ref, create.unwrap_or(false))).await?;
    changed(&app); Ok(())
}

#[tauri::command]
pub async fn git_create_branch(app: AppHandle, name: String, checkout: Option<bool>) -> Result<()> {
    let root = root_of(&app)?;
    blocking(move || {
        let repo = open(&root)?;
        let head = head_commit(&repo).ok_or_else(|| CaretError::invalid("Repository has no commits yet"))?;
        repo.branch(&name, &head, false)?;
        if checkout.unwrap_or(true) { repo.set_head(&format!("refs/heads/{name}"))?; }
        Ok(())
    }).await?;
    changed(&app); Ok(())
}

#[tauri::command]
pub async fn git_delete_branch(app: AppHandle, name: String, force: Option<bool>) -> Result<()> {
    let root = root_of(&app)?;
    blocking(move || {
        let repo = open(&root)?;
        let mut b = repo.find_branch(&name, BranchType::Local).map_err(|_| CaretError::not_found(format!("branch {name}")))?;
        if b.is_head() { return Err(CaretError::invalid("Cannot delete the current branch")); }
        if !force.unwrap_or(false) {
            if let (Some(bo), Some(ho)) = (b.get().target(), repo.head().ok().and_then(|h| h.target())) {
                if let Ok((ahead, _)) = repo.graph_ahead_behind(bo, ho) { if ahead > 0 { return Err(CaretError::new("unmerged", format!("Branch '{name}' is not fully merged ({ahead} commits). Force delete to discard them."))); } }
            }
        }
        b.delete()?;
        Ok(())
    }).await?;
    changed(&app); Ok(())
}

#[tauri::command]
pub async fn git_remotes(app: AppHandle) -> Result<Vec<GitRemote>> {
    let root = root_of(&app)?;
    blocking(move || { let repo = open(&root)?; let names = repo.remotes()?; Ok(names.iter().flatten().flatten().filter_map(|n| repo.find_remote(n).ok().map(|r| GitRemote { name: n.to_string(), url: r.url().unwrap_or("").to_string() })).collect()) }).await
}

#[tauri::command]
pub async fn git_default_branch(app: AppHandle) -> Result<Option<String>> {
    let root = root_of(&app)?;
    blocking(move || {
        let repo = open(&root)?;
        if let Ok(r) = repo.find_reference("refs/remotes/origin/HEAD") { if let Some(t) = r.symbolic_target().ok().flatten() { return Ok(t.strip_prefix("refs/remotes/origin/").map(String::from)); } }
        for c in ["main", "master", "develop", "trunk"] { if repo.find_branch(c, BranchType::Local).is_ok() { return Ok(Some(c.to_string())); } }
        Ok(None)
    }).await
}

#[tauri::command]
pub async fn git_init(app: AppHandle) -> Result<()> {
    let root = root_of(&app)?;
    blocking(move || { Repository::init(&root)?; Ok(()) }).await?;
    changed(&app); Ok(())
}

// ───────────────────────── git CLI ops ─────────────────────────

pub async fn run_git(root: &Path, args: &[&str], timeout: std::time::Duration) -> Result<GitRemoteOpResult> {
    let command = format!("git {}", args.join(" "));
    let fut = tokio::process::Command::new("git").args(args).current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0").env("LC_ALL", "C")
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true).output();
    match tokio::time::timeout(timeout, fut).await {
        Ok(Ok(out)) => {
            let mut text = String::from_utf8_lossy(&out.stdout).to_string();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() { if !text.is_empty() { text.push('\n'); } text.push_str(err.trim_end()); }
            if text.len() > 64 * 1024 { text.truncate(64 * 1024); text.push_str("\n…"); }
            Ok(GitRemoteOpResult { ok: out.status.success(), output: text, command })
        }
        Ok(Err(e)) => Ok(GitRemoteOpResult { ok: false, output: format!("failed to run git: {e}"), command }),
        Err(_) => Ok(GitRemoteOpResult { ok: false, output: format!("timed out after {}s", timeout.as_secs()), command }),
    }
}
const NET_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

#[tauri::command]
pub async fn git_push(app: AppHandle, remote: Option<String>, branch: Option<String>, set_upstream: Option<bool>, force: Option<bool>) -> Result<GitRemoteOpResult> {
    let root = root_of(&app)?;
    let mut args: Vec<String> = vec!["push".into()];
    if force.unwrap_or(false) { args.push("--force-with-lease".into()); }
    if set_upstream.unwrap_or(false) { args.push("-u".into()); }
    if let Some(r) = remote.filter(|r| !r.is_empty()) { args.push(r); if let Some(b) = branch.filter(|b| !b.is_empty()) { args.push(b); } }
    else if set_upstream.unwrap_or(false) { args.push("origin".into()); if let Some(b) = branch.filter(|b| !b.is_empty()) { args.push(b); } else if let Some(cur) = status_sync(&root).ok().and_then(|s| s.branch) { args.push(cur); } }
    let r = run_git(&root, &args.iter().map(String::as_str).collect::<Vec<_>>(), NET_TIMEOUT).await?;
    changed(&app); Ok(r)
}
#[tauri::command]
pub async fn git_pull(app: AppHandle, rebase: Option<bool>) -> Result<GitRemoteOpResult> {
    let root = root_of(&app)?;
    let args: Vec<&str> = if rebase.unwrap_or(false) { vec!["pull", "--rebase"] } else { vec!["pull"] };
    let r = run_git(&root, &args, NET_TIMEOUT).await?; changed(&app); Ok(r)
}
#[tauri::command]
pub async fn git_fetch(app: AppHandle) -> Result<GitRemoteOpResult> { let root = root_of(&app)?; let r = run_git(&root, &["fetch", "--all", "--prune"], NET_TIMEOUT).await?; changed(&app); Ok(r) }
#[tauri::command]
pub async fn git_stash(app: AppHandle, message: Option<String>) -> Result<GitRemoteOpResult> {
    let root = root_of(&app)?;
    let mut args = vec!["stash", "push", "--include-untracked"];
    let m = message.unwrap_or_default();
    if !m.is_empty() { args.push("-m"); args.push(&m); }
    let r = run_git(&root, &args, NET_TIMEOUT).await?; changed(&app); Ok(r)
}
#[tauri::command]
pub async fn git_stash_pop(app: AppHandle) -> Result<GitRemoteOpResult> { let root = root_of(&app)?; let r = run_git(&root, &["stash", "pop"], NET_TIMEOUT).await?; changed(&app); Ok(r) }

pub fn parse_worktrees(porcelain: &str) -> Vec<GitWorktree> {
    let mut out = vec![];
    for (i, block) in porcelain.split("\n\n").filter(|b| !b.trim().is_empty()).enumerate() {
        let mut wt = GitWorktree { path: String::new(), branch: None, head: String::new(), is_main: i == 0, is_locked: false };
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") { wt.path = p.to_string(); }
            else if let Some(h) = line.strip_prefix("HEAD ") { wt.head = h.to_string(); }
            else if let Some(b) = line.strip_prefix("branch ") { wt.branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string()); }
            else if line.starts_with("locked") { wt.is_locked = true; }
        }
        if !wt.path.is_empty() { out.push(wt); }
    }
    out
}

#[tauri::command]
pub async fn git_worktrees(app: AppHandle) -> Result<Vec<GitWorktree>> {
    let root = root_of(&app)?;
    let r = run_git(&root, &["worktree", "list", "--porcelain"], std::time::Duration::from_secs(20)).await?;
    if !r.ok { return Err(CaretError::new("git", r.output)); }
    Ok(parse_worktrees(&r.output))
}

#[tauri::command]
pub async fn git_worktree_add(app: AppHandle, path: String, branch: String, create_branch: Option<bool>) -> Result<GitWorktree> {
    let root = root_of(&app)?;
    let target = crate::paths::resolve(Some(&root), &path);
    if let Some(parent) = target.parent() { std::fs::create_dir_all(parent)?; }
    let t = target.to_string_lossy().to_string();
    let args: Vec<&str> = if create_branch.unwrap_or(true) { vec!["worktree", "add", "-b", &branch, &t] } else { vec!["worktree", "add", &t, &branch] };
    let r = run_git(&root, &args, std::time::Duration::from_secs(60)).await?;
    if !r.ok { return Err(CaretError::new("git", r.output)); }
    changed(&app);
    let list = run_git(&root, &["worktree", "list", "--porcelain"], std::time::Duration::from_secs(20)).await?;
    let canon = std::fs::canonicalize(&target).unwrap_or(target.clone());
    Ok(parse_worktrees(&list.output).into_iter().find(|w| Path::new(&w.path) == canon || Path::new(&w.path) == target).unwrap_or(GitWorktree { path: t, branch: Some(branch), head: String::new(), is_main: false, is_locked: false }))
}

#[tauri::command]
pub async fn git_worktree_remove(app: AppHandle, path: String, force: Option<bool>) -> Result<()> {
    let root = root_of(&app)?;
    let target = crate::paths::resolve(Some(&root), &path).to_string_lossy().to_string();
    let mut args = vec!["worktree", "remove"]; if force.unwrap_or(false) { args.push("--force"); } args.push(&target);
    let r = run_git(&root, &args, std::time::Duration::from_secs(60)).await?;
    if !r.ok { return Err(CaretError::new("git", r.output)); }
    let _ = run_git(&root, &["worktree", "prune"], std::time::Duration::from_secs(20)).await;
    changed(&app); Ok(())
}

// ───────────────────────── tests ─────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    fn repo_fixture() -> (tempfile::TempDir, Repository) {
        let d = tempfile::tempdir().unwrap();
        let repo = Repository::init(d.path()).unwrap();
        { let mut cfg = repo.config().unwrap(); cfg.set_str("user.name", "Tester").unwrap(); cfg.set_str("user.email", "t@example.com").unwrap(); }
        std::fs::write(d.path().join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(d.path().join("b.txt"), "b\n").unwrap();
        stage_sync(d.path(), &["a.txt".into(), "b.txt".into()]).unwrap();
        {
            let sig = signature(&repo);
            let tree = { let mut i = repo.index().unwrap(); let t = i.write_tree().unwrap(); repo.find_tree(t).unwrap() };
            repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[]).unwrap();
        }
        (d, repo)
    }
    #[test]
    fn status_diff_stage_commit_flow() {
        let (d, _repo) = repo_fixture();
        let root = d.path();
        let st = status_sync(root).unwrap();
        assert!(st.is_repo); assert!(st.branch.is_some(), "branch should be known on a fresh repo");
        assert!(st.staged.is_empty() && st.unstaged.is_empty() && st.untracked.is_empty());
        std::fs::write(root.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(root.join("c.txt"), "new\n").unwrap();
        let st = status_sync(root).unwrap();
        assert_eq!(st.unstaged.len(), 1); assert_eq!(st.unstaged[0].status, GitFileStatus::Modified);
        assert_eq!(st.untracked.len(), 1); assert!(st.untracked[0].path.ends_with("c.txt"));
        let diff = diff_sync(root, None, false, None).unwrap();
        assert!(diff.contains("-two") && diff.contains("+TWO") && diff.contains("+three"), "{diff}");
        assert!(diff.contains("c.txt") && diff.contains("+new"), "untracked content shown: {diff}");
        stage_sync(root, &["a.txt".into()]).unwrap();
        let st = status_sync(root).unwrap();
        assert_eq!(st.staged.len(), 1); assert!(st.unstaged.is_empty());
        let sd = diff_sync(root, None, true, None).unwrap(); assert!(sd.contains("+TWO"));
        unstage_sync(root, Some(&["a.txt".into()])).unwrap();
        assert!(status_sync(root).unwrap().staged.is_empty());
        stage_sync(root, &["a.txt".into(), "c.txt".into()]).unwrap();
        assert!(commit_sync(root, "   ", false, false).is_err(), "empty message rejected");
        let sha = commit_sync(root, "second change\n\nbody here", false, true).unwrap();
        assert_eq!(sha.len(), 40);
        assert!(commit_sync(root, "nothing", false, false).is_err(), "no staged changes rejected");
        let st = status_sync(root).unwrap();
        assert!(st.staged.is_empty() && st.untracked.is_empty());
        // decorations + log
        let repo = open(root).unwrap();
        let decos = ref_decorations(&repo);
        let head = head_commit(&repo).unwrap();
        let c = to_commit(&head, &decos);
        assert_eq!(c.summary, "second change"); assert!(c.body.starts_with("body here") && c.body.contains("Signed-off-by: Tester")); assert!(c.refs.iter().any(|r| r == "HEAD"));
        assert!(touches_path(&repo, &head, Path::new("a.txt")));
        assert!(!touches_path(&repo, &head, Path::new("b.txt")));
    }
    #[test]
    fn checkout_and_branches() {
        let (d, repo) = repo_fixture();
        checkout_sync(d.path(), "feature", true).unwrap();
        assert_eq!(repo.head().unwrap().shorthand().ok(), Some("feature"));
        let main_name = repo.branches(Some(BranchType::Local)).unwrap().flatten().filter_map(|(b, _)| b.name().ok().flatten().map(String::from)).find(|n| n != "feature").unwrap();
        checkout_sync(d.path(), &main_name, false).unwrap();
        assert_eq!(repo.head().unwrap().shorthand().ok(), Some(main_name.as_str()));
    }
    #[test]
    fn worktree_porcelain_parse() {
        let txt = "worktree /r/main\nHEAD abc\nbranch refs/heads/main\n\nworktree /r/.caret/worktrees/x\nHEAD def\nbranch refs/heads/agent/x\nlocked\n\nworktree /r/d\nHEAD 123\ndetached\n";
        let w = parse_worktrees(txt);
        assert_eq!(w.len(), 3); assert!(w[0].is_main); assert_eq!(w[1].branch.as_deref(), Some("agent/x")); assert!(w[1].is_locked); assert!(w[2].branch.is_none());
    }
}

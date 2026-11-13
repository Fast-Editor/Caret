//! Agent worktree policies (`settings.worktrees`): location and branch templates, per-thread status, and cleanup
//! (preview + removal). Decision helpers are pure and unit-tested; git probes go through `core::git::run_git`.
//!
//! Cleanup is per workspace root: the preview lists the threads of one root (or of every root), and the automatic
//! pass runs once per root per process, when the root's first window opens (`events::on_workspace_change`).
use crate::ai::agent::manager::{AgentManager, SessionHandle};
use crate::ai::agent::session::*;
use crate::core::git::run_git;
use crate::core::settings::WorktreeSettings;
use crate::error::{CaretError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const GIT_TIMEOUT: Duration = Duration::from_secs(30);
const DAY_MS: u64 = 86_400_000;

// ───────────────────────── naming ─────────────────────────

/// Directory slug for a thread's worktree: title words plus 4 id chars, or `agent-<id8>` for untitled threads.
pub fn worktree_slug(title: &str, id: &str) -> String {
    let slug = crate::ai::agent::manager::slugify(title);
    let short = |n: usize| id.chars().take(n).collect::<String>();
    if slug.is_empty() || slug == "new-chat" || slug == "scratch-thread" { format!("agent-{}", short(8)) } else { format!("{slug}-{}", short(4)) }
}

/// Keep a rendered template a valid branch name: only `[A-Za-z0-9/_.-]`, no `..`, `//`, `--`, no leading or
/// trailing separators, no `.lock` suffix.
pub fn sanitize_branch(s: &str) -> String {
    let mut out: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.') { c } else { '-' }).collect();
    while out.contains("..") { out = out.replace("..", "."); }
    while out.contains("//") { out = out.replace("//", "/"); }
    while out.contains("--") { out = out.replace("--", "-"); }
    while out.contains("/-") { out = out.replace("/-", "/"); }
    while out.contains("-/") { out = out.replace("-/", "/"); }
    let out = out.trim_matches(|c| c == '/' || c == '-' || c == '.').to_string();
    let out = out.trim_end_matches(".lock").to_string();
    if out.is_empty() { "caret/agent".into() } else { out }
}

/// Render `branchTemplate`: `{slug}`, `{model}` (slugified model id, `auto` when none), `{id}` (8 chars),
/// `{date}` (YYYY-MM-DD). The result is sanitized into a valid branch name.
pub fn render_branch_template(template: &str, slug: &str, model: Option<&str>, id: &str, date: &str) -> String {
    let model_slug = model.map(crate::ai::agent::manager::slugify).filter(|m| !m.is_empty()).unwrap_or_else(|| "auto".into());
    let short: String = id.chars().take(8).collect();
    let t = if template.trim().is_empty() { "caret/{slug}" } else { template.trim() };
    sanitize_branch(&t.replace("{slug}", slug).replace("{model}", &model_slug).replace("{id}", &short).replace("{date}", date))
}

pub fn today() -> String { chrono::Local::now().format("%Y-%m-%d").to_string() }

/// Where worktrees go: `~/...`, absolute, or relative to the workspace root (default `.caret/worktrees`).
pub fn resolve_location(root: &Path, location: &str) -> PathBuf {
    let loc = location.trim();
    if loc.is_empty() { return root.join(".caret").join("worktrees"); }
    let expanded = if let Some(rest) = loc.strip_prefix("~/") { crate::paths::home_dir().join(rest).to_string_lossy().to_string() } else if loc == "~" { crate::paths::home_dir().to_string_lossy().to_string() } else { loc.to_string() };
    crate::paths::resolve(Some(root), &expanded)
}

// ───────────────────────── status / cleanup types ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeStatus { pub path: String, pub branch: Option<String>, pub has_commits_beyond_base: bool, pub dirty: bool, pub merged_into_default: bool, pub last_activity: u64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCleanupCandidate { pub session_id: String, pub title: String, pub path: String, pub branch: Option<String>, pub reasons: Vec<String>, pub dirty: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupSkipped { pub session_id: String, pub reason: String }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCleanupReport { pub removed: Vec<String>, pub skipped: Vec<CleanupSkipped>, pub notes: Vec<String> }

/// Facts the cleanup policy evaluates for one agent worktree.
#[derive(Debug, Clone, Default)]
pub struct WorktreeFacts { pub updated_at: u64, pub archived: bool, pub merged_into_default: bool, pub has_commits_beyond_base: bool, pub dirty: bool, pub running: bool, pub missing: bool }

pub fn any_policy_on(cfg: &WorktreeSettings) -> bool { cfg.cleanup_inactive_days > 0 || cfg.cleanup_settled || cfg.cleanup_merged || cfg.cleanup_no_commits }

/// Why a worktree qualifies for cleanup (empty = keep). Running threads never qualify; a missing folder always does.
pub fn cleanup_reasons(cfg: &WorktreeSettings, f: &WorktreeFacts, now_ms: u64) -> Vec<String> {
    let mut r = vec![];
    if f.running { return r; }
    if f.missing { r.push("worktree folder is missing".into()); return r; }
    if cfg.cleanup_inactive_days > 0 {
        let idle_days = now_ms.saturating_sub(f.updated_at) / DAY_MS;
        if idle_days >= cfg.cleanup_inactive_days as u64 { r.push(format!("inactive for {idle_days} days")); }
    }
    if cfg.cleanup_settled && f.archived { r.push("thread is archived".into()); }
    if cfg.cleanup_merged && f.merged_into_default { r.push("branch is merged into the default branch".into()); }
    if cfg.cleanup_no_commits && !f.has_commits_beyond_base && !f.merged_into_default { r.push("no commits beyond the base".into()); }
    r
}

#[derive(Debug, Clone, PartialEq)]
pub enum CleanupAction { Skip(String), Remove { stash: bool, force: bool } }

/// What to do with a worktree given `onLocalChanges` and whether it has uncommitted changes.
pub fn cleanup_action(cfg: &WorktreeSettings, dirty: bool) -> CleanupAction {
    if !dirty { return CleanupAction::Remove { stash: false, force: false }; }
    match cfg.on_local_changes.as_str() {
        "stash" => CleanupAction::Remove { stash: true, force: true },
        "discard" => CleanupAction::Remove { stash: false, force: true },
        _ => CleanupAction::Skip("worktree has local changes (onLocalChanges is keep)".into()),
    }
}

/// Delete the branch only when the setting is on and the count of unmerged commits is known to be zero.
pub fn should_delete_branch(cfg: &WorktreeSettings, unmerged_commits: Option<u32>) -> bool { cfg.delete_branch_with_worktree && unmerged_commits == Some(0) }

// ───────────────────────── git probes ─────────────────────────

#[derive(Debug, Clone, Default)]
pub struct GitProbe { pub head: Option<String>, pub dirty: bool, pub has_commits_beyond_base: bool, pub merged_into_default: bool, /// Commits on the worktree branch that are not on the default branch (None = could not determine).
    pub unmerged_commits: Option<u32> }

async fn git_out(dir: &Path, args: &[&str]) -> Option<String> { let r = run_git(dir, args, GIT_TIMEOUT).await.ok()?; if r.ok { Some(r.output.trim().to_string()) } else { None } }
async fn git_ok(dir: &Path, args: &[&str]) -> bool { run_git(dir, args, GIT_TIMEOUT).await.map(|r| r.ok).unwrap_or(false) }

/// `origin/HEAD` target, else the first of main/master/develop/trunk that exists locally.
pub async fn default_branch(root: &Path) -> Option<String> {
    if let Some(s) = git_out(root, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]).await { if let Some(b) = s.strip_prefix("origin/") { return Some(b.to_string()); } }
    for c in ["main", "master", "develop", "trunk"] { if git_ok(root, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{c}")]).await { return Some(c.into()); } }
    None
}

/// Dirty state, commits beyond the recorded base (fallback: beyond the default branch) and merged-ness.
pub async fn probe(root: &Path, wt: &Path, base: Option<&str>) -> GitProbe {
    let mut p = GitProbe::default();
    if !wt.exists() { return p; }
    p.dirty = git_out(wt, &["status", "--porcelain", "--untracked-files=normal"]).await.map(|o| !o.is_empty()).unwrap_or(false);
    p.head = git_out(wt, &["rev-parse", "HEAD"]).await;
    let Some(head) = p.head.clone() else { return p };
    let default = default_branch(root).await;
    let upstream = default.clone().map(|d| format!("refs/heads/{d}")).unwrap_or_else(|| "HEAD".into());
    p.unmerged_commits = git_out(root, &["rev-list", "--count", &format!("{upstream}..{head}")]).await.and_then(|s| s.parse().ok());
    p.has_commits_beyond_base = match base { Some(b) => b != head, None => p.unmerged_commits.unwrap_or(0) > 0 };
    p.merged_into_default = match &default {
        Some(d) => p.unmerged_commits == Some(0) && p.has_commits_beyond_base && git_ok(root, &["merge-base", "--is-ancestor", &head, &format!("refs/heads/{d}")]).await,
        None => false,
    };
    p
}

// ───────────────────────── per-thread status ─────────────────────────

pub async fn status_for_session(mgr: &Arc<AgentManager>, session_id: &str) -> Result<WorktreeStatus> {
    let h = mgr.handle(session_id)?;
    let (wt, branch, root, base, updated) = { let s = h.session.lock(); (s.worktree_path.clone(), s.branch.clone(), s.root(), s.worktree_base.clone(), s.updated_at) };
    let wt = wt.ok_or_else(|| CaretError::invalid("This thread does not use a worktree"))?;
    let root = root.ok_or_else(CaretError::no_workspace)?;
    let p = probe(&root, Path::new(&wt), base.as_deref()).await;
    Ok(WorktreeStatus { path: wt, branch, has_commits_beyond_base: p.has_commits_beyond_base, dirty: p.dirty, merged_into_default: p.merged_into_default, last_activity: updated })
}

// ───────────────────────── cleanup ─────────────────────────

/// Summaries of the threads that belong to `root` (every workspace's when `None`), archived included.
fn sessions_for_root(mgr: &Arc<AgentManager>, root: Option<&Path>) -> Vec<SessionSummary> {
    let all = mgr.list_sessions_all(true);
    match root { Some(r) => all.into_iter().filter(|s| Path::new(&s.workspace_root) == r).collect(), None => all }
}

/// The handle of a thread that lives under `root` (its own `workspace_root`): the loaded one when the manager has
/// it, otherwise read from that root's store without going through the focused window's workspace.
fn handle_for(mgr: &Arc<AgentManager>, root: Option<&Path>, id: &str) -> Result<Arc<SessionHandle>> {
    if let Some(h) = mgr.get_loaded(id) { return Ok(h); }
    let key = crate::core::sessions::workspace_key(root);
    mgr.store.load(&key, id).or_else(|_| mgr.store.load("global", id)).map(SessionHandle::new)
}

/// Threads of workspace `root` (every workspace when `None`) whose worktree qualifies under the configured policies.
pub async fn cleanup_preview(mgr: &Arc<AgentManager>, root: Option<&Path>) -> Result<Vec<WorktreeCleanupCandidate>> {
    let cfg = mgr.settings().worktrees.clone();
    if !any_policy_on(&cfg) { return Ok(vec![]); }
    let now = now_ms();
    let mut out = vec![];
    for sum in sessions_for_root(mgr, root) {
        if sum.worktree_path.is_none() || sum.is_subagent == Some(true) { continue; }
        let Ok(h) = handle_for(mgr, Some(Path::new(&sum.workspace_root)), &sum.id) else { continue };
        let (wt, branch, root, base, title, archived, updated) = { let s = h.session.lock(); (s.worktree_path.clone().unwrap_or_default(), s.branch.clone(), s.root(), s.worktree_base.clone(), s.title.clone(), s.archived, s.updated_at) };
        let Some(root) = root else { continue };
        let wt_path = PathBuf::from(&wt);
        let p = probe(&root, &wt_path, base.as_deref()).await;
        let facts = WorktreeFacts { updated_at: updated, archived, merged_into_default: p.merged_into_default, has_commits_beyond_base: p.has_commits_beyond_base, dirty: p.dirty, running: h.is_running(), missing: !wt_path.exists() };
        let reasons = cleanup_reasons(&cfg, &facts, now);
        if reasons.is_empty() { continue; }
        out.push(WorktreeCleanupCandidate { session_id: sum.id.clone(), title, path: wt, branch, reasons, dirty: p.dirty });
    }
    Ok(out)
}

/// Remove the worktrees of the given threads, honouring `onLocalChanges` and `deleteBranchWithWorktree`.
/// The threads themselves are kept; their cwd falls back to the workspace root.
pub async fn cleanup(mgr: &Arc<AgentManager>, session_ids: Vec<String>) -> Result<WorktreeCleanupReport> {
    let cfg = mgr.settings().worktrees.clone();
    let mut rep = WorktreeCleanupReport::default();
    // Threads may belong to any workspace (the auto pass runs per root): find each one's root before loading it.
    let roots: std::collections::HashMap<String, String> = mgr.list_sessions_all(true).into_iter().map(|s| (s.id, s.workspace_root)).collect();
    for id in session_ids {
        let root = roots.get(&id).filter(|r| !r.is_empty()).map(|r| PathBuf::from(r));
        let h = match handle_for(mgr, root.as_deref(), &id) { Ok(h) => h, Err(_) => { rep.skipped.push(CleanupSkipped { session_id: id, reason: "thread not found".into() }); continue; } };
        if h.is_running() { rep.skipped.push(CleanupSkipped { session_id: id, reason: "thread is running".into() }); continue; }
        let (wt, branch, root, base) = { let s = h.session.lock(); (s.worktree_path.clone(), s.branch.clone(), s.root(), s.worktree_base.clone()) };
        let Some(wt) = wt else { rep.skipped.push(CleanupSkipped { session_id: id, reason: "thread has no worktree".into() }); continue };
        let Some(root) = root else { rep.skipped.push(CleanupSkipped { session_id: id, reason: "thread has no workspace root".into() }); continue };
        let wt_path = PathBuf::from(&wt);
        let p = probe(&root, &wt_path, base.as_deref()).await;
        if wt_path.exists() {
            match cleanup_action(&cfg, p.dirty) {
                CleanupAction::Skip(reason) => { rep.skipped.push(CleanupSkipped { session_id: id, reason }); continue; }
                CleanupAction::Remove { stash, force } => {
                    if stash {
                        let msg = format!("caret worktree cleanup {id}");
                        let r = run_git(&wt_path, &["stash", "push", "--include-untracked", "-m", &msg], GIT_TIMEOUT).await?;
                        if !r.ok { rep.skipped.push(CleanupSkipped { session_id: id, reason: format!("stash failed: {}", r.output.trim()) }); continue; }
                        rep.notes.push(format!("{}: local changes stashed in the repository", branch.clone().unwrap_or_else(|| wt.clone())));
                    }
                    let mut args = vec!["worktree", "remove"];
                    if force { args.push("--force"); }
                    args.push(&wt);
                    let r = run_git(&root, &args, Duration::from_secs(60)).await?;
                    if !r.ok { rep.skipped.push(CleanupSkipped { session_id: id, reason: format!("worktree remove failed: {}", r.output.trim()) }); continue; }
                }
            }
        }
        let _ = run_git(&root, &["worktree", "prune"], GIT_TIMEOUT).await;
        if let Some(b) = &branch {
            if should_delete_branch(&cfg, p.unmerged_commits) {
                // Merged-ness was checked against the default branch above, so a forced delete loses nothing.
                let r = run_git(&root, &["branch", "-D", b], GIT_TIMEOUT).await?;
                if !r.ok { rep.notes.push(format!("branch {b} kept: {}", r.output.trim().lines().last().unwrap_or("delete failed"))); }
            } else if cfg.delete_branch_with_worktree {
                rep.notes.push(match p.unmerged_commits { Some(n) => format!("branch {b} kept: {n} unmerged commit(s)"), None => format!("branch {b} kept: could not verify it is merged") });
            }
        }
        { let mut s = h.session.lock(); s.worktree_path = None; s.branch = None; s.worktree_base = None; s.cwd = s.workspace_root.clone(); s.touch(); }
        mgr.persist_now(&h);
        mgr.emit(AgentEvent::SessionReset { session_id: id.clone() });
        rep.removed.push(id);
    }
    Ok(rep)
}

// ───────────────────────── auto cleanup on start ─────────────────────────

static AUTO_DONE: std::sync::OnceLock<parking_lot::Mutex<std::collections::HashSet<PathBuf>>> = std::sync::OnceLock::new();

/// Register the start-up / workspace-open hook. Runs when `worktrees.autoCleanup` is on and a policy applies, once
/// per workspace root per process (at start-up for every open root, then for each root as its first window opens),
/// and reports through an in-app notification.
pub fn init(app: &AppHandle) {
    let h = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(12)).await;
        for root in h.state::<crate::core::windows::WindowRegistry>().roots() { run_auto_cleanup(&h, &root).await; }
    });
    crate::events::on_workspace_change(app, |app, change| {
        if !change.first_for_root { return; }
        let Some(info) = &change.info else { return };
        let (h, root) = (app.clone(), PathBuf::from(&info.root));
        tauri::async_runtime::spawn(async move { tokio::time::sleep(Duration::from_secs(4)).await; run_auto_cleanup(&h, &root).await; });
    });
}

/// The automatic cleanup pass for one workspace root.
pub async fn run_auto_cleanup(app: &AppHandle, root: &Path) {
    let Some(mgr) = app.try_state::<Arc<AgentManager>>().map(|s| s.inner().clone()) else { return };
    let settings = mgr.settings();
    if !settings.worktrees.auto_cleanup || !any_policy_on(&settings.worktrees) { return; }
    if !AUTO_DONE.get_or_init(Default::default).lock().insert(root.to_path_buf()) { return; }
    let preview = match cleanup_preview(&mgr, Some(root)).await { Ok(p) => p, Err(e) => { tracing::warn!(error=%e, root = %root.display(), "worktree cleanup preview failed"); return; } };
    if preview.is_empty() { return; }
    let ids: Vec<String> = preview.iter().map(|c| c.session_id.clone()).collect();
    match cleanup(&mgr, ids).await {
        Ok(rep) => {
            let mut lines: Vec<String> = preview.iter().filter(|c| rep.removed.contains(&c.session_id)).map(|c| format!("{} ({})", c.title, c.reasons.join(", "))).collect();
            for s in &rep.skipped { let title = preview.iter().find(|c| c.session_id == s.session_id).map(|c| c.title.clone()).unwrap_or_else(|| s.session_id.clone()); lines.push(format!("skipped {title}: {}", s.reason)); }
            lines.extend(rep.notes.iter().cloned());
            crate::events::notify(app, "info", format!("Cleaned up {} agent worktree(s)", rep.removed.len()), Some(lines.join("\n")));
        }
        Err(e) => crate::events::notify(app, "error", "Worktree cleanup failed", Some(e.message.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cfg() -> WorktreeSettings { WorktreeSettings::default() }

    #[test]
    fn branch_template_rendering() {
        assert_eq!(render_branch_template("caret/{slug}", "fix-login-ab12", None, "abcdef1234567890", "2026-11-07"), "caret/fix-login-ab12");
        assert_eq!(render_branch_template("agents/{model}/{slug}", "fix-login-ab12", Some("claude-opus-5-5"), "abcdef12", "2026-11-07"), "agents/claude-opus-5-5/fix-login-ab12");
        assert_eq!(render_branch_template("{date}/{id}", "x", None, "abcdef1234567890", "2026-11-07"), "2026-11-07/abcdef12");
        assert_eq!(render_branch_template("", "slug-1", None, "id", "d"), "caret/slug-1", "empty template falls back to the default");
        assert_eq!(render_branch_template("{model}", "s", None, "id", "d"), "auto", "no model renders as auto");
        // invalid characters, doubled separators and trailing junk are sanitized
        assert_eq!(sanitize_branch("caret//my branch..name?*~^:[\\"), "caret/my-branch.name");
        assert_eq!(sanitize_branch("/-leading--and--trailing-/"), "leading-and-trailing");
        assert_eq!(sanitize_branch("x.lock"), "x");
        assert_eq!(sanitize_branch("???"), "caret/agent");
    }

    #[test]
    fn worktree_slugs_and_locations() {
        assert_eq!(worktree_slug("Fix the login bug", "abcdef1234"), "fix-the-login-bug-abcd");
        assert_eq!(worktree_slug("New chat", "abcdef1234"), "agent-abcdef12");
        assert_eq!(worktree_slug("", "abcdef1234"), "agent-abcdef12");
        let root = Path::new("/work/repo");
        assert_eq!(resolve_location(root, ""), PathBuf::from("/work/repo/.caret/worktrees"));
        assert_eq!(resolve_location(root, ".caret/worktrees"), PathBuf::from("/work/repo/.caret/worktrees"));
        assert_eq!(resolve_location(root, "../wt"), PathBuf::from("/work/wt"));
        assert_eq!(resolve_location(root, "/tmp/agent-wts"), PathBuf::from("/tmp/agent-wts"));
        assert_eq!(resolve_location(root, "~/caret-worktrees"), crate::paths::home_dir().join("caret-worktrees"));
    }

    #[test]
    fn cleanup_reasons_follow_policies() {
        let now = 100 * DAY_MS;
        let fresh = WorktreeFacts { updated_at: now - DAY_MS, has_commits_beyond_base: true, ..Default::default() };
        assert!(cleanup_reasons(&cfg(), &fresh, now).is_empty(), "active thread with commits is kept");
        let idle = WorktreeFacts { updated_at: now - 20 * DAY_MS, has_commits_beyond_base: true, ..Default::default() };
        assert_eq!(cleanup_reasons(&cfg(), &idle, now), vec!["inactive for 20 days".to_string()]);
        let mut off = cfg(); off.cleanup_inactive_days = 0;
        assert!(cleanup_reasons(&off, &idle, now).is_empty(), "0 disables the inactivity policy");
        let archived = WorktreeFacts { updated_at: now, archived: true, has_commits_beyond_base: true, ..Default::default() };
        assert_eq!(cleanup_reasons(&cfg(), &archived, now), vec!["thread is archived".to_string()]);
        let mut no_settled = cfg(); no_settled.cleanup_settled = false;
        assert!(cleanup_reasons(&no_settled, &archived, now).is_empty());
        let merged = WorktreeFacts { updated_at: now, merged_into_default: true, has_commits_beyond_base: true, ..Default::default() };
        assert_eq!(cleanup_reasons(&cfg(), &merged, now), vec!["branch is merged into the default branch".to_string()]);
        let empty = WorktreeFacts { updated_at: now, ..Default::default() };
        assert_eq!(cleanup_reasons(&cfg(), &empty, now), vec!["no commits beyond the base".to_string()]);
        let mut no_empty = cfg(); no_empty.cleanup_no_commits = false;
        assert!(cleanup_reasons(&no_empty, &empty, now).is_empty());
        // running threads never qualify; a missing folder always does
        let running = WorktreeFacts { updated_at: 0, archived: true, running: true, ..Default::default() };
        assert!(cleanup_reasons(&cfg(), &running, now).is_empty());
        let missing = WorktreeFacts { updated_at: now, has_commits_beyond_base: true, missing: true, ..Default::default() };
        assert_eq!(cleanup_reasons(&cfg(), &missing, now), vec!["worktree folder is missing".to_string()]);
        // several reasons stack
        let both = WorktreeFacts { updated_at: now - 30 * DAY_MS, archived: true, ..Default::default() };
        assert_eq!(cleanup_reasons(&cfg(), &both, now).len(), 3);
        let mut nothing = cfg(); nothing.cleanup_inactive_days = 0; nothing.cleanup_settled = false; nothing.cleanup_merged = false; nothing.cleanup_no_commits = false;
        assert!(!any_policy_on(&nothing)); assert!(any_policy_on(&cfg()));
    }

    #[test]
    fn cleanup_actions_and_branch_deletion() {
        assert_eq!(cleanup_action(&cfg(), false), CleanupAction::Remove { stash: false, force: false });
        assert!(matches!(cleanup_action(&cfg(), true), CleanupAction::Skip(_)), "keep skips dirty worktrees");
        let mut c = cfg(); c.on_local_changes = "stash".into();
        assert_eq!(cleanup_action(&c, true), CleanupAction::Remove { stash: true, force: true });
        c.on_local_changes = "discard".into();
        assert_eq!(cleanup_action(&c, true), CleanupAction::Remove { stash: false, force: true });
        assert!(should_delete_branch(&cfg(), Some(0)));
        assert!(!should_delete_branch(&cfg(), Some(2)));
        assert!(!should_delete_branch(&cfg(), None), "unknown merged-ness keeps the branch");
        let mut keep = cfg(); keep.delete_branch_with_worktree = false;
        assert!(!should_delete_branch(&keep, Some(0)));
    }
}

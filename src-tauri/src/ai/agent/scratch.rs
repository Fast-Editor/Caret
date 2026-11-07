//! Scratch threads: a chat with its own throwaway git repository under `~/.caret/scratch/<id>` (no folder needs to
//! be open), persisted under the "global" session key. `promote_scratch` moves the folder to a real project
//! directory and re-homes the session file and checkpoints under that workspace's key.
use crate::ai::agent::checkpoints;
use crate::ai::agent::manager::{AgentManager, SessionHandle};
use crate::ai::agent::session::*;
use crate::core::git::run_git;
use crate::core::sessions::workspace_key;
use crate::error::{CaretError, Result};
use crate::paths;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub fn scratch_root() -> PathBuf { paths::sub_dir("scratch") }
pub fn scratch_dir(id: &str) -> PathBuf { scratch_root().join(id) }
pub fn git_available() -> bool { which::which("git").is_ok() }

async fn git(dir: &Path, args: &[&str]) -> Result<()> {
    let r = run_git(dir, args, Duration::from_secs(30)).await?;
    if r.ok { Ok(()) } else { Err(CaretError::new("git", format!("{} failed: {}", r.command, r.output.trim()))) }
}

/// `git init` plus an initial empty commit so the thread starts on a real branch (checkpoints, diffs and worktrees work).
pub async fn init_repo(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    if !dir.join(".git").exists() {
        git(dir, &["init", "-q"]).await?;
        let _ = git(dir, &["symbolic-ref", "HEAD", "refs/heads/main"]).await;
    }
    let has_head = run_git(dir, &["rev-parse", "--verify", "--quiet", "HEAD"], Duration::from_secs(10)).await.map(|r| r.ok).unwrap_or(false);
    if !has_head { git(dir, &["-c", "user.name=Caret", "-c", "user.email=caret@localhost", "commit", "--allow-empty", "-q", "-m", "Scratch thread"]).await?; }
    Ok(())
}

/// Create a scratch thread: fresh repo in `~/.caret/scratch/<id>`, session rooted there, flagged `scratch`.
pub async fn create_scratch_session(mgr: &Arc<AgentManager>, mode: Option<String>, model_id: Option<String>, title: Option<String>, agent_id: Option<String>) -> Result<Arc<SessionHandle>> {
    if !git_available() { return Err(CaretError::config("git is required for scratch threads (not found on PATH)")); }
    let settings = mgr.settings();
    let mut s = Session::default();
    let dir = scratch_dir(&s.id);
    init_repo(&dir).await?;
    s.mode = mode.filter(|m| !m.is_empty()).unwrap_or_else(|| settings.agent.default_mode.clone());
    s.model_id = model_id.filter(|m| !m.is_empty() && m != "auto");
    match title { Some(t) if !t.trim().is_empty() => { s.title = t; s.title_generated = true; } _ => { s.title = "Scratch thread".into(); } }
    s.workspace_root = dir.to_string_lossy().to_string();
    s.cwd = s.workspace_root.clone();
    s.scratch = true;
    s.agent_id = agent_id.filter(|a| !a.is_empty() && a != "caret");
    let h = SessionHandle::new(s);
    mgr.adopt(h.clone());
    Ok(h)
}

fn dir_is_empty(p: &Path) -> bool { std::fs::read_dir(p).map(|mut d| d.next().is_none()).unwrap_or(false) }

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?; let to = dst.join(e.file_name()); let ty = e.file_type()?;
        if ty.is_dir() { copy_dir_all(&e.path(), &to)?; }
        else if ty.is_symlink() {
            #[cfg(unix)] { std::os::unix::fs::symlink(std::fs::read_link(e.path())?, &to)?; }
            #[cfg(not(unix))] { std::fs::copy(e.path(), &to)?; }
        } else { std::fs::copy(e.path(), &to)?; }
    }
    Ok(())
}

/// Rename, falling back to copy + delete across devices.
fn move_dir(src: &Path, dst: &Path) -> Result<()> {
    if std::fs::rename(src, dst).is_ok() { return Ok(()); }
    copy_dir_all(src, dst)?;
    std::fs::remove_dir_all(src)?;
    Ok(())
}

/// `~/x`, absolute, or relative to the home directory.
pub fn resolve_target(target: &str) -> PathBuf {
    let t = target.trim();
    let expanded = if let Some(rest) = t.strip_prefix("~/") { paths::home_dir().join(rest).to_string_lossy().to_string() } else { t.to_string() };
    paths::resolve(Some(&paths::home_dir()), &expanded)
}

/// Move the scratch folder to `target_dir` (must not exist or be empty) and re-home the session under that workspace.
pub async fn promote_scratch(mgr: &Arc<AgentManager>, session_id: &str, target_dir: &str) -> Result<Arc<SessionHandle>> {
    let h = mgr.handle(session_id)?;
    if h.is_running() { return Err(CaretError::invalid("Stop the agent before making this thread a project")); }
    let (src, scratch) = { let s = h.session.lock(); (PathBuf::from(&s.workspace_root), s.scratch) };
    if !scratch { return Err(CaretError::invalid("This thread is not a scratch thread")); }
    if target_dir.trim().is_empty() { return Err(CaretError::invalid("Choose a target folder")); }
    let target = resolve_target(target_dir);
    if target == src { return Err(CaretError::invalid("The target is the scratch folder itself")); }
    if target.exists() {
        if !target.is_dir() { return Err(CaretError::invalid("The target exists and is not a folder")); }
        if !dir_is_empty(&target) { return Err(CaretError::invalid(format!("The target folder is not empty: {}", target.display()))); }
        std::fs::remove_dir(&target)?;
    }
    if let Some(parent) = target.parent() { std::fs::create_dir_all(parent)?; }
    if src.exists() { move_dir(&src, &target)?; } else { std::fs::create_dir_all(&target)?; init_repo(&target).await?; }
    let old_key = { let s = h.session.lock(); mgr.ws_key_of(&s) };
    { let mut s = h.session.lock(); s.workspace_root = target.to_string_lossy().to_string(); s.cwd = s.workspace_root.clone(); s.scratch = false; s.touch(); }
    let new_key = workspace_key(Some(&target));
    mgr.store.delete(&old_key, session_id)?;
    mgr.persist_now(&h);
    checkpoints::move_session(&old_key, &new_key, session_id)?;
    mgr.emit(AgentEvent::SessionReset { session_id: session_id.to_string() });
    Ok(h)
}

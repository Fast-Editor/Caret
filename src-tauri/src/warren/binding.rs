//! Per-thread sandbox binding (docs/WARREN.md): bind / unbind a thread to a Warren instance, manual push/pull, the
//! executor the runner uses for a turn, and turning pulled files into pending changes.
use super::adapter::{self, SandboxBinding};
use super::executor::{CommandExecutor, LocalExecutor, RemoteOps, RemoteSync, WarrenExecutor};
use crate::ai::agent::manager::{AgentManager, SessionHandle};
use crate::ai::agent::modes;
use crate::ai::agent::session::{short_id, AgentEvent, ChangeKind, ChatSessionView, FileChangeSummary, Session};
use crate::ai::agent::tools::{fs_tools, ToolContext};
use crate::core::settings::Settings;
use crate::error::{CaretError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;

type Mgr<'a> = State<'a, Arc<AgentManager>>;

/// Result of `warren_sync` (TS `SyncReport`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport { pub direction: String, pub files: u64, pub bytes: u64, pub deleted: u64, pub ms: u64 }

/// Event part A emits when the instance list changed; a deleted instance (unbind) counts.
const WARREN_CHANGED: &str = "caret://warren/changed";

// ───────────────────────── paths / names ─────────────────────────

/// The folder the sandbox mirrors: the thread's worktree when it has one, else the workspace, else its cwd.
pub fn local_root_of(s: &Session) -> PathBuf {
    if s.worktree_path.is_some() { return s.cwd_path(); }
    s.root().unwrap_or_else(|| s.cwd_path())
}
fn folder_name(local_root: &Path) -> String {
    let base = local_root.file_name().map(|n| n.to_string_lossy().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "workspace".into());
    let slug = crate::ai::agent::manager::slugify(&base);
    if slug.is_empty() { "workspace".into() } else { slug }
}
/// Name for a thread's new instance: `<folder>-<short id>`.
pub fn instance_name_for(local_root: &Path, session_id: &str) -> String { format!("{}-{}", folder_name(local_root), session_id.chars().filter(|c| c.is_ascii_alphanumeric()).take(6).collect::<String>().to_lowercase()) }
/// Synced copy of `local_root` inside an instance: `<warren.remoteRoot>/<folder basename>`.
pub fn remote_root_for(settings: &Settings, local_root: &Path) -> String {
    let base = local_root.file_name().map(|n| n.to_string_lossy().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "workspace".into());
    format!("{}/{}", settings.warren.remote_root.trim_end_matches('/'), base)
}

// ───────────────────────── binding state ─────────────────────────

/// Mutate the thread's binding (loaded sessions only), persist and announce it.
pub fn update_binding(mgr: &AgentManager, session_id: &str, f: impl FnOnce(&mut SandboxBinding)) {
    let Some(h) = mgr.get_loaded(session_id) else { return };
    let sv = { let mut s = h.session.lock(); let Some(b) = s.sandbox.as_mut() else { return }; f(b); s.touch(); s.summary_view() };
    mgr.persist(&h);
    mgr.emit(AgentEvent::SessionUpdated { session: sv });
}
pub fn set_binding_status(mgr: &AgentManager, session_id: &str, status: &str, error: Option<String>) { update_binding(mgr, session_id, |b| adapter::set_status(b, status, error)); }
/// After a sandboxed command: stamp the push/pull and the outcome on the binding.
pub fn record_run(mgr: &AgentManager, session_id: &str, pushed_files: u64, error: Option<String>) {
    update_binding(mgr, session_id, |b| {
        adapter::record_push(b, pushed_files);
        adapter::record_pull(b);
        match error { Some(e) => adapter::set_status(b, "error", Some(e)), None => adapter::set_status(b, "ready", None) }
    });
}

/// Bind the thread to `instance_id`, or to a new instance named after its folder. Waits for the instance to run.
pub async fn bind(mgr: &AgentManager, h: &SessionHandle, settings: &Settings, instance_id: Option<String>) -> Result<SandboxBinding> {
    if !settings.warren.enabled { return Err(CaretError::config("Warren sandboxes are disabled (Settings → Sandboxes → Enable)")); }
    let (sid, local_root) = { let s = h.session.lock(); (s.id.clone(), local_root_of(&s)) };
    let client = adapter::connect(mgr)?;
    let (id, name) = match instance_id.filter(|i| !i.trim().is_empty()) {
        Some(id) => adapter::instance_ident(&adapter::instance(&client, id.trim()).await?),
        None => adapter::instance_ident(&adapter::create_instance(mgr, &instance_name_for(&local_root, &sid)).await?),
    };
    let running = adapter::wait_running(&client, &id, Duration::from_secs(180)).await?;
    let (id, name) = { let (rid, rname) = adapter::instance_ident(&running); (if rid.is_empty() { id } else { rid }, if rname.is_empty() { name } else { rname }) };
    adapter::forget_manifest(&id);
    let binding = adapter::new_binding(&id, &name, remote_root_for(settings, &local_root));
    let sv = { let mut s = h.session.lock(); s.sandbox = Some(binding.clone()); s.touch(); s.summary_view() };
    mgr.persist_now(h);
    mgr.emit(AgentEvent::SessionUpdated { session: sv });
    Ok(binding)
}

/// Clear the thread's binding (the instance stays). Returns the previous binding.
pub fn unbind(mgr: &AgentManager, h: &SessionHandle) -> Option<SandboxBinding> {
    let (prev, sv) = { let mut s = h.session.lock(); let p = s.sandbox.take(); s.touch(); (p, s.summary_view()) };
    mgr.persist_now(h);
    mgr.emit(AgentEvent::SessionUpdated { session: sv });
    prev
}

// ───────────────────────── executor for a turn ─────────────────────────

/// The executor for one turn: the thread's Warren binding (subagents inherit their parent's), else the local shell.
/// `first_turn` + `warren.defaultForNewThreads` binds a fresh instance first; any failure falls back to local with a
/// notification so the turn still runs.
pub async fn executor_for_turn(mgr: &Arc<AgentManager>, h: &SessionHandle, settings: &Settings, root: Option<&Path>, cwd: &Path, first_turn: bool) -> Arc<dyn CommandExecutor> {
    let local = || -> Arc<dyn CommandExecutor> { Arc::new(LocalExecutor::from_settings(settings, root, cwd)) };
    let (binding, is_subagent, parent, local_root) = { let s = h.session.lock(); (s.sandbox.clone(), s.is_subagent, s.parent_session_id.clone(), local_root_of(&s)) };
    let binding = binding.or_else(|| if is_subagent { parent.and_then(|p| mgr.get_loaded(&p)).and_then(|ph| ph.session.lock().sandbox.clone()) } else { None });
    let binding = match binding {
        Some(b) => Some(b),
        None if first_turn && !is_subagent && settings.warren.enabled && settings.warren.default_for_new_threads && mgr.app_opt().is_some() => match bind(mgr, h, settings, None).await {
            Ok(b) => Some(b),
            Err(e) => { tracing::warn!(error = %e, "warren: auto-bind failed; running locally"); warn(mgr, "Warren sandbox unavailable", format!("{} — this thread's commands run locally.", e.message)); None }
        },
        None => None,
    };
    match binding {
        Some(b) if b.provider == "warren" => match adapter::connect(mgr) {
            Ok(client) => Arc::new(WarrenExecutor::new(client, b, local_root, settings.warren.sync_excludes.clone())),
            Err(e) => { warn(mgr, "Warren sandbox unavailable", format!("{} — this turn's commands run locally.", e.message)); local() }
        },
        _ => local(),
    }
}
fn warn(mgr: &AgentManager, title: &str, message: String) { if let Some(app) = mgr.app_opt() { crate::events::notify(&app, "warn", title.to_string(), Some(message)); } }

// ───────────────────────── pulled files → pending changes ─────────────────────────

/// What `apply_pulled` did with a pull.
#[derive(Debug, Default)]
pub struct PullApplied {
    /// Text files staged as pending changes (create / modify / delete).
    pub changes: Vec<FileChangeSummary>,
    /// Binary files written directly (no diff).
    pub binaries: Vec<String>,
    /// Pulled files identical to the local copy.
    pub unchanged: usize,
    /// Paths refused (absolute, `..`, `.git`).
    pub skipped: Vec<String>,
}

fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let p = Path::new(rel);
    if rel.is_empty() || p.is_absolute() || p.components().any(|c| !matches!(c, Component::Normal(_))) { return None; }
    if rel == ".git" || rel.starts_with(".git/") { return None; }
    Some(root.join(p))
}

/// Turn what a remote run moved into pending changes: text files go through `fs_tools::apply_change` (before = the
/// local content, after = the pulled bytes), binary files are written directly and listed, deletions become
/// `ChangeKind::Delete`. Paths outside `local_root` and `.git` are refused.
pub async fn apply_pulled(ctx: &ToolContext, sync: RemoteSync) -> Result<PullApplied> {
    let root = &sync.local_root;
    let mut out = PullApplied::default();
    for (rel, bytes) in sync.changed {
        let Some(abs) = safe_join(root, &rel) else { out.skipped.push(rel); continue };
        if abs.is_dir() { out.skipped.push(rel); continue; }
        let is_text = !content_inspector::inspect(&bytes[..bytes.len().min(8192)]).is_binary();
        match (is_text, String::from_utf8(bytes)) {
            (true, Ok(after)) => {
                let exists = abs.exists();
                let before = if exists { fs_tools::read_text(&abs).ok() } else { None };
                if before.as_deref() == Some(after.as_str()) { out.unchanged += 1; continue; }
                let kind = if exists { ChangeKind::Modify } else { ChangeKind::Create };
                out.changes.push(fs_tools::apply_change(ctx, &abs, kind, before, Some(after), None).await?);
            }
            (_, bytes) => {
                let bytes = match bytes { Ok(s) => s.into_bytes(), Err(e) => e.into_bytes() };
                if std::fs::read(&abs).ok().as_deref() == Some(bytes.as_slice()) { out.unchanged += 1; continue; }
                if let Some(p) = abs.parent() { std::fs::create_dir_all(p)?; }
                std::fs::write(&abs, &bytes)?;
                out.binaries.push(rel);
            }
        }
    }
    for rel in sync.deleted {
        let Some(abs) = safe_join(root, &rel) else { out.skipped.push(rel); continue };
        if !abs.is_file() { continue; }
        let before = fs_tools::read_text(&abs).ok();
        out.changes.push(fs_tools::apply_change(ctx, &abs, ChangeKind::Delete, before, None, None).await?);
    }
    Ok(out)
}

/// One-line summary of a pull for tool output / notifications.
pub fn describe_pull(a: &PullApplied) -> Option<String> {
    if a.changes.is_empty() && a.binaries.is_empty() && a.skipped.is_empty() { return None; }
    let mut parts = vec![];
    if !a.changes.is_empty() { parts.push(format!("{} file(s) changed by the command were pulled back as pending changes: {}", a.changes.len(), a.changes.iter().map(|c| format!("{} ({})", c.path, format!("{:?}", c.kind).to_lowercase())).collect::<Vec<_>>().join(", "))); }
    if !a.binaries.is_empty() { parts.push(format!("{} binary file(s) written directly (no diff): {}", a.binaries.len(), a.binaries.join(", "))); }
    if !a.skipped.is_empty() { parts.push(format!("{} path(s) refused (outside the workspace or .git): {}", a.skipped.len(), a.skipped.join(", "))); }
    Some(parts.join("\n"))
}

// ───────────────────────── commands ─────────────────────────

/// Bind a thread to a Warren instance (`None` creates `<folder>-<short id>`); waits for `running`.
#[tauri::command]
pub async fn warren_bind_thread(_app: AppHandle, mgr: Mgr<'_>, session_id: String, instance_id: Option<String>) -> Result<SandboxBinding> {
    let h = mgr.handle(&session_id)?;
    let settings = { let s = h.session.lock(); mgr.settings_for(s.root().as_deref()) };
    bind(&mgr, &h, &settings, instance_id).await
}

/// Clear a thread's binding; with `delete_instance` the instance is deleted too.
#[tauri::command]
pub async fn warren_unbind_thread(app: AppHandle, mgr: Mgr<'_>, session_id: String, delete_instance: bool) -> Result<ChatSessionView> {
    let h = mgr.handle(&session_id)?;
    let prev = unbind(&mgr, &h);
    if let (true, Some(b)) = (delete_instance, prev) {
        adapter::forget_manifest(&b.instance_id);
        let client = adapter::connect(&mgr)?;
        adapter::delete_instance(&client, &b.instance_id).await?;
        crate::events::emit(&app, WARREN_CHANGED, serde_json::json!({ "deleted": b.instance_id }));
    }
    let v = h.session.lock().view(); Ok(v)
}

/// Manual sync: `push` sends what changed since the last push; `pull` brings back files the instance changed as
/// pending changes.
#[tauri::command]
pub async fn warren_sync(app: AppHandle, mgr: Mgr<'_>, session_id: String, direction: String) -> Result<SyncReport> {
    let h = mgr.handle(&session_id)?;
    let (binding, local_root, root) = { let s = h.session.lock(); (s.sandbox.clone(), local_root_of(&s), s.root()) };
    let binding = binding.filter(|b| b.provider == "warren").ok_or_else(|| CaretError::invalid("This thread has no Warren sandbox"))?;
    let settings = mgr.settings_for(root.as_deref());
    let client = adapter::connect(&mgr)?;
    let remote = adapter::WarrenRemote::new(client, binding.clone());
    let excludes = settings.warren.sync_excludes.clone();
    set_binding_status(&mgr, &session_id, "syncing", None);
    let started = Instant::now();
    let res: Result<SyncReport> = match direction.as_str() {
        "push" => remote.push_changes(&local_root, &excludes).await.map(|st| SyncReport { direction: "push".into(), files: st.files, bytes: st.bytes, deleted: st.deleted, ms: started.elapsed().as_millis() as u64 }),
        "pull" => async {
            let pulled = remote.pull_changes(&local_root, &excludes).await?;
            let bytes: u64 = pulled.changed.iter().map(|(_, b)| b.len() as u64).sum();
            let ctx = sync_context(&app, &mgr, &h, settings.clone());
            let applied = apply_pulled(&ctx, RemoteSync { local_root: local_root.clone(), pushed_files: 0, changed: pulled.changed, deleted: pulled.deleted }).await?;
            let deleted = applied.changes.iter().filter(|c| c.kind == ChangeKind::Delete).count() as u64;
            Ok(SyncReport { direction: "pull".into(), files: applied.changes.len() as u64 + applied.binaries.len() as u64 - deleted, bytes, deleted, ms: started.elapsed().as_millis() as u64 })
        }.await,
        other => Err(CaretError::invalid(format!("direction must be push or pull, not `{other}`"))),
    };
    match &res {
        Ok(r) => update_binding(&mgr, &session_id, |b| { if r.direction == "push" { adapter::record_push(b, r.files + r.deleted); } else { adapter::record_pull(b); } adapter::set_status(b, "ready", None); }),
        Err(e) => set_binding_status(&mgr, &session_id, "error", Some(e.message.clone())),
    }
    res
}

/// A tool context for changes made outside a tool call (manual pull): staged under the running turn when there is
/// one, else under a fresh id.
fn sync_context(app: &AppHandle, mgr: &Arc<AgentManager>, h: &SessionHandle, settings: Arc<Settings>) -> ToolContext {
    let s = h.session.lock();
    let mode = Arc::new(modes::resolve(&settings, &s.mode));
    ToolContext { app: Some(app.clone()), manager: mgr.clone(), session_id: s.id.clone(), message_id: String::new(), tool_call_id: "warren-sync".into(), turn_id: s.current_turn.clone().unwrap_or_else(short_id), root: s.root(), cwd: s.cwd_path(), settings, cancel: CancellationToken::new(), mode, executor: Arc::new(LocalExecutor::default()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_roots_follow_the_folder() {
        let mut st = Settings::default(); st.warren.remote_root = "/home/agent/work/".into();
        assert_eq!(remote_root_for(&st, Path::new("/Users/me/My Proj")), "/home/agent/work/My Proj");
        assert_eq!(instance_name_for(Path::new("/Users/me/My Proj"), "9f8e7d6c-1234"), "my-proj-9f8e7d");
        assert_eq!(instance_name_for(Path::new("/"), "abcdef12"), "workspace-abcdef");
        let mut s = Session::default(); s.workspace_root = "/w/repo".into(); s.cwd = "/w/repo".into();
        assert_eq!(local_root_of(&s), PathBuf::from("/w/repo"));
        s.worktree_path = Some("/w/.caret/wt/x".into()); s.cwd = "/w/.caret/wt/x".into();
        assert_eq!(local_root_of(&s), PathBuf::from("/w/.caret/wt/x"), "worktree threads sync the worktree");
    }

    #[test]
    fn safe_join_refuses_escapes_and_git() {
        let r = Path::new("/w");
        assert_eq!(safe_join(r, "src/a.rs"), Some(PathBuf::from("/w/src/a.rs")));
        for bad in ["", "/etc/passwd", "../x", "a/../../x", ".git", ".git/config"] { assert!(safe_join(r, bad).is_none(), "{bad}"); }
        assert!(safe_join(r, ".gitignore").is_some());
    }

    #[test]
    fn sync_report_serialises_camel_case() {
        let r = SyncReport { direction: "push".into(), files: 3, bytes: 10, deleted: 1, ms: 5 };
        assert_eq!(serde_json::to_string(&r).unwrap(), r#"{"direction":"push","files":3,"bytes":10,"deleted":1,"ms":5}"#);
    }
}

//! AgentManager: owns live sessions, the UI event channel, approvals/questions, queueing,
//! pending changes, checkpoints and subagent execution.
use crate::ai::agent::checkpoints;
use crate::ai::agent::modes::{ModeSpec, ToolFilter};
use crate::ai::agent::session::*;
use crate::ai::agent::subagents;
use crate::ai::agent::tools::{search_tools, ToolContext, ToolRegistry};
use crate::core::sessions::{workspace_key, SessionStore};
use crate::core::settings::{Settings, SettingsState};
use crate::core::workspace::WorkspaceState;
use crate::error::{CaretError, Result};
use dashmap::DashMap;
use once_cell::sync::OnceCell;
use parking_lot::{Mutex, RwLock};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

pub const MAX_LOADED_SESSIONS: usize = 8;
pub const MAX_CHANGE_CONTENT: usize = 2 * 1024 * 1024;

pub struct SessionHandle {
    pub session: Mutex<Session>,
    pub cancel: Mutex<CancellationToken>,
    pub running: AtomicBool,
    pub last_used: Mutex<Instant>,
}
impl SessionHandle {
    pub fn new(s: Session) -> Arc<Self> { Arc::new(Self { session: Mutex::new(s), cancel: Mutex::new(CancellationToken::new()), running: AtomicBool::new(false), last_used: Mutex::new(Instant::now()) }) }
    pub fn id(&self) -> String { self.session.lock().id.clone() }
    pub fn is_running(&self) -> bool { self.running.load(Ordering::SeqCst) }
    pub fn touch(&self) { *self.last_used.lock() = Instant::now(); }
    pub fn token(&self) -> CancellationToken { self.cancel.lock().clone() }
    pub fn reset_token(&self) -> CancellationToken { let t = CancellationToken::new(); *self.cancel.lock() = t.clone(); t }
}

pub struct AgentManager {
    app: OnceCell<AppHandle>,
    sessions: DashMap<String, Arc<SessionHandle>>,
    channel: RwLock<Option<Channel<AgentEvent>>>,
    approvals: DashMap<String, oneshot::Sender<ApprovalOption>>,
    questions: DashMap<String, oneshot::Sender<String>>,
    pub store: SessionStore,
    pub registry: ToolRegistry,
    file_cache: Mutex<Option<(PathBuf, Instant, Arc<Vec<String>>)>>,
    /// Workspace root used when no AppHandle is attached (tests / headless).
    fallback_root: Mutex<Option<PathBuf>>,
    #[cfg(test)] pub test_events: Mutex<Vec<AgentEvent>>,
}

impl Default for AgentManager { fn default() -> Self { Self::new() } }

impl AgentManager {
    pub fn new() -> Self {
        Self { app: OnceCell::new(), sessions: DashMap::new(), channel: RwLock::new(None), approvals: DashMap::new(), questions: DashMap::new(), store: SessionStore::default(), registry: ToolRegistry::builtin(), file_cache: Mutex::new(None), fallback_root: Mutex::new(None), #[cfg(test)] test_events: Mutex::new(vec![]) }
    }
    pub fn attach(&self, app: AppHandle) { let _ = self.app.set(app); }
    pub fn app(&self) -> AppHandle { self.app.get().expect("AgentManager not attached").clone() }
    pub fn app_opt(&self) -> Option<AppHandle> { self.app.get().cloned() }
    /// Headless mode (no Tauri app): set the workspace root explicitly.
    pub fn set_fallback_root(&self, root: Option<PathBuf>) { *self.fallback_root.lock() = root; }
    pub fn settings(&self) -> Arc<Settings> { Arc::new(self.app_opt().map(|a| a.state::<SettingsState>().effective()).unwrap_or_else(|| SettingsState::load().effective())) }
    pub fn workspace_root(&self) -> Option<PathBuf> { match self.app_opt() { Some(a) => a.state::<WorkspaceState>().root(), None => self.fallback_root.lock().clone() } }
    pub fn ws_key(&self) -> String { workspace_key(self.workspace_root().as_deref()) }
    pub fn ws_key_of(&self, s: &Session) -> String { workspace_key(s.root().as_deref()) }

    // ───────── events ─────────
    pub fn subscribe(&self, ch: Channel<AgentEvent>) { *self.channel.write() = Some(ch); }
    pub fn emit(&self, ev: AgentEvent) {
        #[cfg(test)] { self.test_events.lock().push(ev.clone()); }
        if let Some(ch) = self.channel.read().as_ref() { if let Err(e) = ch.send(ev) { tracing::debug!(error = %e, "agent event send failed"); } }
    }
    pub fn set_status(&self, h: &SessionHandle, status: SessionStatus) {
        let id = { let mut s = h.session.lock(); if s.status == status { return; } s.status = status; s.id.clone() };
        self.emit(AgentEvent::SessionStatus { session_id: id, status });
    }

    // ───────── sessions ─────────
    pub fn get_loaded(&self, id: &str) -> Option<Arc<SessionHandle>> { self.sessions.get(id).map(|h| { h.touch(); h.clone() }) }
    pub fn handle(&self, id: &str) -> Result<Arc<SessionHandle>> {
        if let Some(h) = self.get_loaded(id) { return Ok(h); }
        let key = self.ws_key();
        let s = self.store.load(&key, id).or_else(|_| self.store.load("global", id))?;
        let h = SessionHandle::new(s);
        self.sessions.insert(id.to_string(), h.clone());
        self.evict_idle();
        Ok(h)
    }
    fn evict_idle(&self) {
        if self.sessions.len() <= MAX_LOADED_SESSIONS { return; }
        let mut idle: Vec<(Instant, String)> = self.sessions.iter().filter(|e| !e.value().is_running()).map(|e| (*e.value().last_used.lock(), e.key().clone())).collect();
        idle.sort();
        for (_, id) in idle.into_iter().take(self.sessions.len().saturating_sub(MAX_LOADED_SESSIONS)) {
            if let Some((_, h)) = self.sessions.remove(&id) { let s = h.session.lock(); let _ = self.store.save_now(&self.ws_key_of(&s), &s); }
        }
    }
    pub fn persist(&self, h: &SessionHandle) { let s = h.session.lock(); self.store.save(&self.ws_key_of(&s), &s); }
    pub fn persist_now(&self, h: &SessionHandle) { let s = h.session.lock(); let _ = self.store.save_now(&self.ws_key_of(&s), &s); }
    /// Record the latest verification report for a session (verify engine) and refresh listeners.
    pub fn set_last_verification(&self, session_id: &str, report_id: &str) {
        if let Some(h) = self.get_loaded(session_id) {
            { let mut s = h.session.lock(); s.last_verification_id = Some(report_id.to_string()); s.touch(); }
            self.persist(&h); // the verify Emitter follows with AgentEvent::Verification carrying the report
        }
    }

    pub async fn create_session(&self, mode: Option<String>, model_id: Option<String>, title: Option<String>, use_worktree: bool, parent: Option<(&str, bool)>) -> Result<Arc<SessionHandle>> {
        let settings = self.settings();
        let root = self.workspace_root();
        let mut s = Session::default();
        s.mode = mode.unwrap_or_else(|| settings.agent.default_mode.clone());
        // "Auto" (None/"auto") stays unresolved on the session so every turn can route through the Auto router or the
        // current default; an explicit pick is pinned. (Previously the default id was baked in at creation time.)
        s.model_id = model_id.filter(|m| !m.is_empty() && m != "auto");
        if let Some(t) = title { s.title = t; s.title_generated = true; }
        s.workspace_root = root.as_ref().map(|r| r.to_string_lossy().to_string()).unwrap_or_default();
        s.cwd = s.workspace_root.clone();
        if let Some((pid, _)) = parent { s.parent_session_id = Some(pid.to_string()); s.is_subagent = true; }
        if use_worktree {
            let root = root.clone().ok_or_else(CaretError::no_workspace)?;
            let (path, branch) = create_worktree(&root, &s.title, &s.id).await?;
            s.worktree_path = Some(path.to_string_lossy().to_string()); s.branch = Some(branch); s.cwd = path.to_string_lossy().to_string();
        }
        let h = SessionHandle::new(s);
        let id = h.id();
        self.sessions.insert(id.clone(), h.clone());
        self.persist_now(&h);
        self.emit(AgentEvent::SessionCreated { session: h.session.lock().summary_view() });
        self.evict_idle();
        Ok(h)
    }

    pub fn list_sessions(&self, include_archived: bool) -> Vec<SessionSummary> {
        let key = self.ws_key();
        let mut list = self.store.read_index(&key);
        for e in self.sessions.iter() {
            let s = e.value().session.lock();
            if self.ws_key_of(&s) != key { continue; }
            let sv = s.summary_view();
            if let Some(x) = list.iter_mut().find(|x| x.id == sv.id) { *x = sv; } else { list.push(sv); }
        }
        list.retain(|s| include_archived || s.archived != Some(true));
        list.sort_by(|a, b| b.pinned.unwrap_or(false).cmp(&a.pinned.unwrap_or(false)).then(b.updated_at.cmp(&a.updated_at)));
        list
    }

    pub async fn delete_session(&self, id: &str) -> Result<()> {
        self.stop(id);
        let (worktree, root, ws_key) = match self.handle(id) { Ok(h) => { let s = h.session.lock(); (s.worktree_path.clone(), s.root(), self.ws_key_of(&s)) } Err(_) => (None, self.workspace_root(), self.ws_key()) };
        if let (Some(wt), Some(root)) = (worktree, root) { let _ = tokio::process::Command::new("git").args(["worktree", "remove", "--force", &wt]).current_dir(&root).output().await; }
        self.sessions.remove(id);
        self.store.delete(&ws_key, id)?;
        checkpoints::remove_session(&ws_key, id);
        self.emit(AgentEvent::SessionDeleted { session_id: id.to_string() });
        Ok(())
    }

    // ───────── run / queue / stop ─────────
    pub async fn submit(self: &Arc<Self>, req: SendMessageRequest) -> Result<()> {
        let h = self.handle(&req.session_id)?;
        if h.is_running() {
            let q = QueuedMessage { id: new_id(), text: req.text.clone(), context: req.context.clone(), images: req.images.clone(), mode: Some(req.mode.clone()), request: Some(req) };
            let queued = { let mut s = h.session.lock(); s.queued.push(q); s.queued.clone() };
            self.emit(AgentEvent::Queue { session_id: h.id(), queued });
            self.persist(&h);
            return Ok(());
        }
        self.spawn_drive(h, req, None);
        Ok(())
    }
    pub fn spawn_drive(self: &Arc<Self>, h: Arc<SessionHandle>, req: SendMessageRequest, mode_override: Option<ModeSpec>) {
        h.running.store(true, Ordering::SeqCst);
        let mgr = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut next = Some(req);
            while let Some(r) = next.take() {
                let cancel = h.reset_token();
                if let Err(e) = crate::ai::agent::runner::run_turn(mgr.clone(), h.clone(), r, mode_override.clone(), cancel).await { tracing::warn!(error=%e, "turn failed"); }
                // Dequeue next message unless cancelled explicitly with an empty queue.
                let q = { let mut s = h.session.lock(); if s.queued.is_empty() { None } else { Some(s.queued.remove(0)) } };
                if let Some(q) = q {
                    mgr.emit(AgentEvent::Queue { session_id: h.id(), queued: h.session.lock().queued.clone() });
                    next = Some(q.request.unwrap_or(SendMessageRequest { session_id: h.id(), text: q.text, context: q.context, images: q.images, mode: q.mode.unwrap_or_else(|| h.session.lock().mode.clone()), ..Default::default() }));
                }
            }
            h.running.store(false, Ordering::SeqCst);
            mgr.set_status(&h, SessionStatus::Idle);
            mgr.persist_now(&h);
        });
    }
    pub fn stop(&self, id: &str) {
        if let Some(h) = self.get_loaded(id) { h.token().cancel(); }
        // Drop pending waiters for this session so the turn unblocks.
        self.approvals.retain(|_, _| true);
    }

    // ───────── approvals / questions ─────────
    pub async fn request_approval(&self, h: &SessionHandle, message_id: &str, req: ApprovalRequest, cancel: &CancellationToken) -> Result<ApprovalOption> {
        let (tx, rx) = oneshot::channel();
        self.approvals.insert(req.approval_id.clone(), tx);
        { let mut s = h.session.lock(); if let Some(call) = s.find_tool_mut(message_id, &req.tool_call_id) { call.status = ToolStatus::AwaitingApproval; call.approval = Some(req.clone()); let c = call.clone(); let sid = s.id.clone(); drop(s); self.emit(AgentEvent::ToolUpdate { session_id: sid, message_id: message_id.to_string(), call: c }); } }
        self.set_status(h, SessionStatus::AwaitingApproval);
        self.emit(AgentEvent::ApprovalRequest { session_id: h.id(), request: req.clone() });
        self.persist(h);
        let res = tokio::select! { r = rx => r.map_err(|_| CaretError::cancelled()), _ = cancel.cancelled() => { self.approvals.remove(&req.approval_id); Err(CaretError::cancelled()) } };
        self.set_status(h, SessionStatus::Running);
        res
    }
    pub fn resolve_approval(&self, approval_id: &str, option: ApprovalOption) -> Result<()> {
        let (_, tx) = self.approvals.remove(approval_id).ok_or_else(|| CaretError::not_found(format!("approval {approval_id}")))?;
        tx.send(option).map_err(|_| CaretError::internal("approval receiver dropped"))
    }
    pub async fn ask_question(&self, session_id: &str, message_id: &str, q: AgentQuestion, cancel: &CancellationToken) -> Result<String> {
        let h = self.handle(session_id)?;
        let (tx, rx) = oneshot::channel();
        self.questions.insert(q.question_id.clone(), tx);
        { let mut s = h.session.lock(); if let Some(m) = s.message_mut(message_id) { m.parts.push(MessagePart::Question { question: q.clone(), answer: None }); } }
        self.set_status(&h, SessionStatus::AwaitingInput);
        self.emit(AgentEvent::Question { session_id: session_id.to_string(), message_id: message_id.to_string(), question: q.clone() });
        self.persist(&h);
        let res = tokio::select! { r = rx => r.map_err(|_| CaretError::cancelled()), _ = cancel.cancelled() => { self.questions.remove(&q.question_id); Err(CaretError::cancelled()) } };
        if let Ok(ans) = &res { let mut s = h.session.lock(); if let Some(m) = s.message_mut(message_id) { for p in &mut m.parts { if let MessagePart::Question { question, answer } = p { if question.question_id == q.question_id { *answer = Some(ans.clone()); } } } } }
        self.set_status(&h, SessionStatus::Running);
        res
    }
    pub fn resolve_question(&self, question_id: &str, answer: String) -> Result<()> {
        let (_, tx) = self.questions.remove(question_id).ok_or_else(|| CaretError::not_found(format!("question {question_id}")))?;
        tx.send(answer).map_err(|_| CaretError::internal("question receiver dropped"))
    }

    // ───────── changes / checkpoints / todos / plans ─────────
    pub fn snapshot_before_write(&self, session_id: &str, turn_id: &str, abs: &Path, current: Option<&str>) {
        let Some(h) = self.get_loaded(session_id) else { return };
        let ws_key = { let s = h.session.lock(); self.ws_key_of(&s) };
        match checkpoints::snapshot(&ws_key, session_id, turn_id, abs, current) {
            Ok(true) => { let mut s = h.session.lock(); if let Some(cp) = s.checkpoints.iter_mut().find(|c| c.id == turn_id) { cp.files.push(abs.to_string_lossy().to_string()); } }
            Ok(false) => {}
            Err(e) => tracing::warn!(error=%e, "checkpoint snapshot failed"),
        }
    }
    pub fn add_change(&self, session_id: &str, mut change: PendingChange) -> FileChangeSummary {
        if change.before.as_ref().map(|b| b.len() > MAX_CHANGE_CONTENT).unwrap_or(false) { change.before = None; }
        if change.after.as_ref().map(|a| a.len() > MAX_CHANGE_CONTENT).unwrap_or(false) { change.after = None; }
        let summary = change.summary();
        let diff = if change.diff.len() < 200_000 { Some(change.diff.clone()) } else { None };
        if let Some(h) = self.get_loaded(session_id) { { let mut s = h.session.lock(); s.changes.push(change); s.touch(); } self.persist(&h); }
        self.emit(AgentEvent::FileChange { session_id: session_id.to_string(), change: summary.clone(), diff });
        summary
    }
    pub fn change_diff(&self, change_id: &str) -> Option<String> {
        for e in self.sessions.iter() { let s = e.value().session.lock(); if let Some(c) = s.changes.iter().find(|c| c.change_id == change_id) { return Some(c.diff.clone()); } }
        None
    }
    pub fn find_change(&self, change_id: &str) -> Option<PendingChange> {
        for e in self.sessions.iter() { let s = e.value().session.lock(); if let Some(c) = s.changes.iter().find(|c| c.change_id == change_id) { return Some(c.clone()); } }
        None
    }
    /// Accept (keep) pending changes. With auto-apply disabled, accepting writes `after` to disk.
    pub fn accept_changes(&self, session_id: &str, ids: Option<Vec<String>>) -> Result<()> {
        let h = self.handle(session_id)?;
        let auto = self.settings().agent.auto_apply_edits;
        let mut done = vec![];
        { let mut s = h.session.lock(); let cwd = s.cwd_path();
          let targets: Vec<usize> = s.changes.iter().enumerate().filter(|(_, c)| c.status == ChangeStatus::Pending && ids.as_ref().map(|v| v.contains(&c.change_id)).unwrap_or(true)).map(|(i, _)| i).collect();
          for i in targets {
              let c = &mut s.changes[i];
              if !auto { let abs = crate::paths::resolve(Some(&cwd), &c.path); match c.kind { ChangeKind::Delete => { let _ = std::fs::remove_file(&abs); } _ => { if let Some(a) = &c.after { if let Some(p) = abs.parent() { let _ = std::fs::create_dir_all(p); } let _ = std::fs::write(&abs, a); } } } }
              c.status = ChangeStatus::Accepted; c.before = None; c.after = None; done.push(c.change_id.clone());
          }
          s.touch(); }
        self.emit(AgentEvent::FileChangeStatus { session_id: session_id.to_string(), change_ids: done, status: ChangeStatus::Accepted });
        self.persist(&h);
        Ok(())
    }
    /// Reject pending changes: restores the pre-change content (earliest pending change per path wins).
    pub fn reject_changes(&self, session_id: &str, ids: Option<Vec<String>>) -> Result<()> {
        let h = self.handle(session_id)?;
        let auto = self.settings().agent.auto_apply_edits;
        let mut done = vec![];
        { let mut s = h.session.lock(); let cwd = s.cwd_path();
          let targets: Vec<usize> = s.changes.iter().enumerate().filter(|(_, c)| c.status == ChangeStatus::Pending && ids.as_ref().map(|v| v.contains(&c.change_id)).unwrap_or(true)).map(|(i, _)| i).collect();
          let mut restored_paths: std::collections::HashSet<String> = Default::default();
          for i in targets {
              let (path, kind, before, old_path) = { let c = &s.changes[i]; (c.path.clone(), c.kind, c.before.clone(), c.old_path.clone()) };
              if auto && restored_paths.insert(path.clone()) {
                  // earliest pending change for this path determines the restore content
                  let earliest = s.changes.iter().find(|c| c.path == path && c.status == ChangeStatus::Pending).map(|c| (c.kind, c.before.clone(), c.old_path.clone())).unwrap_or((kind, before, old_path));
                  let abs = crate::paths::resolve(Some(&cwd), &path);
                  match earliest { (ChangeKind::Create, _, _) => { let _ = std::fs::remove_file(&abs); }
                      (ChangeKind::Rename, b, Some(op)) => { let oa = crate::paths::resolve(Some(&cwd), &op); let _ = std::fs::rename(&abs, &oa).or_else(|_| b.map(|b| std::fs::write(&oa, b)).unwrap_or(Ok(()))); }
                      (_, Some(b), _) => { if let Some(p) = abs.parent() { let _ = std::fs::create_dir_all(p); } let _ = std::fs::write(&abs, b); }
                      _ => {} }
              }
              let c = &mut s.changes[i]; c.status = ChangeStatus::Rejected; c.before = None; c.after = None; done.push(c.change_id.clone());
          }
          s.touch(); }
        self.emit(AgentEvent::FileChangeStatus { session_id: session_id.to_string(), change_ids: done, status: ChangeStatus::Rejected });
        self.persist(&h);
        Ok(())
    }
    pub fn restore_checkpoint(&self, session_id: &str, checkpoint_id: &str) -> Result<Vec<String>> {
        let h = self.handle(session_id)?;
        if h.is_running() { return Err(CaretError::invalid("Stop the agent before restoring a checkpoint")); }
        let mut restored = vec![];
        { let mut s = h.session.lock(); let ws_key = self.ws_key_of(&s);
          let Some(idx) = s.checkpoints.iter().position(|c| c.id == checkpoint_id) else { return Err(CaretError::not_found(format!("checkpoint {checkpoint_id}"))) };
          let to_undo: Vec<Checkpoint> = s.checkpoints[idx..].iter().rev().cloned().collect();
          for cp in &to_undo { restored.extend(checkpoints::restore_turn(&ws_key, session_id, &cp.id)?); }
          let target = s.checkpoints[idx].clone();
          if let Some(mi) = s.messages.iter().position(|m| m.id == target.message_id) { s.messages.truncate(mi); }
          if let Some((_, len)) = s.transcript_marks.iter().find(|(t, _)| t == &target.id).cloned() { s.transcript.truncate(len); }
          let removed: std::collections::HashSet<String> = to_undo.iter().map(|c| c.id.clone()).collect();
          s.changes.retain(|c| c.turn_id.as_ref().map(|t| !removed.contains(t)).unwrap_or(true));
          s.transcript_marks.retain(|(t, _)| !removed.contains(t));
          s.checkpoints.truncate(idx);
          s.context_tokens = s.estimate_context_tokens();
          s.touch(); }
        self.persist_now(&h);
        self.emit(AgentEvent::SessionReset { session_id: session_id.to_string() });
        Ok(restored)
    }
    pub fn update_todos(&self, session_id: &str, items: Vec<TodoItem>, merge: bool) -> Vec<TodoItem> {
        let Some(h) = self.get_loaded(session_id) else { return items };
        let todos = { let mut s = h.session.lock(); if merge { for it in items { if let Some(e) = s.todos.iter_mut().find(|t| t.id == it.id) { *e = it; } else { s.todos.push(it); } } } else { s.todos = items; } s.touch(); s.todos.clone() };
        self.emit(AgentEvent::Todos { session_id: session_id.to_string(), todos: todos.clone() });
        self.persist(&h);
        todos
    }
    pub fn emit_plan(&self, session_id: &str, message_id: &str, path: &str, title: &str) {
        if let Some(h) = self.get_loaded(session_id) { let mut s = h.session.lock(); if let Some(m) = s.message_mut(message_id) { m.parts.push(MessagePart::Plan { path: path.to_string(), title: title.to_string() }); } }
        self.emit(AgentEvent::Plan { session_id: session_id.to_string(), path: path.to_string(), title: title.to_string() });
    }

    // ───────── file list cache ─────────
    pub async fn file_list(&self, cwd: &Path) -> Arc<Vec<String>> {
        // No workspace → cwd is the home dir: never walk it (privacy prompts + seconds of IO); offer no files instead.
        if crate::paths::is_broad_root(cwd) { return Arc::new(vec![]); }
        if let Some((p, t, v)) = self.file_cache.lock().as_ref() { if p == cwd && t.elapsed() < std::time::Duration::from_secs(15) { return v.clone(); } }
        let root = cwd.to_path_buf();
        let list = tokio::task::spawn_blocking(move || search_tools::list_files(&root, 30_000)).await.unwrap_or_default();
        let v = Arc::new(list);
        *self.file_cache.lock() = Some((cwd.to_path_buf(), Instant::now(), v.clone()));
        v
    }
    pub fn invalidate_file_cache(&self) { *self.file_cache.lock() = None; }

    // ───────── subagents ─────────
    pub async fn run_subagent(self: Arc<Self>, ctx: &ToolContext, description: &str, prompt: &str, name: Option<&str>, readonly: Option<bool>) -> Result<(String, String)> {
        let def = name.and_then(|n| subagents::get(ctx.root.as_deref(), n));
        if name.is_some() && def.is_none() { return Err(CaretError::not_found(format!("subagent '{}'", name.unwrap_or("")))); }
        let readonly = readonly.unwrap_or(def.as_ref().map(|d| d.readonly).unwrap_or(true));
        let tools = match &def { Some(d) if d.info.tools.as_ref().map(|t| !t.is_empty()).unwrap_or(false) => ToolFilter::Only { names: d.info.tools.clone().unwrap_or_default() }, _ if readonly => ToolFilter::Readonly, _ => ToolFilter::All };
        let instructions = def.as_ref().map(|d| d.system_prompt.clone()).unwrap_or_else(|| "You are a focused worker subagent. Complete exactly the delegated task using the tools available, then reply with a concise, information-dense report (findings with file paths and line numbers, what you changed, anything the parent agent must know). You cannot ask the user questions.".into());
        let mode = ModeSpec { id: "subagent".into(), name: def.as_ref().map(|d| d.info.name.clone()).unwrap_or_else(|| "Subagent".into()), description: description.into(), icon: None, instructions, tools, builtin: true, auto_run: None, model_id: def.as_ref().and_then(|d| d.info.model_id.clone()).or_else(|| ctx.settings.agent.subagent_model_id.clone()) };
        let child = self.create_session(Some("agent".into()), mode.model_id.clone().or_else(|| ctx.manager.get_loaded(&ctx.session_id).and_then(|h| h.session.lock().model_id.clone())), Some(format!("↳ {description}")), false, Some((&ctx.session_id, true))).await?;
        { let mut s = child.session.lock(); s.cwd = ctx.cwd.to_string_lossy().to_string(); }
        child.running.store(true, Ordering::SeqCst);
        let token = ctx.cancel.child_token();
        *child.cancel.lock() = token.clone();
        let req = SendMessageRequest { session_id: child.id(), text: prompt.to_string(), mode: "agent".into(), ..Default::default() };
        let res = crate::ai::agent::runner::run_turn(self.clone(), child.clone(), req, Some(mode), token).await;
        child.running.store(false, Ordering::SeqCst);
        self.set_status(&child, SessionStatus::Idle);
        self.persist_now(&child);
        let text = res?;
        Ok((child.id(), if text.trim().is_empty() { "(subagent produced no final text)".into() } else { text }))
    }
}

pub fn slugify(s: &str) -> String {
    let mut out: String = s.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    while out.contains("--") { out = out.replace("--", "-"); }
    out.trim_matches('-').chars().take(32).collect()
}

/// Create a git worktree under <root>/.caret/worktrees/<slug> on branch caret/<slug>.
pub async fn create_worktree(root: &Path, title: &str, id: &str) -> Result<(PathBuf, String)> {
    let mut slug = slugify(title); if slug.is_empty() || slug == "new-chat" { slug = format!("agent-{}", &id[..8]); } else { slug = format!("{}-{}", slug, &id[..4]); }
    let dir = root.join(".caret").join("worktrees"); std::fs::create_dir_all(&dir)?;
    // keep worktrees out of git status
    let exclude = root.join(".git").join("info").join("exclude");
    if let Ok(cur) = std::fs::read_to_string(&exclude) { if !cur.contains(".caret/worktrees") { let _ = std::fs::write(&exclude, format!("{cur}\n.caret/worktrees/\n")); } } else if root.join(".git").is_dir() { let _ = std::fs::create_dir_all(exclude.parent().unwrap()); let _ = std::fs::write(&exclude, ".caret/worktrees/\n"); }
    let path = dir.join(&slug); let branch = format!("caret/{slug}");
    let out = tokio::process::Command::new("git").args(["worktree", "add", "-b", &branch, path.to_str().unwrap_or("")]).current_dir(root).output().await.map_err(|e| CaretError::io(format!("git: {e}")))?;
    if !out.status.success() { return Err(CaretError::new("git", format!("git worktree add failed: {}", String::from_utf8_lossy(&out.stderr)))); }
    Ok((path, branch))
}

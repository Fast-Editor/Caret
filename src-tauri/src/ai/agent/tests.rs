//! Agent-core integration tests (headless: no Tauri app handle).
use crate::ai::agent::manager::{AgentManager, SessionHandle};
use crate::ai::agent::modes::{ModeSpec, ToolFilter};
use crate::ai::agent::policy::ToolEffect;
use crate::ai::agent::runner::{execute_one, ToolRun};
use crate::ai::agent::session::*;
use crate::ai::agent::testutil::{temp_workspace, test_home};
use crate::ai::agent::tools::{Tool, ToolContext, ToolOutput};
use crate::core::sessions::{workspace_key, SessionStore};
use crate::core::settings::Settings;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

fn mode_all() -> Arc<ModeSpec> { Arc::new(ModeSpec { id: "agent".into(), name: "Agent".into(), description: String::new(), icon: None, instructions: String::new(), tools: ToolFilter::All, builtin: true, auto_run: None, model_id: None }) }

fn ctx(mgr: &Arc<AgentManager>, h: &SessionHandle, settings: Settings, message_id: &str, call_id: &str) -> ToolContext {
    let s = h.session.lock();
    ToolContext { app: None, manager: mgr.clone(), session_id: s.id.clone(), message_id: message_id.into(), tool_call_id: call_id.into(), turn_id: "t1".into(), root: s.root(), cwd: s.cwd_path(), settings: Arc::new(settings), cancel: CancellationToken::new(), mode: mode_all() }
}

/// Session with a user message + streaming assistant message holding a pending tool record.
fn seed(h: &SessionHandle, call_id: &str, name: &str) -> String {
    let mut s = h.session.lock();
    let mut um = ChatMessage::new(MessageRole::User); um.turn_id = Some("t1".into()); um.parts.push(MessagePart::Text { text: "do it".into() });
    let sid = s.id.clone();
    s.checkpoints.push(Checkpoint { id: "t1".into(), session_id: sid, message_id: um.id.clone(), created_at: now_ms(), files: vec![], label: "do it".into() });
    s.transcript_marks.push(("t1".into(), 0));
    s.messages.push(um);
    let mut am = ChatMessage::new(MessageRole::Assistant); am.turn_id = Some("t1".into());
    am.parts.push(MessagePart::Tool { call: ToolCallRecord::new(call_id, name, name) });
    let id = am.id.clone(); s.messages.push(am); id
}

struct EchoShell;
#[async_trait::async_trait]
impl Tool for EchoShell {
    fn name(&self) -> &str { "run_terminal_cmd" }
    fn display_name(&self) -> &str { "Run" }
    fn description(&self) -> String { "x".into() }
    fn input_schema(&self) -> Value { json!({"type":"object","properties":{}}) }
    fn readonly(&self) -> bool { false }
    fn group(&self) -> &str { "t" }
    fn effect(&self, i: &Value) -> ToolEffect { ToolEffect::Shell(i["command"].as_str().unwrap_or("").into()) }
    async fn run(&self, _c: &ToolContext, i: Value) -> crate::error::Result<ToolOutput> { Ok(ToolOutput::text(format!("ran {}", i["command"]))) }
}

async fn new_mgr(ws: &std::path::Path) -> (Arc<AgentManager>, Arc<SessionHandle>) {
    test_home();
    let mgr = Arc::new(AgentManager::new());
    mgr.set_fallback_root(Some(ws.to_path_buf()));
    let h = mgr.create_session(Some("agent".into()), None, Some("Test".into()), false, None).await.unwrap();
    (mgr, h)
}

#[tokio::test]
async fn session_persistence_round_trip() {
    let ws = temp_workspace();
    let store = SessionStore::default();
    let key = workspace_key(Some(ws.path()));
    let mut s = Session::default(); s.title = "Round trip".into(); s.workspace_root = ws.path().to_string_lossy().into();
    s.messages.push({ let mut m = ChatMessage::new(MessageRole::User); m.parts.push(MessagePart::Text { text: "hello".into() }); m });
    s.transcript.push(crate::ai::llm::Message::user_text("hello"));
    store.save_now(&key, &s).unwrap();
    let loaded = store.load(&key, &s.id).unwrap();
    assert_eq!(loaded.title, "Round trip"); assert_eq!(loaded.transcript.len(), 1); assert_eq!(loaded.messages[0].text(), "hello");
    let idx = store.read_index(&key); assert!(idx.iter().any(|x| x.id == s.id && x.message_count == 1));
    assert!(SessionStore::export_markdown(&loaded).contains("hello"));
    store.delete(&key, &s.id).unwrap();
    assert!(store.load(&key, &s.id).is_err());
}

#[tokio::test]
async fn shell_tool_requires_approval_when_autorun_off_and_allow_runs_it() {
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let mid = seed(&h, "c1", "run_terminal_cmd");
    let mut settings = Settings::default(); settings.agent.auto_run = "off".into();
    let c = ctx(&mgr, &h, settings, &mid, "c1");
    let tool: Arc<dyn Tool> = Arc::new(EchoShell);
    let run = ToolRun { id: "c1".into(), name: "run_terminal_cmd".into(), input: json!({"command": "echo hi"}), tool: Some(tool) };
    let (m2, h2) = (mgr.clone(), h.clone());
    let approver = tokio::spawn(async move {
        for _ in 0..100 { tokio::time::sleep(std::time::Duration::from_millis(20)).await; let req = m2.test_events.lock().iter().rev().find_map(|e| if let AgentEvent::ApprovalRequest { request, .. } = e { Some(request.clone()) } else { None }); if let Some(r) = req { assert_eq!(r.kind, ApprovalKind::Command); assert!(r.options.contains(&ApprovalOption::AllowAlways)); m2.resolve_approval(&r.approval_id, ApprovalOption::AllowSession).unwrap(); return; } }
        let _ = h2; panic!("no approval request emitted");
    });
    let (_, out) = execute_one(mgr.clone(), h.clone(), c, run).await;
    approver.await.unwrap();
    assert!(!out.is_error); assert!(out.text.contains("ran"));
    assert!(h.session.lock().session_allow.contains(&"cmd:echo".to_string()));
    let rec = h.session.lock().find_tool_mut(&mid, "c1").unwrap().clone();
    assert_eq!(rec.status, ToolStatus::Done);
    // second call with the same prefix is auto-allowed this session (no approval event)
    let before = mgr.test_events.lock().len();
    let mid2 = seed(&h, "c2", "run_terminal_cmd");
    let mut settings = Settings::default(); settings.agent.auto_run = "off".into();
    let c2 = ctx(&mgr, &h, settings, &mid2, "c2");
    let (_, out2) = execute_one(mgr.clone(), h.clone(), c2, ToolRun { id: "c2".into(), name: "run_terminal_cmd".into(), input: json!({"command": "echo again"}), tool: Some(Arc::new(EchoShell)) }).await;
    assert!(!out2.is_error);
    assert!(!mgr.test_events.lock()[before..].iter().any(|e| matches!(e, AgentEvent::ApprovalRequest { .. })));
}

#[tokio::test]
async fn denied_approval_rejects_tool() {
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let mid = seed(&h, "c1", "run_terminal_cmd");
    let mut settings = Settings::default(); settings.agent.auto_run = "off".into();
    let c = ctx(&mgr, &h, settings, &mid, "c1");
    let m2 = mgr.clone();
    tokio::spawn(async move { for _ in 0..100 { tokio::time::sleep(std::time::Duration::from_millis(20)).await; let req = m2.test_events.lock().iter().rev().find_map(|e| if let AgentEvent::ApprovalRequest { request, .. } = e { Some(request.approval_id.clone()) } else { None }); if let Some(id) = req { m2.resolve_approval(&id, ApprovalOption::Deny).unwrap(); return; } } });
    let (_, out) = execute_one(mgr.clone(), h.clone(), c, ToolRun { id: "c1".into(), name: "run_terminal_cmd".into(), input: json!({"command": "rm -rf build"}), tool: Some(Arc::new(EchoShell)) }).await;
    assert!(out.is_error); assert!(out.text.contains("denied"));
    assert_eq!(h.session.lock().find_tool_mut(&mid, "c1").unwrap().status, ToolStatus::Rejected);
}

#[tokio::test]
async fn denylisted_command_is_denied_without_prompt() {
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let mid = seed(&h, "c1", "run_terminal_cmd");
    let c = ctx(&mgr, &h, Settings::default(), &mid, "c1");
    let (_, out) = execute_one(mgr.clone(), h.clone(), c, ToolRun { id: "c1".into(), name: "run_terminal_cmd".into(), input: json!({"command": "sudo rm -rf /"}), tool: Some(Arc::new(EchoShell)) }).await;
    assert!(out.is_error); assert!(out.text.contains("denylist"));
    assert!(!mgr.test_events.lock().iter().any(|e| matches!(e, AgentEvent::ApprovalRequest { .. })));
}

#[tokio::test]
async fn edit_file_records_change_and_reject_restores() {
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let mid = seed(&h, "e1", "edit_file");
    let c = ctx(&mgr, &h, Settings::default(), &mid, "e1");
    let tool = mgr.registry.get("edit_file").unwrap();
    let (_, out) = execute_one(mgr.clone(), h.clone(), c, ToolRun { id: "e1".into(), name: "edit_file".into(), input: json!({"path": "src/lib.rs", "edits": [{"old_string": "a + b", "new_string": "a - b"}]}), tool: Some(tool) }).await;
    assert!(!out.is_error, "{}", out.text);
    assert_eq!(std::fs::read_to_string(ws.path().join("src/lib.rs")).unwrap(), "pub fn add(a: i32, b: i32) -> i32 { a - b }\n");
    let changes = h.session.lock().changes.clone();
    assert_eq!(changes.len(), 1); assert_eq!(changes[0].status, ChangeStatus::Pending); assert_eq!(changes[0].additions, 1); assert_eq!(changes[0].deletions, 1);
    assert!(mgr.test_events.lock().iter().any(|e| matches!(e, AgentEvent::FileChange { .. })));
    // checkpoint recorded the file
    assert!(h.session.lock().checkpoints[0].files.iter().any(|f| f.ends_with("src/lib.rs")));
    // reject → restored
    let sid = h.id();
    mgr.reject_changes(&sid, None).unwrap();
    assert_eq!(std::fs::read_to_string(ws.path().join("src/lib.rs")).unwrap(), "pub fn add(a: i32, b: i32) -> i32 { a + b }\n");
    assert_eq!(h.session.lock().changes[0].status, ChangeStatus::Rejected);
}

#[tokio::test]
async fn ambiguous_edit_is_refused_and_write_then_checkpoint_restore() {
    let ws = temp_workspace();
    std::fs::write(ws.path().join("dup.txt"), "x = 1\ny\nx = 1\n").unwrap();
    let (mgr, h) = new_mgr(ws.path()).await;
    let mid = seed(&h, "e1", "edit_file");
    let c = ctx(&mgr, &h, Settings::default(), &mid, "e1");
    let (_, out) = execute_one(mgr.clone(), h.clone(), c, ToolRun { id: "e1".into(), name: "edit_file".into(), input: json!({"path": "dup.txt", "edits": [{"old_string": "x = 1", "new_string": "x = 2"}]}), tool: Some(mgr.registry.get("edit_file").unwrap()) }).await;
    assert!(out.is_error); assert!(out.text.contains("matches 2 places"));
    // write_file creates a new file; restoring the checkpoint deletes it and truncates the transcript
    let mid2 = seed_same_turn(&h, "w1", "write_file");
    let c2 = ctx(&mgr, &h, Settings::default(), &mid2, "w1");
    let (_, out2) = execute_one(mgr.clone(), h.clone(), c2, ToolRun { id: "w1".into(), name: "write_file".into(), input: json!({"path": "new/file.txt", "content": "hello"}), tool: Some(mgr.registry.get("write_file").unwrap()) }).await;
    assert!(!out2.is_error, "{}", out2.text);
    assert!(ws.path().join("new/file.txt").exists());
    let sid = h.id();
    let restored = mgr.restore_checkpoint(&sid, "t1").unwrap();
    assert!(restored.iter().any(|p| p.ends_with("new/file.txt")));
    assert!(!ws.path().join("new/file.txt").exists());
    let s = h.session.lock(); assert!(s.messages.is_empty()); assert!(s.checkpoints.is_empty()); assert!(s.changes.is_empty());
}
fn seed_same_turn(h: &SessionHandle, call_id: &str, name: &str) -> String {
    let mut s = h.session.lock();
    let mut am = ChatMessage::new(MessageRole::Assistant); am.turn_id = Some("t1".into());
    am.parts.push(MessagePart::Tool { call: ToolCallRecord::new(call_id, name, name) });
    let id = am.id.clone(); s.messages.push(am); id
}

#[tokio::test]
async fn read_list_grep_tools_work() {
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let mid = seed(&h, "r1", "read_file");
    let c = ctx(&mgr, &h, Settings::default(), &mid, "r1");
    let (_, out) = execute_one(mgr.clone(), h.clone(), c, ToolRun { id: "r1".into(), name: "read_file".into(), input: json!({"path": "main.rs"}), tool: Some(mgr.registry.get("read_file").unwrap()) }).await;
    assert!(out.text.contains("1| fn main()"), "{}", out.text);
    let c = ctx(&mgr, &h, Settings::default(), &mid, "r1");
    let (_, out) = execute_one(mgr.clone(), h.clone(), c, ToolRun { id: "r1".into(), name: "list_dir".into(), input: json!({"path": ".", "depth": 2}), tool: Some(mgr.registry.get("list_dir").unwrap()) }).await;
    assert!(out.text.contains("src/lib.rs"), "{}", out.text);
    let c = ctx(&mgr, &h, Settings::default(), &mid, "r1");
    let (_, out) = execute_one(mgr.clone(), h.clone(), c, ToolRun { id: "r1".into(), name: "grep_search".into(), input: json!({"pattern": "println"}), tool: Some(mgr.registry.get("grep_search").unwrap()) }).await;
    assert!(out.text.contains("main.rs:"), "{}", out.text);
    let c = ctx(&mgr, &h, Settings::default(), &mid, "r1");
    let (_, out) = execute_one(mgr.clone(), h.clone(), c, ToolRun { id: "r1".into(), name: "file_search".into(), input: json!({"query": "lib"}), tool: Some(mgr.registry.get("file_search").unwrap()) }).await;
    assert!(out.text.contains("src/lib.rs"), "{}", out.text);
    let c = ctx(&mgr, &h, Settings::default(), &mid, "r1");
    let (_, out) = execute_one(mgr.clone(), h.clone(), c, ToolRun { id: "r1".into(), name: "todo_write".into(), input: json!({"todos": [{"id": "1", "content": "a", "status": "in_progress"}]}), tool: Some(mgr.registry.get("todo_write").unwrap()) }).await;
    assert!(out.text.contains("1 items")); assert_eq!(h.session.lock().todos.len(), 1);
}

#[tokio::test]
async fn ask_question_round_trip_and_queue() {
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let mid = seed(&h, "q1", "ask_question");
    let sid = h.id();
    let m2 = mgr.clone();
    tokio::spawn(async move { for _ in 0..100 { tokio::time::sleep(std::time::Duration::from_millis(20)).await; let q = m2.test_events.lock().iter().rev().find_map(|e| if let AgentEvent::Question { question, .. } = e { Some(question.question_id.clone()) } else { None }); if let Some(id) = q { m2.resolve_question(&id, "blue".into()).unwrap(); return; } } });
    let ans = mgr.ask_question(&sid, &mid, AgentQuestion { question_id: new_id(), question: "color?".into(), options: None, allow_free_text: true }, &CancellationToken::new()).await.unwrap();
    assert_eq!(ans, "blue");
    let parts = h.session.lock().messages.last().unwrap().parts.clone();
    assert!(parts.iter().any(|p| matches!(p, MessagePart::Question { answer: Some(a), .. } if a == "blue")));
    // queue while "running"
    h.running.store(true, std::sync::atomic::Ordering::SeqCst);
    mgr.submit(SendMessageRequest { session_id: sid.clone(), text: "later".into(), mode: "agent".into(), ..Default::default() }).await.unwrap();
    assert_eq!(h.session.lock().queued.len(), 1);
    assert!(mgr.test_events.lock().iter().any(|e| matches!(e, AgentEvent::Queue { queued, .. } if queued.len() == 1)));
    h.running.store(false, std::sync::atomic::Ordering::SeqCst);
}

#[tokio::test]
async fn context_builds_file_and_truncates_large_note() {
    use crate::ai::agent::context::{build, ContextEnv};
    let ws = temp_workspace();
    test_home();
    let settings = Settings::default();
    let store = SessionStore::default();
    let env = ContextEnv { app: None, root: Some(ws.path().to_path_buf()), cwd: ws.path().to_path_buf(), settings: &settings, ws_key: workspace_key(Some(ws.path())), store: &store, files: Arc::new(vec!["main.rs".into(), "src/lib.rs".into()]) };
    let big = "word ".repeat(40_000);
    let items = vec![
        ContextItem { id: "f".into(), kind: "file".into(), label: "main.rs".into(), path: Some("main.rs".into()), ..Default::default() },
        ContextItem { id: "n".into(), kind: "note".into(), label: "big".into(), data: Some(big), ..Default::default() },
        ContextItem { id: "s".into(), kind: "selection".into(), label: "sel".into(), path: Some("src/lib.rs".into()), range: Some(Range1 { start_line: 1, end_line: 1 }), ..Default::default() },
    ];
    let b = build(&env, &items).await;
    assert!(b.text.contains("<file path=\"main.rs\">")); assert!(b.text.contains("println"));
    assert!(b.preview.iter().find(|p| p.id == "n").unwrap().truncated);
    assert!(b.text.contains("pub fn add"));
    assert!(b.total_tokens > 0);
    let cands = crate::ai::agent::context::mention_candidates(&env, "file", "lib", 10).await;
    assert_eq!(cands[0].path.as_deref(), Some("src/lib.rs"));
}

#[tokio::test]
async fn run_turn_with_unavailable_provider_ends_in_error_state() {
    // Providers are stubs in this build: the turn must create the user message + checkpoint, then fail cleanly.
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let sid = h.id();
    mgr.submit(SendMessageRequest { session_id: sid.clone(), text: "hello agent".into(), mode: "agent".into(), model_id: Some("claude-opus-5-5".into()), ..Default::default() }).await.unwrap();
    for _ in 0..200 { tokio::time::sleep(std::time::Duration::from_millis(25)).await; if !h.is_running() { break; } }
    assert!(!h.is_running());
    let s = h.session.lock();
    assert_eq!(s.messages.len(), 2, "user + assistant messages expected");
    assert_eq!(s.messages[0].role, MessageRole::User);
    assert_eq!(s.messages[1].status, Some(MessageStatus::Error));
    assert_eq!(s.checkpoints.len(), 1);
    assert_eq!(s.transcript.len(), 1, "failed assistant turn must not be added to the transcript");
    drop(s);
    let ev = mgr.test_events.lock();
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::Checkpoint { .. })));
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::MessageEnd { status: MessageStatus::Error, .. })));
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::SessionStatus { status: SessionStatus::Idle, .. })));
}

// ───────── thread lifecycle (docs/THREADS.md) ─────────
fn text_msg(role: MessageRole, text: &str) -> ChatMessage { let mut m = ChatMessage::new(role); m.parts.push(MessagePart::Text { text: text.into() }); m }
fn ws_session(ws: &std::path::Path, title: &str) -> Session { let mut s = Session::default(); s.title = title.into(); s.workspace_root = ws.to_string_lossy().to_string(); s.cwd = s.workspace_root.clone(); s }

#[tokio::test]
async fn lifecycle_setters_emit_session_updated_and_persist() {
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let id = h.id();
    let key = workspace_key(Some(ws.path()));
    let until = now_ms() + 3_600_000;
    let sv = mgr.set_snoozed(&id, Some(until)).unwrap();
    assert_eq!(sv.snoozed_until, Some(until));
    assert!(mgr.test_events.lock().iter().any(|e| matches!(e, AgentEvent::SessionUpdated { session } if session.id == id && session.snoozed_until == Some(until))));
    assert_eq!(mgr.store.load(&key, &id).unwrap().snoozed_until, Some(until));
    // wire format: kebab-case tag, camelCase fields
    let v = serde_json::to_value(AgentEvent::SessionUpdated { session: sv }).unwrap();
    assert_eq!(v["type"], "session-updated"); assert_eq!(v["session"]["snoozedUntil"], json!(until)); assert_eq!(v["session"]["settled"], json!(false));
    assert_eq!(mgr.set_snoozed(&id, None).unwrap().snoozed_until, None, "None un-snoozes");
    assert_eq!(mgr.set_snoozed(&id, Some(now_ms() - 1)).unwrap().snoozed_until, None, "a past time un-snoozes");
    assert!(mgr.set_settled(&id, true).unwrap().settled);
    let a = mgr.set_archived(&id, true).unwrap(); assert_eq!(a.archived, Some(true)); assert!(a.archived_at.is_some());
    let p = mgr.set_pinned(&id, true).unwrap(); assert_eq!(p.pinned, Some(true)); assert!(p.pinned_at.is_some());
    let on_disk = mgr.store.load(&key, &id).unwrap();
    assert!(on_disk.settled && on_disk.archived && on_disk.pinned && on_disk.archived_at.is_some() && on_disk.pinned_at.is_some());
    let idx = mgr.store.read_index(&key); let e = idx.iter().find(|x| x.id == id).unwrap();
    assert!(e.settled); assert_eq!(e.archived, Some(true)); assert_eq!(e.pinned_at, p.pinned_at);
    assert_eq!(mgr.set_pinned(&id, false).unwrap().pinned_at, None);
    assert!(!mgr.set_settled(&id, false).unwrap().settled);
    assert_eq!(mgr.set_archived(&id, false).unwrap().archived_at, None);
    assert!(mgr.set_settled("missing-session", true).is_err());
}

#[tokio::test]
async fn auto_wake_clears_expired_snooze_on_list() {
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let id = h.id();
    let key = workspace_key(Some(ws.path()));
    { h.session.lock().snoozed_until = Some(now_ms() - 10); } mgr.persist_now(&h);
    // an on-disk-only session (not loaded) in the same workspace
    let mut other = ws_session(ws.path(), "Other"); other.snoozed_until = Some(now_ms() - 10);
    mgr.store.save_now(&key, &other).unwrap();
    let list = mgr.list_sessions(true);
    for sid in [&id, &other.id] {
        assert_eq!(list.iter().find(|x| &x.id == sid).unwrap().snoozed_until, None, "woken in the listing");
        assert_eq!(mgr.store.load(&key, sid).unwrap().snoozed_until, None, "wake persisted");
        assert!(mgr.test_events.lock().iter().any(|e| matches!(e, AgentEvent::SessionUpdated { session } if &session.id == sid && session.snoozed_until.is_none())));
    }
    assert!(h.session.lock().snoozed_until.is_none());
    let fut = now_ms() + 60_000; mgr.set_snoozed(&id, Some(fut)).unwrap();
    assert_eq!(mgr.list_sessions(true).iter().find(|x| x.id == id).unwrap().snoozed_until, Some(fut), "future snooze kept");
}

#[tokio::test]
async fn auto_settle_marks_idle_unpinned_threads() {
    let ws = temp_workspace();
    let (mgr, _h) = new_mgr(ws.path()).await;
    let key = workspace_key(Some(ws.path()));
    let old = now_ms() - 10 * 86_400_000;
    let mk = |title: &str, pinned: bool, archived: bool| { let mut s = ws_session(ws.path(), title); s.updated_at = old; s.pinned = pinned; s.archived = archived; s };
    let idle = mk("idle", false, false); let pinned = mk("pinned", true, false); let archived = mk("archived", false, true);
    let mut fresh = mk("fresh", false, false); fresh.updated_at = now_ms();
    let mut snoozed = mk("snoozed", false, false); snoozed.snoozed_until = Some(now_ms() + 60_000);
    for s in [&idle, &pinned, &archived, &fresh, &snoozed] { mgr.store.save_now(&key, s).unwrap(); }
    let mut list = mgr.store.read_index(&key);
    mgr.apply_lifecycle(&mut list, 0);
    assert!(list.iter().all(|x| !x.settled), "0 days = off");
    mgr.apply_lifecycle(&mut list, 7);
    let settled = |id: &str| list.iter().find(|x| x.id == id).unwrap().settled;
    assert!(settled(&idle.id));
    assert!(!settled(&pinned.id)); assert!(!settled(&archived.id)); assert!(!settled(&fresh.id)); assert!(!settled(&snoozed.id));
    assert!(mgr.store.load(&key, &idle.id).unwrap().settled, "persisted");
    assert_eq!(mgr.store.load(&key, &idle.id).unwrap().updated_at, old, "settling does not touch updated_at");
    assert!(mgr.test_events.lock().iter().any(|e| matches!(e, AgentEvent::SessionUpdated { session } if session.id == idle.id && session.settled)));
}

#[tokio::test]
async fn new_message_wakes_snoozed_and_settled_thread() {
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let id = h.id();
    mgr.set_snoozed(&id, Some(now_ms() + 60_000)).unwrap(); mgr.set_settled(&id, true).unwrap();
    // Mark running so submit() queues instead of starting a turn (no model configured in tests).
    h.running.store(true, std::sync::atomic::Ordering::SeqCst);
    mgr.submit(SendMessageRequest { session_id: id.clone(), text: "more".into(), mode: "agent".into(), ..Default::default() }).await.unwrap();
    let s = h.session.lock();
    assert!(s.snoozed_until.is_none() && !s.settled);
    assert_eq!(s.queued.len(), 1);
}

#[tokio::test]
async fn search_messages_scans_session_files() {
    let ws = temp_workspace();
    let store = SessionStore::default();
    let key = workspace_key(Some(ws.path()));
    let mut a = ws_session(ws.path(), "Alpha"); a.updated_at = 1_000;
    a.messages.push(text_msg(MessageRole::User, "please fix the Zebra parser")); a.messages.push(text_msg(MessageRole::Assistant, "done, the zebra parser now handles stripes"));
    let mut b = ws_session(ws.path(), "Beta"); b.updated_at = 2_000; b.archived = true; b.messages.push(text_msg(MessageRole::User, "ZEBRA again"));
    let mut c = ws_session(ws.path(), "Gamma"); c.messages.push(text_msg(MessageRole::System, "zebra in a system note"));
    for s in [&a, &b, &c] { store.save_now(&key, s).unwrap(); }
    let hits = store.search_messages(&[key.clone()], "zebra", false, 50);
    assert_eq!(hits.len(), 2, "{hits:?}");
    assert!(hits.iter().all(|h| h.session_id == a.id && h.session_title == "Alpha" && h.workspace_root == a.workspace_root && h.updated_at == 1_000));
    assert_eq!(hits[0].message_id, a.messages[0].id); assert_eq!(hits[0].role, MessageRole::User); assert!(hits[0].snippet.contains("Zebra parser"), "{}", hits[0].snippet);
    assert_eq!(hits[1].role, MessageRole::Assistant);
    let with_archived = store.search_messages(&[key.clone()], "zebra", true, 50);
    assert_eq!(with_archived.len(), 3); assert_eq!(with_archived[0].session_id, b.id, "newest session first");
    assert_eq!(store.search_messages(&[key.clone()], "zebra", true, 1).len(), 1, "capped");
    assert!(store.search_messages(&[key.clone()], "nothing-here", true, 50).is_empty());
    assert!(store.search_messages(&[key], "   ", true, 50).is_empty());
}

#[tokio::test]
async fn list_all_spans_workspace_keys() {
    let ws1 = temp_workspace(); let ws2 = temp_workspace();
    let (mgr, h1) = new_mgr(ws1.path()).await;
    let k2 = workspace_key(Some(ws2.path()));
    let s2 = ws_session(ws2.path(), "Elsewhere");
    mgr.store.save_now(&k2, &s2).unwrap();
    let keys = crate::core::sessions::all_workspace_keys();
    assert!(keys.contains(&workspace_key(Some(ws1.path()))) && keys.contains(&k2));
    let all = mgr.list_sessions_all(false);
    assert_eq!(all.iter().find(|x| x.id == h1.id()).unwrap().workspace_root, ws1.path().to_string_lossy().to_string());
    assert_eq!(all.iter().find(|x| x.id == s2.id).unwrap().workspace_root, s2.workspace_root);
    assert!(mgr.list_sessions(true).iter().all(|x| x.id != s2.id), "current-workspace listing stays scoped");
    // lifecycle ops reach sessions stored under another workspace key
    assert!(mgr.set_settled(&s2.id, true).unwrap().settled);
    assert!(mgr.store.load(&k2, &s2.id).unwrap().settled);
    // cross-workspace search
    let mut s3 = ws_session(ws2.path(), "Searchable"); s3.messages.push(text_msg(MessageRole::User, "find the quokka")); mgr.store.save_now(&k2, &s3).unwrap();
    let hits = mgr.store.search_messages(&crate::core::sessions::all_workspace_keys(), "QUOKKA", false, 10);
    assert_eq!(hits.len(), 1); assert_eq!(hits[0].workspace_root, s3.workspace_root);
}

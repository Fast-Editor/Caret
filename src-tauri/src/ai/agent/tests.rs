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

// ───────────────────────── steer + editable queue ─────────────────────────

#[test]
fn inject_steer_appends_to_trailing_user_message_and_mirrors_ui_message() {
    use crate::ai::agent::runner::inject_steer;
    use crate::ai::llm::{ContentBlock, Message as LlmMessage, Role, ToolResultContent};
    let mut s = Session::default();
    s.transcript.push(LlmMessage::user_text("do it"));
    s.transcript.push(LlmMessage { role: Role::Assistant, content: vec![ContentBlock::ToolUse { id: "t1".into(), name: "read_file".into(), input: json!({}) }] });
    s.transcript.push(LlmMessage { role: Role::User, content: vec![ContentBlock::ToolResult { tool_use_id: "t1".into(), content: vec![ToolResultContent::Text { text: "ok".into() }], is_error: false }] });
    let item = QueuedMessage { id: "q1".into(), text: "also check tests".into(), context: vec![ContextItem { id: "f".into(), kind: "file".into(), label: "main.rs".into(), path: Some("main.rs".into()), ..Default::default() }], ..Default::default() };
    let um = inject_steer(&mut s, &item, "turn1", "agent", "<file path=\"main.rs\">fn main() {}</file>\n", &[]);
    // Tool results were the trailing user message: the steer is merged behind them, no second user turn.
    assert_eq!(s.transcript.len(), 3);
    let last = s.transcript.last().unwrap();
    assert_eq!(last.role, Role::User);
    assert!(matches!(last.content[0], ContentBlock::ToolResult { .. }), "tool results stay first");
    assert!(matches!(&last.content[1], ContentBlock::Text { text } if text.starts_with("<attached_context>")));
    assert!(matches!(&last.content[2], ContentBlock::Text { text } if text == "also check tests"));
    // UI message mirrors it inside the same turn.
    assert_eq!(s.messages.len(), 1); assert_eq!(s.messages[0].id, um.id);
    assert_eq!(um.role, MessageRole::User); assert_eq!(um.turn_id.as_deref(), Some("turn1")); assert_eq!(um.mode.as_deref(), Some("agent"));
    assert!(um.parts.iter().any(|p| matches!(p, MessagePart::Context { items } if items.len() == 1)));
    assert_eq!(um.text(), "also check tests");
    // After an assistant answer a fresh user turn is started instead.
    s.transcript.push(LlmMessage { role: Role::Assistant, content: vec![ContentBlock::Text { text: "done".into() }] });
    inject_steer(&mut s, &QueuedMessage { text: "one more".into(), ..Default::default() }, "turn1", "agent", "", &[]);
    assert_eq!(s.transcript.len(), 5);
    assert_eq!(s.transcript.last().unwrap().role, Role::User);
    assert_eq!(s.transcript.last().unwrap().text(), "one more");
    assert_eq!(s.messages.len(), 2);
}

#[tokio::test]
async fn steer_while_running_buffers_in_order_and_take_drains() {
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let sid = h.id();
    h.running.store(true, std::sync::atomic::Ordering::SeqCst);
    mgr.steer(SendMessageRequest { session_id: sid.clone(), text: "first".into(), mode: "agent".into(), model_id: Some("gpt-5".into()), ..Default::default() }).await.unwrap();
    mgr.steer(SendMessageRequest { session_id: sid.clone(), text: "second".into(), mode: "agent".into(), ..Default::default() }).await.unwrap();
    assert!(h.session.lock().queued.is_empty(), "steer must not touch the queue");
    assert_eq!(h.steer.lock().len(), 2);
    assert!(mgr.test_events.lock().iter().any(|e| matches!(e, AgentEvent::Steer { pending, .. } if pending.len() == 2 && pending[0].text == "first")));
    assert!(mgr.steer_pending(&h));
    let items = mgr.take_steer(&h);
    assert_eq!(items.iter().map(|q| q.text.as_str()).collect::<Vec<_>>(), vec!["first", "second"]);
    assert_eq!(items[0].request.as_ref().and_then(|r| r.model_id.clone()).as_deref(), Some("gpt-5"), "steer keeps the full request");
    assert!(!mgr.steer_pending(&h));
    assert!(mgr.test_events.lock().iter().any(|e| matches!(e, AgentEvent::Steer { pending, .. } if pending.is_empty())), "UI told the buffer drained");
    assert!(mgr.take_steer(&h).is_empty());
    h.running.store(false, std::sync::atomic::Ordering::SeqCst);
}

/// Scripted LLM provider for headless turns: records every request, blocks the first call until the test releases
/// it, and answers `reply <n>` with no tool calls.
struct ScriptedProvider { requests: parking_lot::Mutex<Vec<crate::ai::llm::LlmRequest>>, started: tokio::sync::mpsc::UnboundedSender<usize>, gate: tokio::sync::Mutex<Option<tokio::sync::oneshot::Receiver<()>>> }
#[async_trait::async_trait]
impl crate::ai::llm::LlmProvider for ScriptedProvider {
    async fn stream(&self, req: &crate::ai::llm::LlmRequest, tx: tokio::sync::mpsc::Sender<crate::ai::llm::LlmEvent>, _cancel: CancellationToken) -> crate::error::Result<()> {
        use crate::ai::llm::{LlmEvent, StopReason};
        let n = { let mut r = self.requests.lock(); r.push(req.clone()); r.len() };
        let _ = self.started.send(n);
        if n == 1 { if let Some(rx) = self.gate.lock().await.take() { let _ = rx.await; } }
        let _ = tx.send(LlmEvent::MessageStart { model: req.model.model.clone() }).await;
        let _ = tx.send(LlmEvent::TextDelta { index: 0, text: format!("reply {n}") }).await;
        let _ = tx.send(LlmEvent::Stop { reason: StopReason::EndTurn, detail: None }).await;
        Ok(())
    }
    async fn list_models(&self) -> crate::error::Result<Vec<String>> { Ok(vec![]) }
    async fn embed(&self, _model: &str, _inputs: &[String]) -> crate::error::Result<Vec<Vec<f32>>> { Ok(vec![]) }
    fn name(&self) -> &'static str { "scripted" }
}

#[tokio::test]
async fn steer_mid_turn_is_injected_before_the_next_round_and_extends_the_turn() {
    use crate::ai::llm::Role;
    use std::time::Duration;
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let sid = h.id();
    let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
    let (gate_tx, gate_rx) = tokio::sync::oneshot::channel();
    let provider = Arc::new(ScriptedProvider { requests: Default::default(), started: started_tx, gate: tokio::sync::Mutex::new(Some(gate_rx)) });
    const MODEL: &str = "gpt-5-mini"; // a default-catalog model no other test resolves
    { let mut m = crate::ai::llm::TEST_PROVIDERS.lock(); m.get_or_insert_with(Default::default).insert(MODEL.into(), provider.clone()); }
    mgr.submit(SendMessageRequest { session_id: sid.clone(), text: "hello".into(), mode: "agent".into(), model_id: Some(MODEL.into()), ..Default::default() }).await.unwrap();
    // Round 1 is in flight (the provider is parked on the gate): steer now.
    let n = tokio::time::timeout(Duration::from_secs(15), started_rx.recv()).await.expect("provider called").unwrap();
    assert_eq!(n, 1);
    assert!(h.is_running());
    mgr.steer(SendMessageRequest { session_id: sid.clone(), text: "also rename it".into(), mode: "agent".into(), ..Default::default() }).await.unwrap();
    assert_eq!(h.steer.lock().len(), 1);
    assert!(mgr.test_events.lock().iter().any(|e| matches!(e, AgentEvent::Steer { pending, .. } if pending.len() == 1)));
    gate_tx.send(()).unwrap();
    // The model answered with no tool calls, but a steer was pending: the turn must run a second round.
    let n = tokio::time::timeout(Duration::from_secs(15), started_rx.recv()).await.expect("second round").unwrap();
    assert_eq!(n, 2);
    for _ in 0..600 { tokio::time::sleep(Duration::from_millis(25)).await; if !h.is_running() { break; } }
    assert!(!h.is_running());
    let s = h.session.lock();
    let ui: Vec<(MessageRole, String)> = s.messages.iter().map(|m| (m.role, m.text())).collect();
    assert_eq!(ui, vec![(MessageRole::User, "hello".into()), (MessageRole::Assistant, "reply 1".into()), (MessageRole::User, "also rename it".into()), (MessageRole::Assistant, "reply 2".into())]);
    assert!(s.messages.iter().all(|m| m.turn_id == s.messages[0].turn_id), "the steered message belongs to the same turn");
    let transcript: Vec<(Role, String)> = s.transcript.iter().map(|m| (m.role, m.text())).collect();
    assert_eq!(transcript, vec![(Role::User, "hello".into()), (Role::Assistant, "reply 1".into()), (Role::User, "also rename it".into()), (Role::Assistant, "reply 2".into())]);
    assert_eq!(s.checkpoints.len(), 1, "steer does not open a new checkpoint");
    drop(s);
    // Round 2 saw the steered text as the last user message.
    let reqs = provider.requests.lock();
    assert_eq!(reqs.len(), 2);
    let last = reqs[1].messages.last().unwrap();
    assert_eq!(last.role, Role::User); assert_eq!(last.text(), "also rename it");
    drop(reqs);
    assert!(h.steer.lock().is_empty());
    let ev = mgr.test_events.lock();
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::Steer { pending, .. } if pending.is_empty())));
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::MessageStart { message, .. } if message.role == MessageRole::User && message.text() == "also rename it")), "the UI receives the steered user message");
    assert_eq!(ev.iter().filter(|e| matches!(e, AgentEvent::SessionStatus { status: SessionStatus::Idle, .. })).count(), 1, "one turn, one idle transition");
}

#[test]
fn queued_message_request_persists_and_legacy_items_rebuild_from_the_session() {
    let req = SendMessageRequest { session_id: "s".into(), text: "later".into(), mode: "ask".into(), model_id: Some("gpt-5".into()), thinking: Some("on".into()), agent_id: Some("claude-code".into()), ..Default::default() };
    let q = QueuedMessage::from_request(req);
    let back: QueuedMessage = serde_json::from_str(&serde_json::to_string(&q).unwrap()).unwrap();
    let r = back.request.clone().expect("request is persisted with the item");
    assert_eq!((r.model_id.as_deref(), r.thinking.as_deref(), r.agent_id.as_deref(), r.mode.as_str()), (Some("gpt-5"), Some("on"), Some("claude-code"), "ask"));
    // Session files written before `request` existed: rebuilt from the item plus the session's current model/agent.
    let legacy: QueuedMessage = serde_json::from_str(r#"{"id":"q","text":"old","context":[],"images":[],"mode":"agent"}"#).unwrap();
    assert!(legacy.request.is_none());
    let mut s = Session::default(); s.id = "sess".into(); s.model_id = Some("claude-haiku-4-5".into()); s.agent_id = Some("codex".into());
    let r = legacy.into_request(&s);
    assert_eq!((r.session_id.as_str(), r.text.as_str(), r.mode.as_str(), r.model_id.as_deref(), r.agent_id.as_deref()), ("sess", "old", "agent", Some("claude-haiku-4-5"), Some("codex")));
    // An item without a mode falls back to the session mode.
    let no_mode: QueuedMessage = serde_json::from_str(r#"{"id":"q2","text":"x"}"#).unwrap();
    s.mode = "plan".into();
    assert_eq!(no_mode.into_request(&s).mode, "plan");
}

#[tokio::test]
async fn queue_update_and_move_edit_items_and_their_requests() {
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let sid = h.id();
    h.running.store(true, std::sync::atomic::Ordering::SeqCst);
    for t in ["a", "b", "c"] { mgr.submit(SendMessageRequest { session_id: sid.clone(), text: t.into(), mode: "agent".into(), model_id: Some("gpt-5".into()), ..Default::default() }).await.unwrap(); }
    let ids: Vec<String> = h.session.lock().queued.iter().map(|q| q.id.clone()).collect();
    let ctx = vec![ContextItem { id: "f".into(), kind: "file".into(), label: "main.rs".into(), path: Some("main.rs".into()), ..Default::default() }];
    let q = mgr.queue_update(&sid, &ids[1], "b2".into(), Some(ctx)).unwrap();
    assert_eq!(q[1].text, "b2"); assert_eq!(q[1].context.len(), 1);
    let r = q[1].request.as_ref().unwrap();
    assert_eq!(r.text, "b2"); assert_eq!(r.context.len(), 1); assert_eq!(r.model_id.as_deref(), Some("gpt-5"), "editing keeps the submitted model");
    let q = mgr.queue_move(&sid, &ids[2], 0).unwrap();
    assert_eq!(q.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), vec!["c", "a", "b2"]);
    let q = mgr.queue_move(&sid, &ids[2], 99).unwrap();
    assert_eq!(q.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), vec!["a", "b2", "c"], "index is clamped");
    assert!(mgr.queue_update(&sid, "nope", "x".into(), None).is_err());
    assert!(mgr.queue_move(&sid, "nope", 0).is_err());
    assert!(mgr.test_events.lock().iter().filter(|e| matches!(e, AgentEvent::Queue { .. })).count() >= 5);
    h.running.store(false, std::sync::atomic::Ordering::SeqCst);
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

// ───────────────────────── forks / handoff / fan-out (agent/handoff.rs) ─────────────────────────

/// Two complete turns with transcript marks: t1 = (u1, a1) at transcript 0..2, t2 = (u2, a2) at 2..4.
fn seed_two_turns(h: &SessionHandle) -> Vec<String> {
    let mut s = h.session.lock();
    let mut ids = vec![];
    for (turn, u, a) in [("t1", "u1", "a1"), ("t2", "u2", "a2")] {
        let len = s.transcript.len();
        s.transcript_marks.push((turn.into(), len));
        let mut um = ChatMessage::new(MessageRole::User); um.turn_id = Some(turn.into()); um.checkpoint_id = Some(turn.into()); um.parts.push(MessagePart::Text { text: u.into() });
        let mut am = ChatMessage::new(MessageRole::Assistant); am.turn_id = Some(turn.into()); am.parts.push(MessagePart::Text { text: a.into() }); am.status = Some(MessageStatus::Done);
        ids.push(um.id.clone()); ids.push(am.id.clone());
        s.messages.push(um); s.messages.push(am);
        s.transcript.push(crate::ai::llm::Message::user_text(u));
        s.transcript.push(crate::ai::llm::Message::assistant_text(a));
    }
    ids
}

#[tokio::test]
async fn fork_at_message_keeps_exact_messages_and_transcript_marks() {
    use crate::ai::agent::handoff::fork_session;
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let ids = seed_two_turns(&h);
    let src_id = h.id();
    // Cut after a1 (last message of t1): exact cut at t2's mark.
    let f = fork_session(&mgr, &h, Some(&ids[1]), 12_000).unwrap();
    {
        let s = f.session.lock();
        assert_eq!(s.messages.len(), 2); assert_eq!(s.messages[1].text(), "a1");
        assert_eq!(s.transcript.len(), 2); assert_eq!(s.transcript[1].text(), "a1");
        assert_eq!(s.transcript_marks, vec![("t1".to_string(), 0)]);
        assert_eq!(s.parent_session_id.as_deref(), Some(src_id.as_str()));
        assert_eq!(s.fork_of_message_id.as_deref(), Some(ids[1].as_str()));
        assert_eq!(s.title, "Fork: Test"); assert!(!s.is_subagent); assert!(s.fan_out_group.is_none());
        assert!(s.messages.iter().all(|m| m.checkpoint_id.is_none()), "checkpoint links belong to the parent");
    }
    assert!(mgr.test_events.lock().iter().any(|e| matches!(e, AgentEvent::SessionCreated { session } if session.id == f.id() && session.parent_session_id.as_deref() == Some(src_id.as_str()))));
    assert!(mgr.handle(&f.id()).is_ok(), "fork is live in the manager");
    // Cut after u2 (user message, partial turn): transcript before t2 plus the kept user text; t2 keeps its mark.
    let f2 = fork_session(&mgr, &h, Some(&ids[2]), 12_000).unwrap();
    {
        let s = f2.session.lock();
        assert_eq!(s.messages.len(), 3);
        assert_eq!(s.transcript.len(), 3); assert_eq!(s.transcript[2].role, crate::ai::llm::Role::User); assert_eq!(s.transcript[2].text(), "u2");
        assert_eq!(s.transcript_marks, vec![("t1".to_string(), 0), ("t2".to_string(), 2)]);
    }
    // No cut: a full copy.
    let f3 = fork_session(&mgr, &h, None, 12_000).unwrap();
    { let s = f3.session.lock(); assert_eq!(s.messages.len(), 4); assert_eq!(s.transcript.len(), 4); assert_eq!(s.transcript_marks.len(), 2); assert!(s.fork_of_message_id.is_none()); }
    // Marks gone (as after compaction): the transcript is rebuilt from the kept messages.
    { let mut s = h.session.lock(); s.transcript_marks.clear(); }
    let f4 = fork_session(&mgr, &h, Some(&ids[1]), 12_000).unwrap();
    { let s = f4.session.lock(); assert_eq!(s.messages.len(), 2); assert_eq!(s.transcript.len(), 2); assert_eq!(s.transcript[0].text(), "u1"); assert_eq!(s.transcript[1].text(), "a1"); assert!(s.transcript_marks.is_empty()); }
    // The source is untouched.
    { let s = h.session.lock(); assert_eq!(s.messages.len(), 4); assert_eq!(s.transcript.len(), 4); }
    assert!(fork_session(&mgr, &h, Some("missing"), 12_000).is_err());
}

#[tokio::test]
async fn handoff_acp_to_builtin_rebuilds_transcript_and_leaves_note() {
    use crate::ai::agent::handoff::handoff_session;
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    seed_two_turns(&h);
    { let mut s = h.session.lock(); s.agent_id = Some("codex".into()); s.transcript.clear(); s.transcript_marks.clear(); }
    let settings = Settings::default();
    let no_model = |_: Option<&str>| -> crate::error::Result<crate::ai::llm::ResolvedModel> { Err(crate::error::CaretError::config("no model in tests")) };
    // Running sessions refuse.
    h.running.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(handoff_session(&mgr, &h, Some("caret"), None, &settings, &no_model).await.is_err());
    h.running.store(false, std::sync::atomic::Ordering::SeqCst);
    let before = mgr.test_events.lock().len();
    let out = handoff_session(&mgr, &h, Some("caret"), None, &settings, &no_model).await.unwrap();
    assert_eq!(out.transferred, 4); assert_eq!(out.summarized, 0);
    assert_eq!(out.note, "Handed off to Caret. Transferred 4 messages.");
    {
        let s = h.session.lock();
        assert!(s.agent_id.is_none());
        assert_eq!(s.transcript.len(), 4, "u1 a1 u2 a2 rebuilt as text turns");
        assert_eq!(s.transcript[0].role, crate::ai::llm::Role::User); assert_eq!(s.transcript[3].text(), "a2");
        let note = s.messages.last().unwrap();
        assert_eq!(note.role, MessageRole::System);
        assert!(matches!(&note.parts[0], MessagePart::Text { text } if text == &out.note));
    }
    assert!(mgr.test_events.lock()[before..].iter().any(|e| matches!(e, AgentEvent::SessionReset { session_id } if *session_id == h.id())));
    // Same agent, no model: nothing to do.
    assert!(handoff_session(&mgr, &h, Some("caret"), None, &settings, &no_model).await.is_err());
}

#[tokio::test]
async fn handoff_over_budget_summarises_older_messages() {
    use crate::ai::agent::handoff::handoff_session;
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    seed_two_turns(&h);
    { let mut s = h.session.lock(); s.agent_id = Some("codex".into()); s.transcript.clear(); s.transcript_marks.clear(); }
    let mut settings = Settings::default(); settings.chat.handoff_budget_tokens = 500; // floor is 500; each seeded message costs ~5 tokens
    // Pad the second user message so the budget overflows and the oldest turn must be summarised.
    { let mut s = h.session.lock(); let big = "word ".repeat(400); if let MessagePart::Text { text } = &mut s.messages[2].parts[0] { *text = big; } }
    let no_model = |_: Option<&str>| -> crate::error::Result<crate::ai::llm::ResolvedModel> { Err(crate::error::CaretError::config("no model in tests")) };
    let out = handoff_session(&mgr, &h, Some("caret"), None, &settings, &no_model).await.unwrap();
    assert!(out.summarized >= 1 && out.transferred >= 1, "{out:?}");
    assert!(out.note.contains("summarised"), "{}", out.note);
    let s = h.session.lock();
    assert!(s.transcript[0].text().starts_with("<conversation_summary>"), "summary exchange opens the transcript");
    assert!(s.transcript[0].text().contains("- User: u1"), "digest fallback lists the compacted messages");
    assert_eq!(s.transcript[1].role, crate::ai::llm::Role::Assistant);
    assert!(s.summary.is_some());
    let note = s.messages.last().unwrap();
    assert!(note.parts.iter().any(|p| matches!(p, MessagePart::Summary { .. })), "note carries the expandable summary");
}

#[tokio::test]
async fn handoff_model_change_drops_thinking_and_sets_model() {
    use crate::ai::agent::handoff::handoff_session;
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    seed_two_turns(&h);
    { let mut s = h.session.lock(); s.model_id = Some("claude-opus-5-5".into()); s.transcript_model = Some("claude-opus-5-5".into()); s.transcript[1].content.insert(0, crate::ai::llm::ContentBlock::Thinking { text: "hmm".into(), signature: None }); }
    let settings = Settings::default();
    let resolve = |m: Option<&str>| crate::ai::llm::resolve_model(&settings, &crate::core::settings::SettingsState::load(), m);
    let out = handoff_session(&mgr, &h, None, Some("gpt-5"), &settings, &resolve).await.unwrap();
    assert_eq!(out.note, "Switched to GPT-5.");
    let s = h.session.lock();
    assert_eq!(s.model_id.as_deref(), Some("gpt-5"));
    assert_eq!(s.transcript_model.as_deref(), Some("gpt-5"));
    assert!(s.transcript.iter().all(|m| !m.content.iter().any(|b| matches!(b, crate::ai::llm::ContentBlock::Thinking { .. }))), "model-bound thinking blocks are dropped");
    assert_eq!(s.transcript.len(), 4);
}

#[tokio::test]
async fn fan_out_creates_one_session_per_model_with_a_shared_group() {
    use crate::ai::agent::handoff::{fan_out, FanOutRequest};
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    let settings = Settings::default();
    // Fresh sessions (no source history).
    let ids = fan_out(&mgr, &settings, FanOutRequest { text: "Explain the build".into(), model_ids: vec!["claude-opus-5-5".into(), "gpt-5".into(), "gpt-5".into()], ..Default::default() }).await.unwrap();
    assert_eq!(ids.len(), 2, "duplicate models collapse");
    let a = mgr.handle(&ids[0]).unwrap(); let b = mgr.handle(&ids[1]).unwrap();
    let (ga, gb) = (a.session.lock().fan_out_group.clone(), b.session.lock().fan_out_group.clone());
    assert!(ga.is_some()); assert_eq!(ga, gb);
    assert_eq!(a.session.lock().model_id.as_deref(), Some("claude-opus-5-5")); assert_eq!(b.session.lock().model_id.as_deref(), Some("gpt-5"));
    assert_eq!(a.session.lock().title, "Explain the build · Claude Opus 5.5"); assert_eq!(b.session.lock().title, "Explain the build · GPT-5");
    assert!(a.session.lock().parent_session_id.is_none());
    for id in &ids { assert!(mgr.test_events.lock().iter().any(|e| matches!(e, AgentEvent::SessionCreated { session } if session.id == *id && session.fan_out_group == ga))); }
    // Both turns were submitted (providers are stubs: they end in error quickly) and each session got its user message.
    for _ in 0..200 { tokio::time::sleep(std::time::Duration::from_millis(25)).await; if !a.is_running() && !b.is_running() { break; } }
    assert!(!a.is_running() && !b.is_running());
    assert_eq!(a.session.lock().messages[0].text(), "Explain the build"); assert_eq!(b.session.lock().messages[0].text(), "Explain the build");
    // From a session with history: forks carrying the parent id and its messages.
    seed_two_turns(&h);
    let ids2 = fan_out(&mgr, &settings, FanOutRequest { session_id: Some(h.id()), text: "and now?".into(), model_ids: vec!["claude-opus-5-5".into(), "gpt-5".into()], ..Default::default() }).await.unwrap();
    assert_eq!(ids2.len(), 2);
    let c = mgr.handle(&ids2[0]).unwrap();
    for _ in 0..200 { tokio::time::sleep(std::time::Duration::from_millis(25)).await; if !c.is_running() { break; } }
    { let s = c.session.lock(); assert_eq!(s.parent_session_id.as_deref(), Some(h.id().as_str())); assert!(s.title.starts_with("Test · ")); assert!(s.messages.len() >= 5, "4 inherited + the fan-out prompt"); assert_eq!(s.messages[4].text(), "and now?"); }
    assert!(fan_out(&mgr, &settings, FanOutRequest { text: "x".into(), model_ids: vec![], ..Default::default() }).await.is_err());
}

#[tokio::test]
async fn read_thread_tool_pages_earlier_messages() {
    let ws = temp_workspace();
    let (mgr, h) = new_mgr(ws.path()).await;
    seed_two_turns(&h);
    let mid = seed(&h, "r1", "read_thread"); // adds the in-flight turn t1b... (seed uses turn t1 which is also seeded; mark the current turn explicitly)
    { let mut s = h.session.lock(); s.current_turn = Some("t1".into()); }
    let c = ctx(&mgr, &h, Settings::default(), &mid, "r1");
    let tool = mgr.registry.get("read_thread").unwrap();
    assert!(tool.readonly());
    let (_, out) = execute_one(mgr.clone(), h.clone(), c, ToolRun { id: "r1".into(), name: "read_thread".into(), input: json!({"offset": 0, "limit": 1}), tool: Some(tool.clone()) }).await;
    assert!(!out.is_error, "{}", out.text);
    assert!(out.text.contains("has 2 earlier message(s)"), "t1 messages are the current turn and excluded: {}", out.text);
    assert!(out.text.contains("#0 user:\nu2")); assert!(!out.text.contains("a2")); assert!(out.text.contains("offset=1"));
    let c = ctx(&mgr, &h, Settings::default(), &mid, "r1");
    let (_, out) = execute_one(mgr.clone(), h.clone(), c, ToolRun { id: "r1".into(), name: "read_thread".into(), input: json!({"offset": 1}), tool: Some(tool.clone()) }).await;
    assert!(out.text.contains("#1 assistant:\na2"), "{}", out.text);
    let c = ctx(&mgr, &h, Settings::default(), &mid, "r1");
    let (_, out) = execute_one(mgr.clone(), h.clone(), c, ToolRun { id: "r1".into(), name: "read_thread".into(), input: json!({"offset": 9}), tool: Some(tool) }).await;
    assert!(out.text.contains("No messages at offset 9"), "{}", out.text);
}

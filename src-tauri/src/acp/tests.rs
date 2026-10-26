//! Integration tests: Caret's ACP client against the deterministic fake agent in `tests/fixtures/fake_acp_agent.mjs`,
//! plus an opt-in live round-trip with Claude Code (`CARET_LIVE_TEST=1`).
use crate::acp::{bridge, AcpInner};
use crate::ai::agent::manager::{AgentManager, SessionHandle};
use crate::ai::agent::session::*;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn fixture() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_acp_agent.mjs") }

fn cfg(id: &str, enabled: bool, env: Option<(&str, &str)>) -> crate::acp::AcpAgentConfig {
    crate::acp::AcpAgentConfig { id: id.into(), name: format!("Fake {id}"), command: "node".into(), args: vec![fixture().to_string_lossy().to_string()], env: env.map(|(k, v)| std::collections::BTreeMap::from([(k.to_string(), v.to_string())])), enabled, source: "user".into(), description: None, icon: None, install_hint: None, website: None }
}

/// Isolated config home (sessions/checkpoints land in temp, not `~/.caret`) and agent configs injected through
/// `AcpInner::overrides`, so the tests never depend on a settings file other tests may replace.
fn setup(name: &str) -> (Arc<AcpInner>, Arc<AgentManager>, PathBuf) {
    let home = std::env::temp_dir().join("caret-acp-tests-home");
    std::fs::create_dir_all(&home).unwrap();
    std::env::set_var("CARET_HOME", &home);
    let root = std::env::temp_dir().join(format!("caret-acp-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root); std::fs::create_dir_all(&root).unwrap();
    let mgr = Arc::new(AgentManager::new()); mgr.set_fallback_root(Some(root.clone()));
    let inner = Arc::new(AcpInner::new());
    inner.overrides.insert("fake".into(), cfg("fake", true, None));
    inner.overrides.insert("fake-auth".into(), cfg("fake-auth", true, Some(("FAKE_ACP_NEED_AUTH", "1"))));
    inner.overrides.insert("fake-off".into(), cfg("fake-off", false, None));
    let _ = json!(null);
    (inner, mgr, root)
}

async fn new_session(mgr: &Arc<AgentManager>, agent: &str) -> Arc<SessionHandle> {
    let h = mgr.create_session(None, None, Some("acp test".into()), false, None).await.unwrap();
    { let mut s = h.session.lock(); s.agent_id = Some(agent.into()); s.title_generated = false; }
    h
}
fn req(h: &SessionHandle, text: &str) -> SendMessageRequest { SendMessageRequest { session_id: h.id(), text: text.into(), mode: "agent".into(), ..Default::default() } }
fn events(mgr: &AgentManager) -> Vec<AgentEvent> { mgr.test_events.lock().clone() }
fn assistant_text(h: &SessionHandle) -> String {
    let s = h.session.lock();
    s.messages.iter().rev().find(|m| m.role == MessageRole::Assistant).map(|m| m.parts.iter().filter_map(|p| if let MessagePart::Text { text } = p { Some(text.as_str()) } else { None }).collect::<String>()).unwrap_or_default()
}
fn last_assistant(h: &SessionHandle) -> ChatMessage { h.session.lock().messages.iter().rev().find(|m| m.role == MessageRole::Assistant).cloned().unwrap() }
async fn turn(inner: &Arc<AcpInner>, mgr: &Arc<AgentManager>, h: &Arc<SessionHandle>, text: &str) -> crate::error::Result<()> {
    tokio::time::timeout(Duration::from_secs(60), bridge::run_turn(inner, mgr, None, h, req(h, text), CancellationToken::new())).await.expect("turn timed out")
}
fn auto_approve(mgr: &Arc<AgentManager>, option: ApprovalOption) -> tokio::task::JoinHandle<ApprovalRequest> {
    let mgr = mgr.clone();
    tokio::spawn(async move {
        loop {
            let found = { mgr.test_events.lock().iter().find_map(|e| if let AgentEvent::ApprovalRequest { request, .. } = e { Some(request.clone()) } else { None }) };
            if let Some(r) = found { if mgr.resolve_approval(&r.approval_id, option).is_ok() { return r; } }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hello_streams_text_thought_usage_and_title() {
    let (inner, mgr, _root) = setup("hello");
    let h = new_session(&mgr, "fake").await;
    turn(&inner, &mgr, &h, "hello").await.unwrap();
    assert_eq!(assistant_text(&h), "Hello from the fake agent.");
    let ev = events(&mgr);
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::ThinkingDelta { text, .. } if text.contains("thinking"))));
    assert_eq!(ev.iter().filter(|e| matches!(e, AgentEvent::TextDelta { .. })).count(), 2);
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::Checkpoint { .. })));
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::Usage { context_tokens: 1234, .. })));
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::AgentStatus { status, .. } if status.state == "ready" && status.version.as_deref() == Some("9.9.9") && status.modes.as_ref().map(|m| m.len()) == Some(2))));
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::MessageEnd { status: MessageStatus::Done, error: None, .. })));
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::Title { title, .. } if title == "hello")));
    let m = last_assistant(&h);
    assert_eq!(m.status, Some(MessageStatus::Done)); assert_eq!(m.model_id.as_deref(), Some("acp:fake"));
    assert!(matches!(m.parts[0], MessagePart::Thinking { .. }));
    // session reuse: second turn goes through the same ACP session
    let before = inner.links.get(&h.id()).unwrap().acp_id.clone();
    turn(&inner, &mgr, &h, "hello").await.unwrap();
    assert_eq!(inner.links.get(&h.id()).unwrap().acp_id, before);
    assert_eq!(h.session.lock().messages.len(), 4);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn edit_flows_through_permission_fs_write_and_pending_change() {
    let (inner, mgr, root) = setup("edit");
    let file = root.join("a.ts"); std::fs::write(&file, "const a = 1\n").unwrap();
    let h = new_session(&mgr, "fake").await;
    let approver = auto_approve(&mgr, ApprovalOption::Allow);
    turn(&inner, &mgr, &h, &format!("edit {}", file.display())).await.unwrap();
    let approval = approver.await.unwrap();
    assert_eq!(approval.kind, ApprovalKind::FileEdit); assert_eq!(approval.agent_id.as_deref(), Some("fake")); assert!(approval.options.contains(&ApprovalOption::AllowSession));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "const a = 1\n// edited by fake agent\n");
    let s = h.session.lock();
    assert_eq!(s.changes.len(), 1, "fs/write_text_file and the diff block must dedupe to one change");
    let c = &s.changes[0];
    assert_eq!(c.kind, ChangeKind::Modify); assert_eq!(c.path, "a.ts"); assert_eq!(c.additions, 1); assert_eq!(c.deletions, 0); assert_eq!(c.status, ChangeStatus::Pending); assert_eq!(c.before.as_deref(), Some("const a = 1\n"));
    assert_eq!(s.checkpoints.last().unwrap().files.len(), 1, "checkpoint snapshot recorded before the write");
    let m = s.messages.iter().rev().find(|m| m.role == MessageRole::Assistant).unwrap();
    let call = m.parts.iter().find_map(|p| if let MessagePart::Tool { call } = p { Some(call) } else { None }).unwrap();
    assert_eq!(call.id, "call_edit_1"); assert_eq!(call.status, ToolStatus::Done); assert_eq!(call.display_name, "Edit ".to_string() + &file.display().to_string()); assert_eq!(call.raw_kind.as_deref(), Some("edit"));
    assert_eq!(call.locations.as_ref().unwrap()[0].line, Some(1));
    assert!(call.diff.as_deref().unwrap_or("").contains("+// edited by fake agent"));
    drop(s);
    let ev = events(&mgr);
    assert_eq!(ev.iter().filter(|e| matches!(e, AgentEvent::FileChange { .. })).count(), 1);
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::ToolStart { call, .. } if call.id == "call_edit_1")));
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::ToolUpdate { call, .. } if call.id == "call_edit_1" && call.status == ToolStatus::AwaitingApproval)));
    assert!(assistant_text(&h).contains("Edited"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_edit_marks_tool_rejected_and_writes_nothing() {
    let (inner, mgr, root) = setup("reject");
    let file = root.join("b.ts"); std::fs::write(&file, "x\n").unwrap();
    let h = new_session(&mgr, "fake").await;
    let approver = auto_approve(&mgr, ApprovalOption::Deny);
    turn(&inner, &mgr, &h, &format!("edit {}", file.display())).await.unwrap();
    approver.await.unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "x\n");
    assert!(h.session.lock().changes.is_empty());
    assert!(assistant_text(&h).contains("rejected"));
    let m = last_assistant(&h);
    let call = m.parts.iter().find_map(|p| if let MessagePart::Tool { call } = p { Some(call) } else { None }).unwrap();
    assert_eq!(call.status, ToolStatus::Error, "agent reported the call as failed after rejection");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn terminal_roundtrip_surfaces_command_output_and_exit_code() {
    let (inner, mgr, _root) = setup("run");
    let h = new_session(&mgr, "fake").await;
    turn(&inner, &mgr, &h, "run echo hi-from-terminal").await.unwrap();
    assert!(assistant_text(&h).contains("Command exited 0: hi-from-terminal"));
    let m = last_assistant(&h);
    let call = m.parts.iter().find_map(|p| if let MessagePart::Tool { call } = p { Some(call) } else { None }).unwrap();
    assert_eq!(call.status, ToolStatus::Done); assert_eq!(call.command.as_deref(), Some("echo hi-from-terminal")); assert!(call.output.as_deref().unwrap_or("").contains("hi-from-terminal")); assert_eq!(call.exit_code, Some(0));
    assert!(inner.terminals.is_empty(), "terminal/release removes the process record");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plan_updates_become_todos() {
    let (inner, mgr, _root) = setup("plan");
    let h = new_session(&mgr, "fake").await;
    turn(&inner, &mgr, &h, "plan").await.unwrap();
    let todos = h.session.lock().todos.clone();
    assert_eq!(todos.len(), 3); assert_eq!(todos[0].status, "completed"); assert_eq!(todos[1].status, "in_progress"); assert_eq!(todos[2].status, "pending"); assert_eq!(todos[1].content, "Edit code");
    assert!(events(&mgr).iter().any(|e| matches!(e, AgentEvent::Todos { todos, .. } if todos.len() == 3)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_sends_session_cancel_and_ends_as_cancelled() {
    let (inner, mgr, _root) = setup("cancel");
    let h = new_session(&mgr, "fake").await;
    let cancel = CancellationToken::new(); let c2 = cancel.clone();
    tokio::spawn(async move { tokio::time::sleep(Duration::from_millis(700)).await; c2.cancel(); });
    let started = std::time::Instant::now();
    tokio::time::timeout(Duration::from_secs(30), bridge::run_turn(&inner, &mgr, None, &h, req(&h, "slow"), cancel)).await.unwrap().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(assistant_text(&h).contains("tick 0"));
    assert!(events(&mgr).iter().any(|e| matches!(e, AgentEvent::MessageEnd { status: MessageStatus::Cancelled, .. })));
    assert_eq!(last_assistant(&h).status, Some(MessageStatus::Cancelled));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refusal_ends_with_error() {
    let (inner, mgr, _root) = setup("refuse");
    let h = new_session(&mgr, "fake").await;
    turn(&inner, &mgr, &h, "refuse").await.unwrap();
    assert!(events(&mgr).iter().any(|e| matches!(e, AgentEvent::MessageEnd { status: MessageStatus::Error, error: Some(e), .. } if e.contains("refused"))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_required_surfaces_then_authenticate_unblocks() {
    let (inner, mgr, _root) = setup("auth");
    let h = new_session(&mgr, "fake-auth").await;
    let err = turn(&inner, &mgr, &h, "hello").await.unwrap_err();
    assert!(err.message.contains("needs authentication"), "{}", err.message);
    assert_eq!(inner.probes.get("fake-auth").unwrap().auth_required, Some(true));
    assert!(events(&mgr).iter().any(|e| matches!(e, AgentEvent::Error { message, .. } if message.contains("authentication"))));
    let conn = bridge::ensure_conn(&inner, &mgr, "fake-auth").await.unwrap();
    conn.authenticate("fake-login").await.unwrap();
    turn(&inner, &mgr, &h, "hello").await.unwrap();
    assert_eq!(assistant_text(&h), "Hello from the fake agent.");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabled_or_unknown_agents_fail_fast() {
    let (inner, mgr, _root) = setup("disabled");
    let h = new_session(&mgr, "fake-off").await;
    let err = turn(&inner, &mgr, &h, "hello").await.unwrap_err();
    assert!(err.message.contains("disabled"));
    let h2 = new_session(&mgr, "nope").await;
    let err = turn(&inner, &mgr, &h2, "hello").await.unwrap_err();
    assert!(err.message.contains("not found"));
    assert_eq!(h2.session.lock().status, SessionStatus::Error);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_mode_round_trips() {
    let (inner, mgr, _root) = setup("mode");
    let h = new_session(&mgr, "fake").await;
    turn(&inner, &mgr, &h, "hello").await.unwrap();
    let link = inner.links.get(&h.id()).unwrap().clone();
    let conn = inner.conns.get(&link.conn_key).unwrap().clone();
    conn.set_mode(&link.acp_id, "code").await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(link.modes.lock().as_ref().unwrap().current_mode_id, "code");
    assert_eq!(link.commands.lock().len(), 1, "available_commands_update captured");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn set_config_option_round_trips() {
    let (inner, mgr, _root) = setup("cfg");
    let h = new_session(&mgr, "fake").await;
    turn(&inner, &mgr, &h, "hello").await.unwrap();
    let link = inner.links.get(&h.id()).unwrap().clone();
    assert_eq!(link.config_options.lock().as_ref().unwrap()[0].current_value, json!("fast"), "options captured from session/new");
    let conn = inner.conns.get(&link.conn_key).unwrap().clone();
    let updated = conn.set_config_option(&link.acp_id, "model", &json!("smart")).await.unwrap().unwrap();
    assert_eq!(updated[0].current_value, json!("smart"));
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(link.config_options.lock().as_ref().unwrap()[0].current_value, json!("smart"), "config_option_update applied to the link");
    let st = inner.probes.get("fake").unwrap().clone();
    assert_eq!(st.config_options.as_ref().unwrap()[0].current_value, json!("smart"), "status carries the options");
    let err = conn.set_config_option(&link.acp_id, "model", &json!("nope")).await.err().expect("unknown value is rejected");
    assert!(format!("{err:?}").contains("Invalid params"));
}

/// Opt-in: real Claude Code via `claude-agent-acp` using the user's own login. `CARET_LIVE_TEST=1 cargo test acp::tests::live -- --nocapture`
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_claude_code_roundtrip() {
    if std::env::var("CARET_LIVE_TEST").ok().as_deref() != Some("1") { eprintln!("live test skipped (set CARET_LIVE_TEST=1)"); return; }
    let root = PathBuf::from("/tmp/caret-demo"); std::fs::create_dir_all(&root).unwrap();
    let mgr = Arc::new(AgentManager::new()); mgr.set_fallback_root(Some(root.clone()));
    let inner = Arc::new(AcpInner::new());
    let h = new_session(&mgr, "claude-code").await;
    let started = std::time::Instant::now();
    let r = tokio::time::timeout(Duration::from_secs(180), bridge::run_turn(&inner, &mgr, None, &h, req(&h, "Reply with exactly the single word: pong"), CancellationToken::new())).await.expect("live turn timed out");
    for l in inner.logs_for("claude-code") { eprintln!("{l}"); }
    r.unwrap();
    let text = assistant_text(&h);
    eprintln!("live reply ({} ms): {text:?}", started.elapsed().as_millis());
    assert!(text.to_lowercase().contains("pong"));
    let st = inner.probes.get("claude-code").unwrap().clone();
    eprintln!("agent status: {}", serde_json::to_string(&st).unwrap());
    assert_eq!(st.state, "ready"); assert!(st.modes.as_ref().map(|m| m.len() >= 2).unwrap_or(false));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn snippet_diff_blocks_are_expanded_to_whole_file_changes() {
    let (inner, mgr, root) = setup("snippet");
    let file = root.join("c.ts"); std::fs::write(&file, "line one\nline two\n").unwrap();
    let h = new_session(&mgr, "fake").await;
    turn(&inner, &mgr, &h, &format!("snippetedit {}", file.display())).await.unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "// header\nline one\nline two\n");
    let s = h.session.lock();
    assert_eq!(s.changes.len(), 1);
    let c = &s.changes[0];
    assert_eq!(c.before.as_deref(), Some("line one\nline two\n"), "before must be the whole file, not the snippet");
    assert_eq!(c.after.as_deref(), Some("// header\nline one\nline two\n"));
    assert_eq!((c.additions, c.deletions), (1, 0)); assert_eq!(c.tool_call_id.as_deref(), Some("call_snip_1"));
    let m = s.messages.iter().rev().find(|m| m.role == MessageRole::Assistant).unwrap();
    let call = m.parts.iter().find_map(|p| if let MessagePart::Tool { call } = p { Some(call) } else { None }).unwrap();
    assert!(call.diff.as_deref().unwrap_or("").contains("+// header")); assert_eq!(call.file_changes.as_ref().map(|f| f.len()), Some(1));
}

//! Headless tests: Caret's DAP client against the deterministic fake adapter (`tests/fixtures/fake_dap_adapter.mjs`).
use super::session::{gid, split_gid, DebugSession};
use super::*;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn fixture() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_dap_adapter.mjs") }
fn has_node() -> bool { crate::acp::process::resolve_command("node").is_some() }

async fn wait_for(events: &Arc<Mutex<Vec<DebugEvent>>>, pred: impl Fn(&DebugEvent) -> bool, what: &str) -> DebugEvent {
    for _ in 0..200 { if let Some(e) = events.lock().iter().find(|e| pred(e)).cloned() { return e; } tokio::time::sleep(Duration::from_millis(25)).await; }
    panic!("timed out waiting for {what}; got {:?}", events.lock().iter().map(|e| format!("{e:?}").chars().take(60).collect::<String>()).collect::<Vec<_>>());
}

#[test]
fn global_ids_round_trip() {
    for (c, l) in [(0usize, 1i64), (3, 100), (7, 16_000_000)] { assert_eq!(split_gid(gid(c, l)), (c, l)); }
    assert_eq!(split_gid(-1), (0, -1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_lifecycle_against_fake_adapter() {
    if !has_node() { eprintln!("node not found; skipping"); return; }
    let events: Arc<Mutex<Vec<DebugEvent>>> = Arc::new(Mutex::new(vec![]));
    let sink: EventSink = { let ev = events.clone(); Arc::new(move |e| ev.lock().push(e)) };
    let log: Logger = Arc::new(|l| eprintln!("{l}"));
    let bps = Arc::new(BreakpointStore::default());
    bps.set("/tmp/fake/x.js", vec![3]);
    let cfg = serde_json::json!({ "name": "fake", "type": "custom", "request": "launch", "program": "/tmp/fake/x.js" });
    let s = DebugSession::start("s1".into(), "custom", cfg, &std::env::temp_dir(), bps.clone(), sink, log, Some(("node".into(), vec![fixture().to_string_lossy().to_string()]))).await.expect("start");
    wait_for(&events, |e| matches!(e, DebugEvent::Initialized { .. }), "initialized").await;
    let stopped = wait_for(&events, |e| matches!(e, DebugEvent::Stopped { .. }), "stopped").await;
    let DebugEvent::Stopped { thread_id, reason, hit_breakpoint_ids, .. } = stopped else { unreachable!() };
    assert_eq!(reason, "breakpoint"); assert_eq!(hit_breakpoint_ids, vec![1]);
    let snapshot: Vec<String> = events.lock().iter().map(|e| format!("{e:?}").chars().take(90).collect()).collect();
    assert!(events.lock().iter().any(|e| matches!(e, DebugEvent::Output { output, .. } if output.contains("hello"))), "output forwarded; events: {snapshot:?}");
    let threads = s.threads().await.unwrap(); assert_eq!(threads.len(), 1); assert_eq!(threads[0].id, thread_id);
    let frames = s.stack_trace(thread_id).await.unwrap(); assert_eq!(frames[0].line, 3); assert_eq!(frames[0].path.as_deref(), Some("/tmp/fake/x.js"));
    let scopes = s.scopes(frames[0].id).await.unwrap(); assert_eq!(scopes[0].name, "Locals");
    let vars = s.variables(scopes[0].variables_reference).await.unwrap(); assert_eq!(vars[0].name, "x"); assert_eq!(vars[0].value, "42");
    let nested = s.variables(vars[1].variables_reference).await.unwrap(); assert_eq!(nested[0].name, "k");
    let ev = s.evaluate("x", Some(frames[0].id), "repl").await.unwrap(); assert_eq!(ev.result, "42");
    // live breakpoint edit is pushed to the adapter and echoed as verified
    let verified = s.set_breakpoints("/tmp/fake/x.js", &[3, 9]).await; assert_eq!(verified.len(), 2); assert!(verified.iter().all(|b| b.verified));
    s.continue_(thread_id).await.unwrap();
    wait_for(&events, |e| matches!(e, DebugEvent::Continued { .. }), "continued").await;
    wait_for(&events, |e| matches!(e, DebugEvent::Terminated { .. }), "terminated").await;
    assert!(events.lock().iter().any(|e| matches!(e, DebugEvent::Exited { exit_code: 0, .. })));
    assert_eq!(s.state(), "terminated");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_without_breakpoints_runs_to_completion() {
    if !has_node() { return; }
    let events: Arc<Mutex<Vec<DebugEvent>>> = Arc::new(Mutex::new(vec![]));
    let sink: EventSink = { let ev = events.clone(); Arc::new(move |e| ev.lock().push(e)) };
    let s = DebugSession::start("s2".into(), "custom", serde_json::json!({ "name": "run", "type": "custom", "request": "launch" }), &std::env::temp_dir(), Arc::new(BreakpointStore::default()), sink, Arc::new(|_| {}), Some(("node".into(), vec![fixture().to_string_lossy().to_string()]))).await.unwrap();
    wait_for(&events, |e| matches!(e, DebugEvent::Terminated { .. }), "terminated").await;
    assert!(s.is_terminated());
    assert!(!events.lock().iter().any(|e| matches!(e, DebugEvent::Stopped { .. })));
}

/// Opt-in: real js-debug from the managed install (`debug_install_adapter("js-debug")` first). `CARET_LIVE_TEST=1 cargo test --lib debug::tests::live`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn live_js_debug_hits_a_breakpoint() {
    if std::env::var("CARET_LIVE_TEST").ok().as_deref() != Some("1") { return; }
    let dir = std::env::temp_dir().join("caret-dap-live"); std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("main.js"); std::fs::write(&script, "const x = 41;\nconst y = x + 1;\nconsole.log('y is', y);\n").unwrap();
    let events: Arc<Mutex<Vec<DebugEvent>>> = Arc::new(Mutex::new(vec![]));
    let sink: EventSink = { let ev = events.clone(); Arc::new(move |e| ev.lock().push(e)) };
    let bps = Arc::new(BreakpointStore::default()); bps.set(&script.to_string_lossy(), vec![3]);
    let cfg = serde_json::json!({ "name": "live", "type": "node", "request": "launch", "program": script.to_string_lossy(), "cwd": dir.to_string_lossy(), "console": "internalConsole" });
    let s = DebugSession::start("live".into(), "js-debug", cfg, &dir, bps, sink, Arc::new(|l| eprintln!("{l}")), None).await.expect("start js-debug");
    let stopped = wait_for(&events, |e| matches!(e, DebugEvent::Stopped { .. }), "stopped").await;
    let DebugEvent::Stopped { thread_id, .. } = stopped else { unreachable!() };
    let frames = s.stack_trace(thread_id).await.unwrap(); eprintln!("frames: {:?}", frames.iter().map(|f| (f.name.clone(), f.line)).collect::<Vec<_>>());
    assert_eq!(frames[0].line, 3);
    let scopes = s.scopes(frames[0].id).await.unwrap();
    let vars = s.variables(scopes[0].variables_reference).await.unwrap();
    eprintln!("vars: {:?}", vars.iter().map(|v| format!("{}={}", v.name, v.value)).collect::<Vec<_>>());
    assert!(vars.iter().any(|v| v.name == "y" && v.value == "42"));
    s.continue_(thread_id).await.unwrap();
    wait_for(&events, |e| matches!(e, DebugEvent::Terminated { .. }), "terminated").await;
    assert!(events.lock().iter().any(|e| matches!(e, DebugEvent::Output { output, .. } if output.contains("y is 42"))));
}

//! End-to-end tests: real HTTP against the orchestrator with a headless `AgentManager`. The built-in agent talks to
//! a scripted fake LLM through the router bridge (an Anthropic-compatible loopback with an injectable provider
//! factory), so `send` + `wait` exercise the real runner, policy and approval machinery.
use super::server::{self, Server};
use super::tools::{self, Ctx};
use crate::acp::AcpInner;
use crate::ai::agent::manager::AgentManager;
use crate::ai::agent::testutil::{temp_workspace, test_home};
use crate::ai::llm::{LlmEvent, LlmProvider, LlmRequest, StopReason, UsageDelta};
use crate::ai::router::bridge::{Bridge, ModelFactory};
use crate::core::settings::{ModelConfig, ProviderConfig, ProviderKind, Settings};
use crate::error::{CaretError, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const TOKEN: &str = "orchestrator-test-token-0123456789abcdef";

// ───────────────────────── fake LLM behind the bridge ─────────────────────────

/// Replays `first` on its first call and `rest` afterwards (so a tool call can be followed by a final answer).
struct Fake { calls: Arc<AtomicUsize>, first: Vec<LlmEvent>, rest: Vec<LlmEvent>, delay: Duration }
#[async_trait::async_trait]
impl LlmProvider for Fake {
    async fn stream(&self, _req: &LlmRequest, tx: mpsc::Sender<LlmEvent>, _cancel: CancellationToken) -> Result<()> {
        if !self.delay.is_zero() { tokio::time::sleep(self.delay).await; }
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        for ev in if n == 0 { &self.first } else { &self.rest } { let _ = tx.send(ev.clone()).await; }
        Ok(())
    }
    async fn list_models(&self) -> Result<Vec<String>> { Ok(vec![]) }
    async fn embed(&self, _m: &str, _i: &[String]) -> Result<Vec<Vec<f32>>> { Ok(vec![]) }
    fn name(&self) -> &'static str { "fake" }
}

fn text_script(text: &str) -> Vec<LlmEvent> {
    vec![LlmEvent::MessageStart { model: "fake".into() }, LlmEvent::Usage(UsageDelta { input_tokens: 5, ..Default::default() }), LlmEvent::TextDelta { index: 0, text: text.into() }, LlmEvent::BlockStop { index: 0 }, LlmEvent::Usage(UsageDelta { output_tokens: 3, ..Default::default() }), LlmEvent::Stop { reason: StopReason::EndTurn, detail: None }]
}
fn tool_script(command: &str) -> Vec<LlmEvent> {
    vec![LlmEvent::MessageStart { model: "fake".into() }, LlmEvent::ToolUseStart { index: 0, id: "t1".into(), name: "run_terminal_cmd".into() }, LlmEvent::ToolInputDelta { index: 0, partial_json: json!({"command": command}).to_string() }, LlmEvent::BlockStop { index: 0 }, LlmEvent::Stop { reason: StopReason::ToolUse, detail: None }]
}
fn model(id: &str) -> ModelConfig { ModelConfig { id: id.into(), provider_id: "orch-fake".into(), model: id.into(), display_name: id.into(), supports_tools: true, enabled: true, ..Default::default() } }

fn factory() -> ModelFactory {
    let text_calls = Arc::new(AtomicUsize::new(0)); let tool_calls = Arc::new(AtomicUsize::new(0)); let slow_calls = Arc::new(AtomicUsize::new(0));
    Arc::new(move |id: &str| match id {
        "fake-text" => Ok((model(id), Arc::new(Fake { calls: text_calls.clone(), first: text_script("Hello from the fake provider"), rest: text_script("Hello again"), delay: Duration::ZERO }) as Arc<dyn LlmProvider>)),
        // `make build` is neither allow-listed nor deny-listed by default, so it needs an approval.
        "fake-tool" => Ok((model(id), Arc::new(Fake { calls: tool_calls.clone(), first: tool_script("make build"), rest: text_script("Done after the approval decision"), delay: Duration::ZERO }) as Arc<dyn LlmProvider>)),
        "fake-slow" => Ok((model(id), Arc::new(Fake { calls: slow_calls.clone(), first: text_script("Slow reply"), rest: text_script("Slow reply"), delay: Duration::from_millis(2500) }) as Arc<dyn LlmProvider>)),
        other => Err(CaretError::not_found(format!("model {other}"))),
    })
}

/// One bridge for the whole test binary. Every test's turns resolve the same provider (one port, one token), so
/// the settings file below is written exactly once; a per-test bridge would make concurrent tests overwrite each
/// other's provider URL. The bridge runs on Tauri's global runtime so it outlives any single test's runtime.
static BRIDGE: once_cell::sync::Lazy<Arc<Bridge>> = once_cell::sync::Lazy::new(|| {
    let bridge = std::thread::spawn(|| tauri::async_runtime::block_on(async { Bridge::start_with(factory()).await.unwrap() })).join().unwrap();
    install_settings(&bridge);
    bridge
});

/// Point a Caret provider at the bridge. The headless manager reads settings from `CARET_HOME/settings.json` on
/// every turn; the file is written (atomically) to the shared test home and to the ACP tests' home (those tests
/// re-point `CARET_HOME`, which is process-wide), so model resolution is stable whichever is active.
fn install_settings(bridge: &Bridge) {
    let mut s = Settings::default();
    s.providers.push(ProviderConfig { id: "orch-fake".into(), kind: ProviderKind::Anthropic, name: "Orchestrator fake".into(), base_url: Some(format!("http://127.0.0.1:{}", bridge.port)), api_key_ref: None, enabled: true, headers: Some(BTreeMap::from([("x-api-key".to_string(), bridge.token.clone())])), ..Default::default() });
    for id in ["fake-text", "fake-tool", "fake-slow"] { s.models.push(ModelConfig { id: format!("orch-{id}"), provider_id: "orch-fake".into(), model: id.into(), display_name: id.into(), supports_tools: true, enabled: true, ..Default::default() }); }
    let text = serde_json::to_string_pretty(&serde_json::to_value(&s).unwrap()).unwrap();
    for dir in [test_home(), std::env::temp_dir().join("caret-acp-tests-home")] {
        let _ = std::fs::create_dir_all(&dir);
        let tmp = dir.join(format!("settings.json.orch-{}", std::process::id()));
        std::fs::write(&tmp, &text).unwrap();
        std::fs::rename(&tmp, dir.join("settings.json")).unwrap();
    }
}

struct Rig { server: Arc<Server>, ctx: Arc<Ctx>, mgr: Arc<AgentManager>, client: reqwest::Client, _ws: tempfile::TempDir }

async fn rig() -> Rig {
    let ws = temp_workspace();
    once_cell::sync::Lazy::force(&BRIDGE);
    let mgr = Arc::new(AgentManager::new());
    mgr.set_fallback_root(Some(ws.path().to_path_buf()));
    let ctx = Ctx::new(mgr.clone(), Arc::new(AcpInner::new()), None);
    let server = Server::start(ctx.clone(), TOKEN.into(), 0).await.unwrap();
    Rig { server, ctx, mgr, client: reqwest::Client::new(), _ws: ws }
}

impl Rig {
    async fn raw(&self, body: Value) -> reqwest::Response { self.client.post(self.server.url()).bearer_auth(TOKEN).json(&body).send().await.unwrap() }
    async fn rpc(&self, method: &str, params: Value) -> Value {
        let r = self.raw(json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})).await;
        assert_eq!(r.status(), 200, "{method}");
        assert_eq!(r.headers().get("content-type").unwrap(), "application/json");
        r.json().await.unwrap()
    }
    /// `tools/call` returning the structured result (panics on `isError`).
    async fn tool(&self, name: &str, args: Value) -> Value {
        let v = self.rpc("tools/call", json!({"name": name, "arguments": args})).await;
        assert!(v["error"].is_null(), "{name}: {v}");
        assert_eq!(v["result"]["isError"], false, "{name}: {}", v["result"]["content"][0]["text"]);
        assert_eq!(v["result"]["content"][0]["type"], "text");
        v["result"]["structuredContent"].clone()
    }
    async fn tool_err(&self, name: &str, args: Value) -> String {
        let v = self.rpc("tools/call", json!({"name": name, "arguments": args})).await;
        assert_eq!(v["result"]["isError"], true, "{name} should fail: {v}");
        v["result"]["content"][0]["text"].as_str().unwrap().to_string()
    }
    fn stop(self) { self.server.shutdown(); }
}

// ───────────────────────── transport ─────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transport_handshake_auth_and_errors() {
    let rig = rig().await;
    let url = rig.server.url();
    // health is open; everything else needs the token
    let h: Value = rig.client.get(rig.server.health_url()).send().await.unwrap().json().await.unwrap();
    assert_eq!(h["ok"], true); assert_eq!(h["name"], server::HEALTH_NAME);
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
    assert_eq!(rig.client.post(&url).json(&body).send().await.unwrap().status(), 401);
    assert_eq!(rig.client.post(&url).bearer_auth("wrong").json(&body).send().await.unwrap().status(), 401);
    assert_eq!(rig.client.post(&url).header("x-api-key", TOKEN).json(&body).send().await.unwrap().status(), 200, "x-api-key is accepted too");
    // initialize echoes a supported protocol version, falls back to the latest otherwise
    let v = rig.rpc("initialize", json!({"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}})).await;
    assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(v["result"]["serverInfo"]["name"], "caret");
    assert_eq!(v["result"]["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(v["result"]["capabilities"]["tools"].is_object());
    assert!(v["result"]["instructions"].as_str().unwrap().contains("create_thread"));
    let v = rig.rpc("initialize", json!({"protocolVersion": "1999-01-01"})).await;
    assert_eq!(v["result"]["protocolVersion"], server::LATEST_PROTOCOL);
    // notifications: 202 and no body
    let r = rig.raw(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await;
    assert_eq!(r.status(), 202); assert_eq!(r.text().await.unwrap(), "");
    // ping, unknown method, tools/list
    assert_eq!(rig.rpc("ping", Value::Null).await["result"], json!({}));
    let v = rig.rpc("resources/list", Value::Null).await;
    assert_eq!(v["error"]["code"], -32601); assert_eq!(v["id"], 1);
    let v = rig.rpc("tools/list", Value::Null).await;
    let names: Vec<&str> = v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, tools::tool_names());
    assert!(v["result"]["tools"][0]["inputSchema"]["type"] == "object");
    // batch: one request + one notification -> array with one response
    let r = rig.raw(json!([{"jsonrpc": "2.0", "id": "a", "method": "ping"}, {"jsonrpc": "2.0", "method": "notifications/cancelled"}])).await;
    assert_eq!(r.status(), 200);
    let arr: Value = r.json().await.unwrap();
    assert_eq!(arr.as_array().unwrap().len(), 1); assert_eq!(arr[0]["id"], "a");
    // invalid JSON -> 400 parse error; GET -> 405; DELETE -> 200; other path -> 404
    let r = rig.client.post(&url).bearer_auth(TOKEN).header("content-type", "application/json").body("{nope").send().await.unwrap();
    assert_eq!(r.status(), 400); assert_eq!(r.json::<Value>().await.unwrap()["error"]["code"], -32700);
    let r = rig.client.get(&url).bearer_auth(TOKEN).send().await.unwrap();
    assert_eq!(r.status(), 405); assert_eq!(r.headers().get("allow").unwrap(), "POST, DELETE");
    assert_eq!(rig.client.delete(&url).bearer_auth(TOKEN).send().await.unwrap().status(), 200);
    assert_eq!(rig.client.post(format!("http://127.0.0.1:{}/v1/messages", rig.server.port)).bearer_auth(TOKEN).json(&body).send().await.unwrap().status(), 404);
    // tool errors are results with isError, not JSON-RPC errors
    let msg = rig.tool_err("read_thread", json!({"thread_id": "does-not-exist"})).await;
    assert!(msg.contains("not found"), "{msg}");
    assert!(rig.tool_err("nope", json!({})).await.contains("tool `nope`"));
    assert!(rig.tool_err("send", json!({"thread_id": "x"})).await.contains("`text` is required"));
    let v = rig.rpc("tools/call", json!({"arguments": {}})).await;
    assert_eq!(v["error"]["code"], -32602);
    rig.stop();
}

// ───────────────────────── threads ─────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thread_lifecycle_send_wait_read_delete() {
    let rig = rig().await;
    let agents = rig.tool("list_agents", json!({})).await;
    assert_eq!(agents["agents"][0]["id"], "caret");
    assert!(agents["agents"][0]["models"].as_array().unwrap().iter().any(|m| m == "orch-fake-text"));

    let t = rig.tool("create_thread", json!({"title": "Orchestrated", "model_id": "orch-fake-text"})).await;
    let id = t["thread_id"].as_str().unwrap().to_string();
    assert_eq!(t["agent"], "caret"); assert_eq!(t["model"], "orch-fake-text"); assert_eq!(t["title"], "Orchestrated");
    assert!(rig.tool_err("create_thread", json!({"model_id": "no-such-model"})).await.contains("model `no-such-model`"));
    assert!(rig.tool_err("create_thread", json!({"agent_id": "no-such-agent"})).await.contains("agent `no-such-agent`"));

    let list = rig.tool("list_threads", json!({})).await;
    let mine = list["threads"].as_array().unwrap().iter().find(|t| t["id"] == id).expect("thread listed");
    assert_eq!(mine["status"], "idle"); assert_eq!(mine["agent"], "caret"); assert_eq!(mine["model"], "orch-fake-text");

    // send + wait runs the real runner against the fake provider through the bridge
    let r = rig.tool("send", json!({"thread_id": id, "text": "Say hello", "timeout_s": 60})).await;
    assert_eq!(r["status"], "idle", "{r}");
    assert_eq!(r["timed_out"], false);
    assert_eq!(r["assistant_text"], "Hello from the fake provider");
    assert_eq!(r["messages_added"], 2);
    assert!(r["pending_approval"].is_null() && r["pending_question"].is_null());
    assert_eq!(r["files_changed"], json!([]));
    assert_eq!(r["usage"]["turn"]["inputTokens"], 5); assert_eq!(r["usage"]["turn"]["outputTokens"], 3);
    assert!(r["error"].is_null());

    // wait on an idle thread returns immediately with the same shape
    let w = rig.tool("wait", json!({"thread_id": id, "timeout_s": 5})).await;
    assert_eq!(w["status"], "idle"); assert_eq!(w["assistant_text"], "Hello from the fake provider"); assert_eq!(w["messages_added"], 1);

    let rt = rig.tool("read_thread", json!({"thread_id": id})).await;
    assert_eq!(rt["total"], 2);
    assert_eq!(rt["messages"][0]["role"], "user"); assert_eq!(rt["messages"][0]["text"], "Say hello");
    assert_eq!(rt["messages"][1]["role"], "assistant"); assert_eq!(rt["messages"][1]["text"], "Hello from the fake provider");
    assert_eq!(rt["messages"][1]["tool_calls"], json!([]));
    let page = rig.tool("read_thread", json!({"thread_id": id, "offset": 1, "limit": 1})).await;
    assert_eq!(page["messages"].as_array().unwrap().len(), 1); assert_eq!(page["messages"][0]["role"], "assistant"); assert_eq!(page["offset"], 1);

    // stop on an idle thread is a no-op; delete removes it
    assert_eq!(rig.tool("stop", json!({"thread_id": id})).await["ok"], true);
    assert_eq!(rig.tool("delete_thread", json!({"thread_id": id})).await["ok"], true);
    assert!(rig.tool_err("read_thread", json!({"thread_id": id})).await.contains("not found"));
    assert!(!rig.tool("list_threads", json!({"include_archived": true})).await["threads"].as_array().unwrap().iter().any(|t| t["id"] == id));
    rig.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn approval_surfaces_in_send_pending_and_approve_resumes() {
    let rig = rig().await;
    let id = rig.tool("create_thread", json!({"title": "Needs approval", "model_id": "orch-fake-tool"})).await["thread_id"].as_str().unwrap().to_string();
    let r = rig.tool("send", json!({"thread_id": id, "text": "build it", "timeout_s": 60})).await;
    assert_eq!(r["status"], "awaiting-approval", "{r}");
    let approval = &r["pending_approval"];
    let approval_id = approval["approval_id"].as_str().expect("approval id").to_string();
    assert_eq!(approval["thread_id"], id); assert_eq!(approval["command"], "make build"); assert_eq!(approval["kind"], "command");
    assert!(approval["title"].as_str().map(|t| !t.is_empty()).unwrap_or(false));

    // visible through `pending` (all threads and this thread) and the tracker
    let p = rig.tool("pending", json!({})).await;
    assert!(p["approvals"].as_array().unwrap().iter().any(|a| a["approval_id"] == approval_id), "{p}");
    let p1 = rig.tool("pending", json!({"thread_id": id})).await;
    assert_eq!(p1["approvals"].as_array().unwrap().len(), 1); assert_eq!(p1["questions"], json!([]));
    assert_eq!(rig.ctx.pending.approvals_for(Some(&id)).len(), 1);
    let rt = rig.tool("read_thread", json!({"thread_id": id})).await;
    assert_eq!(rt["status"], "awaiting-approval"); assert_eq!(rt["messages"][1]["tool_calls"][0]["status"], "awaiting-approval");

    // deny: nothing runs, the agent is told and finishes with its second script
    assert!(rig.tool_err("approve", json!({"approval_id": approval_id, "option": "maybe"})).await.contains("unknown option"));
    assert_eq!(rig.tool("approve", json!({"approval_id": approval_id, "option": "deny"})).await["ok"], true);
    let w = rig.tool("wait", json!({"thread_id": id, "timeout_s": 60})).await;
    assert_eq!(w["status"], "idle", "{w}");
    assert_eq!(w["assistant_text"], "Done after the approval decision");
    assert!(w["pending_approval"].is_null());
    assert!(rig.tool("pending", json!({"thread_id": id})).await["approvals"].as_array().unwrap().is_empty());
    assert!(rig.ctx.pending.approvals_for(Some(&id)).is_empty(), "tracker cleared when the session resumed");
    let rt = rig.tool("read_thread", json!({"thread_id": id})).await;
    assert_eq!(rt["messages"][1]["tool_calls"][0]["name"], "run_terminal_cmd");
    assert_eq!(rt["messages"][1]["tool_calls"][0]["status"], "rejected");
    assert!(rig.tool_err("approve", json!({"approval_id": approval_id, "option": "allow"})).await.contains("not found"), "already resolved");
    rig.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_without_wait_then_timeout_then_settle() {
    let rig = rig().await;
    let id = rig.tool("create_thread", json!({"model_id": "orch-fake-slow"})).await["thread_id"].as_str().unwrap().to_string();
    let r = rig.tool("send", json!({"thread_id": id, "text": "take your time", "wait": false})).await;
    assert_eq!(r["status"], "running");
    // the provider sleeps 2.5 s; a 1 s wait times out and says so
    let w = rig.tool("wait", json!({"thread_id": id, "timeout_s": 1})).await;
    assert_eq!(w["timed_out"], true); assert_eq!(w["status"], "running");
    let w = rig.tool("wait", json!({"thread_id": id, "timeout_s": 60})).await;
    assert_eq!(w["timed_out"], false); assert_eq!(w["status"], "idle"); assert_eq!(w["assistant_text"], "Slow reply");
    // the manager's broadcast fed the tracker and the wait loop: no pending items remain
    assert!(rig.ctx.pending.is_empty());
    assert_eq!(rig.mgr.list_sessions(false).iter().find(|s| s.id == id).map(|s| s.message_count), Some(2));
    rig.stop();
}

//! Newline-delimited JSON-RPC 2.0 transport used by ACP agents over stdio, plus the typed shapes Caret reads.
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError { pub code: i64, pub message: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub data: Option<Value> }
impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self { Self { code, message: message.into(), data: None } }
    pub fn method_not_found(m: &str) -> Self { Self::new(-32601, format!("Method not found: {m}")) }
    pub fn invalid_params(msg: impl Into<String>) -> Self { Self::new(-32602, msg) }
    pub fn internal(msg: impl Into<String>) -> Self { Self::new(-32603, msg) }
    pub const AUTH_REQUIRED: i64 = -32000;
}
impl std::fmt::Display for RpcError { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{} (code {})", self.message, self.code) } }

/// Handler for messages initiated by the agent (client-side methods and notifications).
#[async_trait::async_trait]
pub trait IncomingHandler: Send + Sync {
    async fn on_request(&self, method: &str, params: Value) -> Result<Value, RpcError>;
    async fn on_notification(&self, method: &str, params: Value);
}

pub struct Rpc {
    tx: mpsc::UnboundedSender<String>,
    pending: DashMap<u64, oneshot::Sender<Result<Value, RpcError>>>,
    next: AtomicU64,
    closed: AtomicBool,
}

impl Rpc {
    /// Start reader/writer tasks over the agent's stdio. `log` receives protocol diagnostics (never full payloads).
    pub fn start(stdin: ChildStdin, stdout: ChildStdout, handler: Arc<dyn IncomingHandler>, log: Arc<dyn Fn(String) + Send + Sync>) -> Arc<Self> {
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let rpc = Arc::new(Self { tx, pending: DashMap::new(), next: AtomicU64::new(1), closed: AtomicBool::new(false) });
        // writer
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(line) = rx.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err() { break; }
                if stdin.write_all(b"\n").await.is_err() { break; }
                if stdin.flush().await.is_err() { break; }
            }
        });
        // reader
        let r2 = rpc.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::with_capacity(1 << 20, stdout).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() { continue; }
                        let msg: Value = match serde_json::from_str(trimmed) { Ok(v) => v, Err(e) => { log(format!("[acp] non-JSON line ignored ({e}): {}", &trimmed.chars().take(160).collect::<String>())); continue; } };
                        r2.dispatch(msg, handler.clone(), log.clone()).await;
                    }
                    Ok(None) => break,
                    Err(e) => { log(format!("[acp] read error: {e}")); break; }
                }
            }
            r2.closed.store(true, Ordering::SeqCst);
            // fail all pending requests
            let ids: Vec<u64> = r2.pending.iter().map(|e| *e.key()).collect();
            for id in ids { if let Some((_, tx)) = r2.pending.remove(&id) { let _ = tx.send(Err(RpcError::new(-32001, "agent process closed the connection"))); } }
            log("[acp] connection closed".into());
        });
        rpc
    }

    /// Notifications are applied inline (in arrival order, before any later response is delivered) so streaming
    /// updates never land after the `session/prompt` result. Agent → client requests run concurrently because they
    /// may block on the user (permission prompts) while the agent keeps streaming.
    async fn dispatch(self: &Arc<Self>, msg: Value, handler: Arc<dyn IncomingHandler>, log: Arc<dyn Fn(String) + Send + Sync>) {
        let has_method = msg.get("method").and_then(|m| m.as_str()).is_some();
        let id = msg.get("id").cloned().filter(|v| !v.is_null());
        if has_method {
            let method = msg["method"].as_str().unwrap_or_default().to_string();
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            match id {
                Some(id) => {
                    let me = self.clone();
                    tokio::spawn(async move {
                        let res = handler.on_request(&method, params).await;
                        if let Err(e) = &res { log(format!("[acp] {method} → error {}", e.message)); }
                        me.respond(id, res);
                    });
                }
                None => { handler.on_notification(&method, params).await; }
            }
            return;
        }
        if let Some(id) = id.and_then(|v| v.as_u64()) {
            if let Some((_, tx)) = self.pending.remove(&id) {
                if let Some(err) = msg.get("error") {
                    let e: RpcError = serde_json::from_value(err.clone()).unwrap_or_else(|_| RpcError::new(-32603, err.to_string()));
                    let _ = tx.send(Err(e));
                } else {
                    let _ = tx.send(Ok(msg.get("result").cloned().unwrap_or(Value::Null)));
                }
            } else {
                log(format!("[acp] response for unknown id {id}"));
            }
        }
    }

    fn respond(&self, id: Value, res: Result<Value, RpcError>) {
        let body = match res { Ok(v) => json!({"jsonrpc": "2.0", "id": id, "result": v}), Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}) };
        let _ = self.tx.send(body.to_string());
    }

    pub fn is_closed(&self) -> bool { self.closed.load(Ordering::SeqCst) }

    pub async fn request(&self, method: &str, params: Value, timeout: Option<Duration>) -> Result<Value, RpcError> {
        if self.is_closed() { return Err(RpcError::new(-32001, "agent connection is closed")); }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.insert(id, tx);
        let body = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if self.tx.send(body.to_string()).is_err() { self.pending.remove(&id); return Err(RpcError::new(-32001, "agent stdin closed")); }
        let fut = async { match rx.await { Ok(r) => r, Err(_) => Err(RpcError::new(-32001, "agent connection dropped")) } };
        match timeout {
            Some(t) => match tokio::time::timeout(t, fut).await { Ok(r) => r, Err(_) => { self.pending.remove(&id); Err(RpcError::new(-32002, format!("{method} timed out after {}s", t.as_secs()))) } },
            None => fut.await,
        }
    }

    pub fn notify(&self, method: &str, params: Value) {
        let body = json!({"jsonrpc": "2.0", "method": method, "params": params});
        let _ = self.tx.send(body.to_string());
    }
}

// ───────────────────────── typed shapes (lenient: unknown fields tolerated) ─────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PromptCapabilities { pub image: bool, pub audio: bool, pub embedded_context: bool }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct McpCapabilities { pub http: bool, pub sse: bool }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentCapabilities { pub load_session: bool, pub prompt_capabilities: PromptCapabilities, pub mcp_capabilities: McpCapabilities }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentInfo { pub name: String, pub title: Option<String>, pub version: Option<String> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AuthMethod { pub id: String, pub name: String, pub description: Option<String> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InitializeResult { pub protocol_version: u32, pub agent_capabilities: AgentCapabilities, pub agent_info: Option<AgentInfo>, pub auth_methods: Vec<AuthMethod> }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionMode { pub id: String, pub name: String, pub description: Option<String> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionModeState { pub current_mode_id: String, pub available_modes: Vec<SessionMode> }
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ConfigOptionChoice { pub value: String, pub name: String, pub description: Option<String> }
/// ACP "session config option" (model / mode / thought_level / …). Agents advertise them in `session/new` and via
/// `config_option_update`; the client changes one with `session/set_config_option` and gets the full list back.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ConfigOption { pub id: String, pub name: String, pub description: Option<String>, pub category: Option<String>, #[serde(rename = "type")] pub kind: String, pub current_value: serde_json::Value, pub options: Vec<ConfigOptionChoice> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewSessionResult { pub session_id: String, pub modes: Option<SessionModeState>, pub config_options: Option<Vec<ConfigOption>> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PromptResult { pub stop_reason: String }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolLocation { pub path: String, pub line: Option<u32> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolCallUpdate {
    pub tool_call_id: String,
    pub name: Option<String>,
    pub title: Option<String>,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub content: Option<Vec<Value>>,
    pub locations: Option<Vec<ToolLocation>>,
    pub raw_input: Option<Value>,
    pub raw_output: Option<Value>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlanEntry { pub content: String, pub priority: Option<String>, pub status: Option<String> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PermissionOption { pub option_id: String, pub name: String, pub kind: String }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PermissionRequest { pub session_id: String, pub tool_call: Value, pub options: Vec<PermissionOption> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FsReadParams { pub session_id: String, pub path: String, pub line: Option<u64>, pub limit: Option<u64> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FsWriteParams { pub session_id: String, pub path: String, pub content: String }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EnvVar { pub name: String, pub value: String }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TerminalCreateParams { pub session_id: String, pub command: String, pub args: Vec<String>, pub env: Vec<EnvVar>, pub cwd: Option<String>, pub output_byte_limit: Option<u64> }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TerminalRef { pub session_id: String, pub terminal_id: String }

/// Text of a content block (`{type:"text",text}`), if any.
pub fn block_text(v: &Value) -> Option<&str> { if v.get("type").and_then(|t| t.as_str()) == Some("text") { v.get("text").and_then(|t| t.as_str()) } else { None } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_initialize_result_leniently() {
        let v = json!({"protocolVersion":1,"agentCapabilities":{"loadSession":true,"promptCapabilities":{"image":true},"unknown":1},"agentInfo":{"name":"x","version":"1.2"},"authMethods":[{"id":"claude-login","name":"Log in"}],"extra":true});
        let r: InitializeResult = serde_json::from_value(v).unwrap();
        assert_eq!(r.protocol_version, 1); assert!(r.agent_capabilities.load_session); assert!(r.agent_capabilities.prompt_capabilities.image); assert_eq!(r.auth_methods[0].id, "claude-login"); assert_eq!(r.agent_info.unwrap().version.as_deref(), Some("1.2"));
    }
    #[test]
    fn tool_call_update_partial() {
        let v = json!({"toolCallId":"c1","status":"completed","content":[{"type":"diff","path":"/a.txt","oldText":"a","newText":"b"}]});
        let u: ToolCallUpdate = serde_json::from_value(v).unwrap();
        assert_eq!(u.tool_call_id, "c1"); assert_eq!(u.status.as_deref(), Some("completed")); assert!(u.title.is_none()); assert_eq!(u.content.unwrap().len(), 1);
    }
    #[test]
    fn block_text_helper() { assert_eq!(block_text(&json!({"type":"text","text":"hi"})), Some("hi")); assert_eq!(block_text(&json!({"type":"image"})), None); }
}

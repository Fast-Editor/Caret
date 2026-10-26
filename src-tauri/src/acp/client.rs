//! One live connection to an ACP agent process (per agent per workspace): initialize handshake, session calls.
use crate::acp::protocol::*;
use crate::acp::{process, AcpAgentConfig};
use crate::error::{CaretError, Result};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Child;

pub const PROTOCOL_VERSION: u32 = 1;

pub struct AgentConn {
    pub key: String,
    pub agent_id: String,
    pub root: PathBuf,
    pub config: AcpAgentConfig,
    pub rpc: Arc<Rpc>,
    pub init: InitializeResult,
    pub authenticated: AtomicBool,
    child: Mutex<Option<Child>>,
}

pub fn conn_key(agent_id: &str, root: &Path) -> String { format!("{agent_id}|{}", root.display()) }

impl AgentConn {
    /// Spawn the agent and complete the `initialize` handshake.
    pub async fn connect(config: AcpAgentConfig, root: PathBuf, handler: Arc<dyn IncomingHandler>, log: Arc<dyn Fn(String) + Send + Sync>) -> Result<Arc<Self>> {
        let spawned = process::spawn(&config, &root, log.clone())?;
        let rpc = Rpc::start(spawned.stdin, spawned.stdout, handler, log.clone());
        let params = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "clientCapabilities": { "fs": { "readTextFile": true, "writeTextFile": true }, "terminal": true },
            "clientInfo": { "name": "caret", "title": "Caret", "version": env!("CARGO_PKG_VERSION") }
        });
        let res = match rpc.request("initialize", params, Some(Duration::from_secs(90))).await {
            Ok(v) => v,
            Err(e) => { let mut c = spawned.child; let _ = c.start_kill(); return Err(CaretError::provider(format!("{}: initialize failed: {}", config.name, e.message))); }
        };
        let init: InitializeResult = serde_json::from_value(res.clone()).map_err(|e| CaretError::provider(format!("bad initialize result: {e}")))?;
        if init.protocol_version != PROTOCOL_VERSION { log(format!("[acp] agent speaks protocol v{} (client v{PROTOCOL_VERSION}); continuing best-effort", init.protocol_version)); }
        log(format!("[acp] initialized {} v{} (loadSession={}, image={}, auth methods={})", init.agent_info.as_ref().map(|a| a.name.clone()).unwrap_or_else(|| config.name.clone()), init.agent_info.as_ref().and_then(|a| a.version.clone()).unwrap_or_else(|| "?".into()), init.agent_capabilities.load_session, init.agent_capabilities.prompt_capabilities.image, init.auth_methods.len()));
        Ok(Arc::new(Self { key: conn_key(&config.id, &root), agent_id: config.id.clone(), root, config, rpc, init, authenticated: AtomicBool::new(false), child: Mutex::new(Some(spawned.child)) }))
    }

    pub fn alive(&self) -> bool { !self.rpc.is_closed() }
    pub fn pid(&self) -> Option<u32> { self.child.lock().as_ref().and_then(|c| c.id()) }

    pub async fn authenticate(&self, method_id: &str) -> Result<()> {
        self.rpc.request("authenticate", json!({ "methodId": method_id }), Some(Duration::from_secs(600))).await.map_err(|e| CaretError::provider(format!("authenticate failed: {}", e.message)))?;
        self.authenticated.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub async fn new_session(&self, cwd: &Path, mcp_servers: Vec<Value>) -> std::result::Result<NewSessionResult, RpcError> {
        let v = self.rpc.request("session/new", json!({ "cwd": cwd.to_string_lossy(), "mcpServers": mcp_servers }), Some(Duration::from_secs(120))).await?;
        serde_json::from_value(v).map_err(|e| RpcError::internal(format!("bad session/new result: {e}")))
    }

    pub async fn load_session(&self, session_id: &str, cwd: &Path, mcp_servers: Vec<Value>) -> std::result::Result<(Option<SessionModeState>, Option<Vec<ConfigOption>>), RpcError> {
        let v = self.rpc.request("session/load", json!({ "sessionId": session_id, "cwd": cwd.to_string_lossy(), "mcpServers": mcp_servers }), Some(Duration::from_secs(120))).await?;
        Ok((v.get("modes").cloned().and_then(|m| serde_json::from_value(m).ok()), v.get("configOptions").cloned().and_then(|c| serde_json::from_value(c).ok())))
    }

    /// Runs a full prompt turn; resolves when the agent sends the `session/prompt` response (after cancel too).
    pub async fn prompt(&self, session_id: &str, blocks: Vec<Value>) -> std::result::Result<PromptResult, RpcError> {
        let v = self.rpc.request("session/prompt", json!({ "sessionId": session_id, "prompt": blocks }), None).await?;
        serde_json::from_value(v).map_err(|e| RpcError::internal(format!("bad session/prompt result: {e}")))
    }

    pub fn cancel(&self, session_id: &str) { self.rpc.notify("session/cancel", json!({ "sessionId": session_id })); }

    pub async fn set_mode(&self, session_id: &str, mode_id: &str) -> Result<()> {
        self.rpc.request("session/set_mode", json!({ "sessionId": session_id, "modeId": mode_id }), Some(Duration::from_secs(30))).await.map_err(|e| CaretError::provider(format!("set_mode failed: {}", e.message)))?;
        Ok(())
    }

    /// `session/set_config_option` (model, effort, mode …). Returns the agent's refreshed option list when it sends one.
    pub async fn set_config_option(&self, session_id: &str, config_id: &str, value: &Value) -> Result<Option<Vec<ConfigOption>>> {
        let v = self.rpc.request("session/set_config_option", json!({ "sessionId": session_id, "configId": config_id, "value": value }), Some(Duration::from_secs(30))).await.map_err(|e| CaretError::provider(format!("set_config_option failed: {}", e.message)))?;
        Ok(v.get("configOptions").cloned().and_then(|c| serde_json::from_value(c).ok()))
    }

    pub async fn shutdown(&self) {
        let pid = self.pid();
        if let Some(mut c) = self.child.lock().take() { let _ = c.start_kill(); }
        if let Some(pid) = pid { process::kill_pid(pid).await; }
    }
}

impl Drop for AgentConn { fn drop(&mut self) { if let Some(mut c) = self.child.lock().take() { let _ = c.start_kill(); } } }

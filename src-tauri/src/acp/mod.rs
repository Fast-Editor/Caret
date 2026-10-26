//! Agent Client Protocol (ACP) client: run external coding agents (Claude Code, Codex, Pi, GLM, DeepSeek Harness,
//! any ACP v1 agent) inside Caret's chat sessions. Sessions, streaming, tool calls, permissions and file changes
//! are mapped onto the same `ChatSession` / `AgentEvent` model the built-in agent uses, so the review bar,
//! proof panel, approvals and checkpoints work for every agent.
//!
//! Modules: `protocol` (newline-delimited JSON-RPC 2.0), `process` (spawn + stderr logs), `client` (one connection
//! per agent per workspace), `bridge` (ACP ↔ Caret mapping), `registry` (builtin agents + npm install into
//! `~/.caret/acp`), `commands` (Tauri commands).
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;

pub mod protocol;
pub mod process;
pub mod client;
pub mod bridge;
pub mod registry;
pub mod commands;
pub use commands::*;
#[cfg(test)] mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpAgentConfig {
    pub id: String, pub name: String, pub command: String, pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub env: Option<std::collections::BTreeMap<String, String>>,
    pub enabled: bool, pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub install_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub website: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpAuthMethod { pub id: String, pub name: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub description: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpMode { pub id: String, pub name: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub description: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AcpCapabilities { #[serde(default)] pub load_session: Option<bool>, #[serde(default)] pub prompt_images: Option<bool>, #[serde(default)] pub mcp: Option<bool> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpAgentStatus {
    pub id: String, pub name: String, pub installed: bool, pub state: String, // idle | starting | ready | error | unsupported
    #[serde(default, skip_serializing_if = "Option::is_none")] pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub protocol_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub auth_required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub auth_methods: Option<Vec<AcpAuthMethod>>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub capabilities: Option<AcpCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub modes: Option<Vec<AcpMode>>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub current_mode: Option<String>,
    /// Session config options (model, effort, mode …) for the session this status was emitted for.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub config_options: Option<Vec<protocol::ConfigOption>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpRegistryEntry { pub id: String, pub name: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub description: Option<String>, #[serde(default, skip_serializing_if = "Option::is_none")] pub install: Option<serde_json::Value>, #[serde(default, skip_serializing_if = "Option::is_none")] pub website: Option<String>, pub installed: bool }

/// Process-wide ACP state (managed by Tauri as `AcpState`).
pub struct AcpInner {
    /// Live connections keyed by `agent_id|workspace_root`.
    pub conns: dashmap::DashMap<String, Arc<client::AgentConn>>,
    /// Caret session id → link to its ACP session.
    pub links: dashmap::DashMap<String, Arc<bridge::SessionLink>>,
    /// ACP session id → Caret session id.
    pub by_acp: dashmap::DashMap<String, String>,
    /// Per-agent stderr/protocol log ring (500 lines).
    pub logs: dashmap::DashMap<String, parking_lot::Mutex<VecDeque<String>>>,
    /// Terminals created on behalf of agents (terminal/create).
    pub terminals: dashmap::DashMap<String, Arc<bridge::TermProc>>,
    /// Last probe result per agent id (version/capabilities/auth methods).
    pub probes: dashmap::DashMap<String, AcpAgentStatus>,
    /// `session/update`s that arrived before the ACP session was linked (agents send them right after `session/new`).
    pub pending_updates: dashmap::DashMap<String, Vec<serde_json::Value>>,
    /// Runtime agent configs that take precedence over settings (tests, one-off agents launched programmatically).
    pub overrides: dashmap::DashMap<String, AcpAgentConfig>,
}
impl AcpInner {
    pub fn new() -> Self { Self { conns: Default::default(), links: Default::default(), by_acp: Default::default(), logs: Default::default(), terminals: Default::default(), probes: Default::default(), pending_updates: Default::default(), overrides: Default::default() } }
    pub fn log(&self, agent_id: &str, line: impl Into<String>) {
        let entry = self.logs.entry(agent_id.to_string()).or_insert_with(|| parking_lot::Mutex::new(VecDeque::with_capacity(512)));
        let mut q = entry.lock();
        if q.len() >= 500 { q.pop_front(); }
        q.push_back(line.into());
    }
    pub fn logs_for(&self, agent_id: &str) -> Vec<String> { self.logs.get(agent_id).map(|q| q.lock().iter().cloned().collect()).unwrap_or_default() }
}
impl Default for AcpInner { fn default() -> Self { Self::new() } }

pub struct AcpState { pub inner: Arc<AcpInner> }
impl Default for AcpState { fn default() -> Self { Self { inner: Arc::new(AcpInner::new()) } } }

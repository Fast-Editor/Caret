//! Auto routing. Lynkr (the user's LLM router) runs as a **managed sidecar**; Caret's loopback **bridge** executes
//! every routed request with Caret's own providers and keys. See docs/AUTO_ROUTING.md.
//!
//! Data path: agent → AnthropicProvider (anthropic-compatible, http://127.0.0.1:<lynkr>) → Lynkr `/v1/messages`
//! (model `lynkr-auto`) → tier routing → `azure-anthropic` slot → bridge `/v1/messages` (model = Caret model id) →
//! `provider_for(real).stream()`. Lynkr's `X-Lynkr-*` headers come back as `LlmEvent::Routed`.
pub mod assign;
pub mod env;
pub mod runtime;
pub mod manager;
pub mod bridge;
pub mod commands;
pub use commands::*;

use crate::ai::llm::ResolvedModel;
use crate::core::settings::{ModelConfig, ProviderConfig, ProviderKind, Settings, SettingsState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// Synthetic model id the runner sees while a request is routed.
pub const AUTO_MODEL_ID: &str = "auto";
/// Synthetic provider id for the sidecar endpoint (never persisted in settings).
pub const LYNKR_PROVIDER_ID: &str = "__lynkr";
/// Lynkr's virtual "full content-based routing" model id.
pub const LYNKR_AUTO_MODEL: &str = "lynkr-auto";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier { Simple, Medium, Complex, Reasoning }
impl Tier {
    pub const ALL: [Tier; 4] = [Tier::Simple, Tier::Medium, Tier::Complex, Tier::Reasoning];
    pub fn key(self) -> &'static str { match self { Tier::Simple => "simple", Tier::Medium => "medium", Tier::Complex => "complex", Tier::Reasoning => "reasoning" } }
    pub fn upper(self) -> &'static str { match self { Tier::Simple => "SIMPLE", Tier::Medium => "MEDIUM", Tier::Complex => "COMPLEX", Tier::Reasoning => "REASONING" } }
    pub fn env_key(self) -> &'static str { match self { Tier::Simple => "TIER_SIMPLE", Tier::Medium => "TIER_MEDIUM", Tier::Complex => "TIER_COMPLEX", Tier::Reasoning => "TIER_REASONING" } }
    pub fn parse(s: &str) -> Option<Tier> { match s.trim().to_ascii_lowercase().as_str() { "simple" => Some(Tier::Simple), "medium" => Some(Tier::Medium), "complex" => Some(Tier::Complex), "reasoning" => Some(Tier::Reasoning), _ => None } }
}

/// One tier's chosen model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TierSlot { pub model_id: String, /// "auto" | "override"
    pub source: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub reason: Option<String> }

/// Tier → model mapping derived from the user's enabled models (+ overrides). Keys are `Tier::key()`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Assignment { pub slots: BTreeMap<String, TierSlot>, pub candidates: usize, pub warnings: Vec<String>, pub hash: String }
impl Assignment {
    pub fn get(&self, t: Tier) -> Option<&TierSlot> { self.slots.get(t.key()) }
    pub fn complete(&self) -> bool { Tier::ALL.iter().all(|t| self.slots.contains_key(t.key())) }
    /// Tier whose slot is `model_id` (lowest tier first), if any.
    pub fn tier_of(&self, model_id: &str) -> Option<Tier> { Tier::ALL.iter().copied().find(|t| self.get(*t).map(|s| s.model_id == model_id).unwrap_or(false)) }
}

/// Live state of the sidecar, broadcast as `caret://router/status`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RouterStatus {
    /// disabled | not-installed | installing | starting | healthy | degraded | error
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub bridge_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub version: Option<String>,
    pub installed: bool,
    pub native_ok: bool,
    pub assignment: Assignment,
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub pid: Option<u32>,
}
impl Default for RouterStatus {
    fn default() -> Self { Self { state: "disabled".into(), reason: None, port: None, bridge_port: None, version: None, installed: false, native_ok: false, assignment: Assignment::default(), warnings: vec![], pid: None } }
}

/// Where a healthy sidecar listens and the assignment it was configured with.
#[derive(Debug, Clone)]
pub struct Endpoint { pub base_url: String, pub api_key: String, pub assignment: Assignment }

/// Tauri-managed handle to the router manager (created in `init`).
#[derive(Default)]
pub struct RouterState { pub manager: once_cell::sync::OnceCell<Arc<manager::RouterManager>> }

pub fn manager(app: &AppHandle) -> Option<Arc<manager::RouterManager>> { app.try_state::<RouterState>().and_then(|s| s.manager.get().cloned()) }

/// Create the manager; start the sidecar in the background when routing is enabled.
pub fn init(app: &AppHandle) {
    let m = manager::RouterManager::new(app.clone());
    let _ = app.state::<RouterState>().manager.set(m.clone());
    if app.state::<SettingsState>().effective().router.enabled { m.ensure_started(); }
}

/// Stop the sidecar (app exit or routing disabled).
pub fn shutdown(app: &AppHandle) { if let Some(m) = manager(app) { m.stop(); } }

/// Synthetic model + provider that sends an Auto request through the sidecar. `None` when routing is off or the
/// sidecar is not healthy — callers then fall back to the ordinary default model.
pub fn resolve_auto(app: Option<&AppHandle>, settings: &Settings, session_id: Option<&str>) -> Option<ResolvedModel> {
    if !settings.router.enabled { return None; }
    let m = manager(app?)?;
    let ep = m.endpoint()?;
    let assigned: Vec<&ModelConfig> = ep.assignment.slots.values().filter_map(|s| settings.model(&s.model_id)).collect();
    if assigned.is_empty() { return None; }
    let model = ModelConfig {
        id: AUTO_MODEL_ID.into(), provider_id: LYNKR_PROVIDER_ID.into(), model: LYNKR_AUTO_MODEL.into(), display_name: "Auto".into(),
        context_window: assigned.iter().map(|m| m.context_window).min().unwrap_or(128_000),
        max_output: assigned.iter().map(|m| m.max_output).max().unwrap_or(16_000),
        supports_tools: true, supports_vision: assigned.iter().any(|m| m.supports_vision), supports_thinking: assigned.iter().any(|m| m.supports_thinking), supports_effort: assigned.iter().any(|m| m.supports_effort),
        enabled: true, ..Default::default()
    };
    let mut headers = BTreeMap::new();
    if let Some(s) = session_id { headers.insert("x-session-id".to_string(), s.to_string()); }
    let provider = ProviderConfig { id: LYNKR_PROVIDER_ID.into(), kind: ProviderKind::AnthropicCompatible, name: "Auto (Lynkr)".into(), base_url: Some(ep.base_url), api_key_ref: None, enabled: true, headers: Some(headers), api_version: None, auth_style: Some("x-api-key".into()), enable_fallbacks: Some(false) };
    Some(ResolvedModel { model, provider, api_key: Some(ep.api_key) })
}

/// True when `resolved` is the synthetic Auto model (so the runner reconciles the served model afterwards).
pub fn is_auto(resolved: &ResolvedModel) -> bool { resolved.model.id == AUTO_MODEL_ID && resolved.provider.id == LYNKR_PROVIDER_ID }

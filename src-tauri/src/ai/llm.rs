//! Provider-neutral LLM abstraction used by the agent runner, Tab, Cmd+K, review, etc.
//! Providers (ai/providers/*) translate these types to Anthropic Messages / OpenAI chat wire formats.
use crate::core::settings::{ModelConfig, ProviderConfig, Settings, SettingsState};
use crate::error::{CaretError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role { User, Assistant }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultContent {
    Text { text: String },
    Image { media_type: String, data: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    Image { media_type: String, data: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: Vec<ToolResultContent>, is_error: bool },
    /// Reasoning emitted by the model. `signature`/`data` are provider-opaque and must be echoed back unchanged.
    Thinking { text: String, #[serde(default, skip_serializing_if = "Option::is_none")] signature: Option<String> },
    RedactedThinking { data: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message { pub role: Role, pub content: Vec<ContentBlock> }

impl Message {
    pub fn user_text(t: impl Into<String>) -> Self { Self { role: Role::User, content: vec![ContentBlock::Text { text: t.into() }] } }
    pub fn assistant_text(t: impl Into<String>) -> Self { Self { role: Role::Assistant, content: vec![ContentBlock::Text { text: t.into() }] } }
    pub fn text(&self) -> String {
        self.content.iter().filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }).collect::<Vec<_>>().join("")
    }
}

/// System prompt segment. `cache: true` marks a prompt-cache breakpoint boundary (providers that support it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemBlock { pub text: String, #[serde(default)] pub cache: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(default)] pub strict: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort { Low, Medium, High, Xhigh, Max }
impl Effort {
    pub fn parse(s: &str) -> Self { match s { "low" => Self::Low, "medium" => Self::Medium, "xhigh" => Self::Xhigh, "max" => Self::Max, _ => Self::High } }
    pub fn as_str(&self) -> &'static str { match self { Self::Low => "low", Self::Medium => "medium", Self::High => "high", Self::Xhigh => "xhigh", Self::Max => "max" } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingConfig {
    /// Request reasoning when the model supports it.
    pub enabled: bool,
    pub effort: Effort,
    /// Ask provider to return a readable summary (Anthropic `display: summarized`).
    #[serde(default)] pub summarized: bool,
}
impl Default for ThinkingConfig { fn default() -> Self { Self { enabled: true, effort: Effort::High, summarized: true } } }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub model: ModelConfig,
    pub system: Vec<SystemBlock>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
    pub max_tokens: u32,
    #[serde(default)] pub temperature: Option<f32>,
    #[serde(default)] pub thinking: ThinkingConfig,
    #[serde(default)] pub stop: Vec<String>,
    /// Allow multiple tool calls per turn.
    #[serde(default = "default_true")] pub parallel_tool_calls: bool,
    /// Enable provider prompt caching where available.
    #[serde(default = "default_true")] pub cache: bool,
    /// Free-form tag for logs/metrics (e.g. "agent", "tab", "inline", "review").
    #[serde(default)] pub purpose: String,
}
fn default_true() -> bool { true }

impl LlmRequest {
    pub fn new(model: ModelConfig) -> Self {
        let max = model.max_output.min(64_000).max(1024);
        Self { model, system: vec![], messages: vec![], tools: vec![], max_tokens: max, temperature: None, thinking: ThinkingConfig::default(), stop: vec![], parallel_tool_calls: true, cache: true, purpose: String::new() }
    }
    pub fn system(mut self, text: impl Into<String>) -> Self { self.system.push(SystemBlock { text: text.into(), cache: true }); self }
    pub fn user(mut self, text: impl Into<String>) -> Self { self.messages.push(Message::user_text(text)); self }
    pub fn no_thinking(mut self) -> Self { self.thinking.enabled = false; self }
    pub fn max_tokens(mut self, n: u32) -> Self { self.max_tokens = n; self }
    pub fn purpose(mut self, p: &str) -> Self { self.purpose = p.to_string(); self }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason { EndTurn, ToolUse, MaxTokens, StopSequence, Refusal, PauseTurn, Other }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageDelta {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub thinking_tokens: u64,
}
impl UsageDelta {
    pub fn add(&mut self, o: &UsageDelta) { self.input_tokens += o.input_tokens; self.output_tokens += o.output_tokens; self.cache_read_tokens += o.cache_read_tokens; self.cache_write_tokens += o.cache_write_tokens; self.thinking_tokens += o.thinking_tokens; }
    pub fn cost_usd(&self, m: &ModelConfig) -> f64 {
        let i = m.input_price_per_m.unwrap_or(0.0) / 1e6; let o = m.output_price_per_m.unwrap_or(0.0) / 1e6;
        let cr = m.cache_read_price_per_m.map(|p| p / 1e6).unwrap_or(i * 0.1);
        let cw = m.cache_write_price_per_m.map(|p| p / 1e6).unwrap_or(i * 1.25);
        self.input_tokens as f64 * i + self.output_tokens as f64 * o + self.cache_read_tokens as f64 * cr + self.cache_write_tokens as f64 * cw
    }
}

/// Streaming events. Block `index` identifies concurrent content blocks within one assistant turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LlmEvent {
    MessageStart { model: String },
    TextDelta { index: u32, text: String },
    ThinkingDelta { index: u32, text: String },
    /// Opaque signature/data to persist with the thinking block for replay.
    ThinkingSignature { index: u32, signature: String },
    RedactedThinking { index: u32, data: String },
    ToolUseStart { index: u32, id: String, name: String },
    ToolInputDelta { index: u32, partial_json: String },
    BlockStop { index: u32 },
    Usage(UsageDelta),
    Stop { reason: StopReason, detail: Option<String> },
    Error { message: String, retryable: bool },
    /// Routing decision reported by the Auto router (Lynkr `X-Lynkr-*` response headers).
    Routed { model: Option<String>, tier: Option<String>, score: Option<f64>, method: Option<String> },
}

/// Which model actually served a request made through Auto routing.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RoutingInfo {
    /// Always "auto" today.
    pub via: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub requested_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub served_tier: Option<String>,
    pub served_model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub method: Option<String>,
    /// The router climbed/fell back: the served model differs from the one its headers announced.
    #[serde(default)] pub fallback: bool,
}

/// Fully-assembled assistant turn (built from events by `collect_stream`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssistantTurn {
    pub content: Vec<ContentBlock>,
    pub stop: Option<StopReason>,
    pub stop_detail: Option<String>,
    pub usage: UsageDelta,
    pub model: String,
    /// Set when the request went through Auto routing (see `LlmEvent::Routed`).
    #[serde(default)] pub routing: Option<RoutingInfo>,
}
impl AssistantTurn {
    pub fn text(&self) -> String { self.content.iter().filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }).collect() }
    pub fn tool_uses(&self) -> Vec<(String, String, Value)> {
        self.content.iter().filter_map(|b| if let ContentBlock::ToolUse { id, name, input } = b { Some((id.clone(), name.clone(), input.clone())) } else { None }).collect()
    }
}

#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stream a completion. Implementations must send `Stop` or `Error` as the final event and honour `cancel`.
    async fn stream(&self, req: &LlmRequest, tx: mpsc::Sender<LlmEvent>, cancel: CancellationToken) -> Result<()>;
    async fn list_models(&self) -> Result<Vec<String>>;
    async fn embed(&self, model: &str, inputs: &[String]) -> Result<Vec<Vec<f32>>>;
    fn name(&self) -> &'static str;
}

#[derive(Clone)]
pub struct ResolvedModel {
    pub model: ModelConfig,
    pub provider: ProviderConfig,
    pub api_key: Option<String>,
}

/// Resolve a model id (or "auto"/None → default) into model+provider+key.
pub fn resolve_model(settings: &Settings, secrets: &SettingsState, model_id: Option<&str>) -> Result<ResolvedModel> {
    let model = settings.resolve_model(model_id).cloned().ok_or_else(|| CaretError::config("No model configured. Add a provider and model in Settings → Models."))?;
    let provider = settings.provider(&model.provider_id).cloned().ok_or_else(|| CaretError::config(format!("Provider '{}' for model '{}' not found", model.provider_id, model.id)))?;
    if !provider.enabled { return Err(CaretError::config(format!("Provider '{}' is disabled", provider.name))); }
    let api_key = provider.api_key_ref.as_deref().and_then(|r| secrets.secret(r));
    Ok(ResolvedModel { model, provider, api_key })
}

/// Build a provider client for a resolved model.
pub fn provider_for(resolved: &ResolvedModel) -> Result<Arc<dyn LlmProvider>> { crate::ai::providers::build(resolved) }

/// Convenience: run a streaming request and assemble the full turn, forwarding events to `on_event`.
pub async fn run_stream(
    provider: Arc<dyn LlmProvider>,
    req: &LlmRequest,
    cancel: CancellationToken,
    mut on_event: impl FnMut(&LlmEvent) + Send,
) -> Result<AssistantTurn> {
    let (tx, mut rx) = mpsc::channel::<LlmEvent>(256);
    let p = provider.clone();
    let req2 = req.clone();
    let c2 = cancel.clone();
    let handle = tokio::spawn(async move { p.stream(&req2, tx, c2).await });
    let mut turn = AssistantTurn { model: req.model.model.clone(), ..Default::default() };
    // index → position in turn.content
    let mut slots: std::collections::HashMap<u32, usize> = Default::default();
    let mut tool_json: std::collections::HashMap<u32, String> = Default::default();
    while let Some(ev) = rx.recv().await {
        on_event(&ev);
        match &ev {
            LlmEvent::MessageStart { model } => turn.model = model.clone(),
            LlmEvent::TextDelta { index, text } => {
                let pos = *slots.entry(*index).or_insert_with(|| { turn.content.push(ContentBlock::Text { text: String::new() }); turn.content.len() - 1 });
                if let ContentBlock::Text { text: t } = &mut turn.content[pos] { t.push_str(text); }
            }
            LlmEvent::ThinkingDelta { index, text } => {
                let pos = *slots.entry(*index).or_insert_with(|| { turn.content.push(ContentBlock::Thinking { text: String::new(), signature: None }); turn.content.len() - 1 });
                if let ContentBlock::Thinking { text: t, .. } = &mut turn.content[pos] { t.push_str(text); }
            }
            LlmEvent::ThinkingSignature { index, signature } => {
                let pos = *slots.entry(*index).or_insert_with(|| { turn.content.push(ContentBlock::Thinking { text: String::new(), signature: None }); turn.content.len() - 1 });
                if let ContentBlock::Thinking { signature: s, .. } = &mut turn.content[pos] { *s = Some(signature.clone()); }
            }
            LlmEvent::RedactedThinking { index, data } => { slots.insert(*index, turn.content.len()); turn.content.push(ContentBlock::RedactedThinking { data: data.clone() }); }
            LlmEvent::ToolUseStart { index, id, name } => { slots.insert(*index, turn.content.len()); turn.content.push(ContentBlock::ToolUse { id: id.clone(), name: name.clone(), input: Value::Null }); tool_json.insert(*index, String::new()); }
            LlmEvent::ToolInputDelta { index, partial_json } => { tool_json.entry(*index).or_default().push_str(partial_json); }
            LlmEvent::BlockStop { index } => {
                if let (Some(pos), Some(json)) = (slots.get(index), tool_json.remove(index)) {
                    if let ContentBlock::ToolUse { input, .. } = &mut turn.content[*pos] {
                        *input = if json.trim().is_empty() { Value::Object(Default::default()) } else { serde_json::from_str(&json).unwrap_or_else(|_| Value::String(json.clone())) };
                    }
                }
            }
            LlmEvent::Usage(u) => turn.usage.add(u),
            LlmEvent::Stop { reason, detail } => { turn.stop = Some(*reason); turn.stop_detail = detail.clone(); }
            LlmEvent::Error { message, .. } => { let _ = handle.abort(); return Err(CaretError::provider(message.clone())); }
            LlmEvent::Routed { model, tier, score, method } => {
                turn.routing = Some(RoutingInfo { via: "auto".into(), requested_tier: tier.clone(), served_tier: None, served_model_id: model.clone().unwrap_or_default(), score: *score, method: method.clone(), fallback: false });
            }
        }
    }
    // Flush any tool blocks whose BlockStop was not sent.
    for (index, json) in tool_json { if let Some(pos) = slots.get(&index) { if let ContentBlock::ToolUse { input, .. } = &mut turn.content[*pos] { if input.is_null() { *input = serde_json::from_str(&json).unwrap_or(Value::Object(Default::default())); } } } }
    match handle.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e),
        Err(e) => return Err(CaretError::internal(format!("provider task failed: {e}"))),
    }
    if cancel.is_cancelled() { return Err(CaretError::cancelled()); }
    Ok(turn)
}

/// Convenience for single-shot text generation (no tools), returns the text.
pub async fn complete_text(resolved: &ResolvedModel, req: LlmRequest, cancel: CancellationToken) -> Result<(String, UsageDelta)> {
    let provider = provider_for(resolved)?;
    let turn = run_stream(provider, &req, cancel, |_| {}).await?;
    if turn.stop == Some(StopReason::Refusal) { return Err(CaretError::provider(format!("The model declined this request{}", turn.stop_detail.map(|d| format!(": {d}")).unwrap_or_default()))); }
    Ok((turn.text(), turn.usage))
}

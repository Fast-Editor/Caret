//! Anthropic Messages API body → `LlmRequest`. Exact inverse of `providers/anthropic.rs::build_body` for the
//! fields Caret produces; unknown keys and `metadata` are ignored. `model` is resolved by the caller (it is a
//! Caret model id, not a provider model name).
use crate::ai::llm::{ContentBlock, Effort, LlmRequest, Message, Role, SystemBlock, ThinkingConfig, ToolDef, ToolResultContent};
use crate::core::settings::ModelConfig;
use crate::error::{CaretError, Result};
use serde_json::Value;

pub struct ParsedRequest { pub req: LlmRequest, pub stream: bool }

/// Signature/data prefix rule: the bridge emits `"<caret model id>|<opaque>"` so a thinking block produced by one
/// model is never replayed to another when Lynkr climbs tiers.
/// - prefixed with this model → strip, keep the block;
/// - prefixed with another model → `None` (caller drops the block);
/// - no prefix → kept unchanged (produced before prefixing existed / by the provider directly).
pub fn strip_signature(value: &str, model_id: &str) -> Option<String> {
    match value.split_once('|') {
        Some((prefix, rest)) if prefix == model_id => Some(rest.to_string()),
        Some((prefix, _)) if looks_like_model_id(prefix) => None,
        _ => Some(value.to_string()),
    }
}
/// Our prefixes are Caret model ids: short, no whitespace, not base64-looking blobs with '+' or '/'.
fn looks_like_model_id(s: &str) -> bool { !s.is_empty() && s.len() <= 128 && !s.contains(['+', '/', '=', ' ']) }

fn s(v: &Value, k: &str) -> Option<String> { v.get(k).and_then(Value::as_str).map(String::from) }

fn parse_image(v: &Value) -> Option<(String, String)> {
    let src = v.get("source")?;
    if src.get("type").and_then(Value::as_str) != Some("base64") { return None; }
    Some((s(src, "media_type")?, s(src, "data")?))
}

fn parse_block(v: &Value, model_id: &str) -> Option<ContentBlock> {
    match v.get("type").and_then(Value::as_str)? {
        "text" => Some(ContentBlock::Text { text: s(v, "text").unwrap_or_default() }),
        "image" => parse_image(v).map(|(media_type, data)| ContentBlock::Image { media_type, data }),
        "tool_use" => Some(ContentBlock::ToolUse { id: s(v, "id").unwrap_or_default(), name: s(v, "name").unwrap_or_default(), input: v.get("input").cloned().unwrap_or(Value::Object(Default::default())) }),
        "tool_result" => {
            let content = match v.get("content") {
                Some(Value::String(t)) => vec![ToolResultContent::Text { text: t.clone() }],
                Some(Value::Array(parts)) => parts.iter().filter_map(|p| match p.get("type").and_then(Value::as_str)? {
                    "text" => Some(ToolResultContent::Text { text: s(p, "text").unwrap_or_default() }),
                    "image" => parse_image(p).map(|(media_type, data)| ToolResultContent::Image { media_type, data }),
                    _ => None,
                }).collect(),
                _ => vec![],
            };
            Some(ContentBlock::ToolResult { tool_use_id: s(v, "tool_use_id").unwrap_or_default(), content, is_error: v.get("is_error").and_then(Value::as_bool).unwrap_or(false) })
        }
        "thinking" => {
            let text = s(v, "thinking").unwrap_or_default();
            let signature = match s(v, "signature") { Some(sig) => Some(strip_signature(&sig, model_id)?), None => None };
            Some(ContentBlock::Thinking { text, signature })
        }
        "redacted_thinking" => Some(ContentBlock::RedactedThinking { data: strip_signature(&s(v, "data").unwrap_or_default(), model_id)? }),
        _ => None,
    }
}

pub fn parse_request(body: &Value, model: &ModelConfig) -> Result<ParsedRequest> {
    let obj = body.as_object().ok_or_else(|| CaretError::invalid("request body must be a JSON object"))?;
    let mut req = LlmRequest::new(model.clone());
    req.purpose = "auto-bridge".into();
    req.cache = true;

    // system: string or [{type:text,text,cache_control?}]
    req.system = match obj.get("system") {
        Some(Value::String(t)) => vec![SystemBlock { text: t.clone(), cache: false }],
        Some(Value::Array(blocks)) => blocks.iter().filter(|b| b.get("type").and_then(Value::as_str).unwrap_or("text") == "text").map(|b| SystemBlock { text: s(b, "text").unwrap_or_default(), cache: b.get("cache_control").is_some() }).collect(),
        _ => vec![],
    };

    // messages
    let msgs = obj.get("messages").and_then(Value::as_array).ok_or_else(|| CaretError::invalid("`messages` must be an array"))?;
    for m in msgs {
        let role = match m.get("role").and_then(Value::as_str) { Some("assistant") => Role::Assistant, Some("user") => Role::User, other => return Err(CaretError::invalid(format!("unsupported message role {other:?}"))) };
        let content: Vec<ContentBlock> = match m.get("content") {
            Some(Value::String(t)) => vec![ContentBlock::Text { text: t.clone() }],
            Some(Value::Array(blocks)) => blocks.iter().filter_map(|b| parse_block(b, &model.id)).collect(),
            _ => vec![],
        };
        if content.is_empty() { continue; }
        req.messages.push(Message { role, content });
    }
    if req.messages.is_empty() { return Err(CaretError::invalid("`messages` is empty")); }

    // tools / tool_choice
    if let Some(tools) = obj.get("tools").and_then(Value::as_array) {
        req.tools = tools.iter().filter_map(|t| Some(ToolDef { name: s(t, "name")?, description: s(t, "description").unwrap_or_default(), input_schema: t.get("input_schema").cloned().unwrap_or(Value::Object(Default::default())), strict: t.get("strict").and_then(Value::as_bool).unwrap_or(false) })).collect();
    }
    req.parallel_tool_calls = !obj.get("tool_choice").and_then(|tc| tc.get("disable_parallel_tool_use")).and_then(Value::as_bool).unwrap_or(false);

    // thinking / effort
    let thinking_on = matches!(obj.get("thinking").and_then(|t| t.get("type")).and_then(Value::as_str), Some("adaptive") | Some("enabled"));
    let summarized = obj.get("thinking").and_then(|t| t.get("display")).and_then(Value::as_str) == Some("summarized");
    let effort = obj.get("output_config").and_then(|o| o.get("effort")).and_then(Value::as_str).map(Effort::parse).unwrap_or(ThinkingConfig::default().effort);
    req.thinking = ThinkingConfig { enabled: thinking_on, effort, summarized };

    // sampling / limits
    if let Some(mt) = obj.get("max_tokens").and_then(Value::as_u64) { req.max_tokens = (mt as u32).min(model.max_output.max(1)).max(1); }
    req.temperature = obj.get("temperature").and_then(Value::as_f64).map(|t| t as f32);
    req.stop = obj.get("stop_sequences").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();

    Ok(ParsedRequest { req, stream: obj.get("stream").and_then(Value::as_bool).unwrap_or(false) })
}

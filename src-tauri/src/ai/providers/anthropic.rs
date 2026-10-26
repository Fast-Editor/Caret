//! Anthropic Messages API provider over raw HTTP + SSE (Rust has no official SDK).
//! Also used for Anthropic-compatible proxies (`ProviderKind::AnthropicCompatible`).
use super::http_client;
use super::sse::SseParser;
use crate::ai::llm::{ContentBlock, LlmEvent, LlmProvider, LlmRequest, ResolvedModel, Role, StopReason, ToolResultContent, UsageDelta};
use crate::error::{CaretError, Result};
use futures::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub const DEFAULT_BASE: &str = "https://api.anthropic.com";
pub const API_VERSION: &str = "2023-06-01";
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

pub struct AnthropicProvider {
    pub resolved: ResolvedModel,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(resolved: &ResolvedModel) -> Result<Self> {
        Ok(Self { resolved: resolved.clone(), client: http_client()? })
    }

    fn base(&self) -> String {
        let b = self.resolved.provider.base_url.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| DEFAULT_BASE.into());
        b.trim_end_matches('/').to_string()
    }
    fn url(&self, path: &str) -> String {
        let base = self.base();
        if base.ends_with("/v1") { format!("{base}/{path}") } else { format!("{base}/v1/{path}") }
    }
    fn is_first_party(&self) -> bool {
        self.resolved.provider.base_url.as_deref().map(|b| b.trim().is_empty() || b.contains("api.anthropic.com")).unwrap_or(true)
    }

    fn apply_auth(&self, rb: reqwest::RequestBuilder, extra_beta: Option<&str>) -> reqwest::RequestBuilder {
        let mut rb = rb.header("anthropic-version", API_VERSION).header("content-type", "application/json");
        if let Some(key) = &self.resolved.api_key {
            rb = match self.resolved.provider.auth_style.as_deref() {
                Some("bearer") => rb.header("authorization", format!("Bearer {key}")),
                _ => rb.header("x-api-key", key.clone()),
            };
        }
        if let Some(h) = &self.resolved.provider.headers { for (k, v) in h { rb = rb.header(k.as_str(), v.as_str()); } }
        if let Some(b) = extra_beta { rb = rb.header("anthropic-beta", b); }
        rb
    }

    /// Whether to request server-side refusal fallbacks (first-party API, eligible models only).
    pub fn wants_fallbacks(&self) -> bool {
        if self.resolved.provider.enable_fallbacks == Some(false) || !self.is_first_party() { return false; }
        let m = &self.resolved.model.model;
        m.starts_with("claude-opus-5-5") || m.starts_with("claude-sonnet-5-5") || m.starts_with("claude-fable-5-1")
    }
}

// ───────────────────────── request body ─────────────────────────

/// Models on which thinking cannot be disabled (omit `thinking`, lower effort instead).
pub fn thinking_cannot_disable(model: &str) -> bool {
    model.contains("opus-5-5") || model.contains("sonnet-5-5") || model.contains("fable") || model.contains("mythos")
}
/// Models that accept `thinking: {type: "adaptive"}` (4.6+ family). Older models would need
/// `budget_tokens`, which Caret never sends — thinking is omitted for them.
pub fn supports_adaptive(model: &str) -> bool {
    let m = model;
    if m.contains("fable") || m.contains("mythos") { return true; }
    if m.contains("opus-5") || m.contains("sonnet-5") || m.contains("haiku-5") { return true; }
    for v in ["4-6", "4-7", "4-8", "4-9", "4.6", "4.7", "4.8", "4.9"] { if m.contains(&format!("opus-{v}")) || m.contains(&format!("sonnet-{v}")) || m.contains(&format!("haiku-{v}")) { return true; } }
    false
}

fn content_to_json(block: &ContentBlock) -> Option<Value> {
    Some(match block {
        ContentBlock::Text { text } => { if text.is_empty() { return None; } json!({"type": "text", "text": text}) }
        ContentBlock::Image { media_type, data } => json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data}}),
        ContentBlock::ToolUse { id, name, input } => json!({"type": "tool_use", "id": id, "name": name, "input": if input.is_null() { json!({}) } else { input.clone() }}),
        ContentBlock::ToolResult { tool_use_id, content, is_error } => {
            let parts: Vec<Value> = content.iter().map(|c| match c {
                ToolResultContent::Text { text } => json!({"type": "text", "text": text}),
                ToolResultContent::Image { media_type, data } => json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data}}),
            }).collect();
            let mut v = json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": parts});
            if *is_error { v["is_error"] = json!(true); }
            v
        }
        ContentBlock::Thinking { text, signature } => {
            // Thinking blocks are only replayable with their signature.
            let sig = signature.as_ref()?;
            json!({"type": "thinking", "thinking": text, "signature": sig})
        }
        ContentBlock::RedactedThinking { data } => json!({"type": "redacted_thinking", "data": data}),
    })
}

/// Build the Messages API body. Pure function (unit-tested). `fallbacks` adds `fallbacks: "default"`.
pub fn build_body(req: &LlmRequest, fallbacks: bool) -> Value {
    let model = &req.model;
    let mut breakpoints = 0usize;
    let max_breakpoints = 4usize;

    // system
    let mut system: Vec<Value> = Vec::new();
    let cache_marked: Vec<usize> = req.system.iter().enumerate().filter(|(_, b)| b.cache && !b.text.is_empty()).map(|(i, _)| i).collect();
    // Keep at most 3 system breakpoints (the last marked ones) so one remains for the conversation tail.
    let keep: std::collections::HashSet<usize> = cache_marked.iter().rev().take(3).copied().collect();
    for (i, b) in req.system.iter().enumerate() {
        if b.text.is_empty() { continue; }
        let mut v = json!({"type": "text", "text": b.text});
        if req.cache && keep.contains(&i) && breakpoints < max_breakpoints { v["cache_control"] = json!({"type": "ephemeral"}); breakpoints += 1; }
        system.push(v);
    }

    // messages
    let mut messages: Vec<Value> = Vec::new();
    for m in &req.messages {
        let content: Vec<Value> = m.content.iter().filter_map(content_to_json).collect();
        if content.is_empty() { continue; }
        messages.push(json!({"role": match m.role { Role::User => "user", Role::Assistant => "assistant" }, "content": content}));
    }
    // Conversation-tail breakpoint: last user message's last block.
    if req.cache && breakpoints < max_breakpoints {
        if let Some(last) = messages.iter_mut().rev().find(|m| m["role"] == "user") {
            if let Some(arr) = last["content"].as_array_mut() { if let Some(b) = arr.last_mut() { b["cache_control"] = json!({"type": "ephemeral"}); } }
        }
    }

    let mut body = json!({
        "model": model.model,
        "max_tokens": req.max_tokens.min(model.max_output.max(1)),
        "messages": messages,
        "stream": true,
    });
    if !system.is_empty() { body["system"] = Value::Array(system); }
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(req.tools.iter().map(|t| {
            let mut v = json!({"name": t.name, "description": t.description, "input_schema": t.input_schema});
            if t.strict { v["strict"] = json!(true); }
            v
        }).collect());
        body["tool_choice"] = json!({"type": "auto", "disable_parallel_tool_use": !req.parallel_tool_calls});
    }
    if !req.stop.is_empty() { body["stop_sequences"] = json!(req.stop); }

    // thinking / effort / sampling
    let want_thinking = req.thinking.enabled && model.supports_thinking && supports_adaptive(&model.model);
    let mut effort = req.thinking.effort;
    let mut thinking_sent = false;
    if want_thinking {
        let mut t = json!({"type": "adaptive"});
        if req.thinking.summarized { t["display"] = json!("summarized"); }
        body["thinking"] = t;
        thinking_sent = true;
    } else if thinking_cannot_disable(&model.model) {
        // Cannot turn thinking off: omit the parameter and keep spend low.
        effort = crate::ai::llm::Effort::Low;
        thinking_sent = true; // sampling params are rejected on these models
    }
    if model.supports_effort && (want_thinking || thinking_cannot_disable(&model.model)) {
        body["output_config"] = json!({"effort": effort.as_str()});
    }
    if !thinking_sent { if let Some(t) = req.temperature { body["temperature"] = json!((t as f64 * 1000.0).round() / 1000.0); } }
    if fallbacks { body["fallbacks"] = json!("default"); }
    body
}

// ───────────────────────── streaming ─────────────────────────

fn map_stop(reason: &str) -> StopReason {
    match reason { "end_turn" => StopReason::EndTurn, "tool_use" => StopReason::ToolUse, "max_tokens" => StopReason::MaxTokens, "stop_sequence" => StopReason::StopSequence, "refusal" => StopReason::Refusal, "pause_turn" => StopReason::PauseTurn, _ => StopReason::Other }
}

fn usage_from(v: &Value) -> UsageDelta {
    UsageDelta {
        input_tokens: v.get("input_tokens").and_then(Value::as_u64).unwrap_or(0),
        output_tokens: v.get("output_tokens").and_then(Value::as_u64).unwrap_or(0),
        cache_read_tokens: v.get("cache_read_input_tokens").and_then(Value::as_u64).unwrap_or(0),
        cache_write_tokens: v.get("cache_creation_input_tokens").and_then(Value::as_u64).unwrap_or(0),
        thinking_tokens: 0,
    }
}

/// Translate one Anthropic SSE event into zero or more LlmEvents. Pure (unit-tested).
pub fn translate_event(event: Option<&str>, data: &str, sent_input_usage: &mut bool) -> Vec<LlmEvent> {
    let Ok(v) = serde_json::from_str::<Value>(data) else { return vec![] };
    let typ = event.map(str::to_string).or_else(|| v.get("type").and_then(Value::as_str).map(String::from)).unwrap_or_default();
    let idx = |v: &Value| v.get("index").and_then(Value::as_u64).unwrap_or(0) as u32;
    match typ.as_str() {
        "message_start" => {
            let msg = &v["message"];
            let mut out = vec![LlmEvent::MessageStart { model: msg.get("model").and_then(Value::as_str).unwrap_or("").to_string() }];
            if let Some(u) = msg.get("usage") { let mut u = usage_from(u); u.output_tokens = 0; out.push(LlmEvent::Usage(u)); *sent_input_usage = true; }
            out
        }
        "content_block_start" => {
            let i = idx(&v); let cb = &v["content_block"];
            match cb.get("type").and_then(Value::as_str).unwrap_or("") {
                "tool_use" | "server_tool_use" => vec![LlmEvent::ToolUseStart { index: i, id: cb["id"].as_str().unwrap_or("").into(), name: cb["name"].as_str().unwrap_or("").into() }],
                "redacted_thinking" => vec![LlmEvent::RedactedThinking { index: i, data: cb["data"].as_str().unwrap_or("").into() }],
                "text" => { let t = cb["text"].as_str().unwrap_or(""); if t.is_empty() { vec![] } else { vec![LlmEvent::TextDelta { index: i, text: t.into() }] } }
                "thinking" => { let t = cb["thinking"].as_str().unwrap_or(""); if t.is_empty() { vec![] } else { vec![LlmEvent::ThinkingDelta { index: i, text: t.into() }] } }
                _ => vec![],
            }
        }
        "content_block_delta" => {
            let i = idx(&v); let d = &v["delta"];
            match d.get("type").and_then(Value::as_str).unwrap_or("") {
                "text_delta" => vec![LlmEvent::TextDelta { index: i, text: d["text"].as_str().unwrap_or("").into() }],
                "thinking_delta" => vec![LlmEvent::ThinkingDelta { index: i, text: d["thinking"].as_str().unwrap_or("").into() }],
                "signature_delta" => vec![LlmEvent::ThinkingSignature { index: i, signature: d["signature"].as_str().unwrap_or("").into() }],
                "input_json_delta" => vec![LlmEvent::ToolInputDelta { index: i, partial_json: d["partial_json"].as_str().unwrap_or("").into() }],
                _ => vec![],
            }
        }
        "content_block_stop" => vec![LlmEvent::BlockStop { index: idx(&v) }],
        "message_delta" => {
            let mut out = vec![];
            if let Some(u) = v.get("usage") {
                let mut ud = usage_from(u);
                // input/cache tokens were already reported at message_start; avoid double counting.
                if *sent_input_usage { ud.input_tokens = 0; ud.cache_read_tokens = 0; ud.cache_write_tokens = 0; }
                out.push(LlmEvent::Usage(ud));
            }
            let d = &v["delta"];
            if let Some(r) = d.get("stop_reason").and_then(Value::as_str) {
                let sd = d.get("stop_details").or_else(|| v.get("stop_details"));
                let detail = sd.and_then(|s| {
                    let cat = s.get("category").and_then(Value::as_str);
                    let exp = s.get("explanation").and_then(Value::as_str);
                    match (cat, exp) { (Some(c), Some(e)) => Some(format!("{c}: {e}")), (None, Some(e)) => Some(e.to_string()), (Some(c), None) => Some(c.to_string()), _ => None }
                });
                out.push(LlmEvent::Stop { reason: map_stop(r), detail });
            }
            out
        }
        "error" => vec![LlmEvent::Error { message: v["error"]["message"].as_str().unwrap_or("stream error").to_string(), retryable: v["error"]["type"].as_str() == Some("overloaded_error") }],
        _ => vec![], // ping, message_stop
    }
}

/// Classify an HTTP error response into a CaretError (with retryability hint).
pub fn classify_http_error(status: u16, body: &str) -> (CaretError, bool) {
    let msg = serde_json::from_str::<Value>(body).ok().and_then(|v| v["error"]["message"].as_str().map(String::from)).unwrap_or_else(|| body.chars().take(500).collect());
    match status {
        401 | 403 => (CaretError::provider(format!("Invalid API key or unauthorized ({status}): {msg}")), false),
        404 => (CaretError::provider(format!("Model or endpoint not found ({status}): {msg}")), false),
        429 => (CaretError::new("rate_limited", format!("Rate limited (429): {msg}")), true),
        400 | 422 => (CaretError::provider(format!("Bad request ({status}): {msg}")), false),
        s if s >= 500 => (CaretError::new("provider_unavailable", format!("Provider error ({s}): {msg}")), true),
        s => (CaretError::provider(format!("HTTP {s}: {msg}")), false),
    }
}

pub(super) fn retry_delay(resp_headers: &reqwest::header::HeaderMap, attempt: u32) -> std::time::Duration {
    if let Some(ra) = resp_headers.get("retry-after").and_then(|v| v.to_str().ok()).and_then(|s| s.trim().parse::<u64>().ok()) {
        return std::time::Duration::from_secs(ra.min(30));
    }
    std::time::Duration::from_millis(800 * 3u64.pow(attempt))
}

#[async_trait::async_trait]
impl LlmProvider for AnthropicProvider {
    async fn stream(&self, req: &LlmRequest, tx: mpsc::Sender<LlmEvent>, cancel: CancellationToken) -> Result<()> {
        if req.messages.is_empty() { return Err(CaretError::invalid("No messages to send")); }
        if self.resolved.api_key.is_none() && self.is_first_party() {
            let e = CaretError::config("No Anthropic API key configured. Add it in Settings → Models or set ANTHROPIC_API_KEY.");
            let _ = tx.send(LlmEvent::Error { message: e.message.clone(), retryable: false }).await;
            return Err(e);
        }
        let fallbacks = self.wants_fallbacks();
        let body = build_body(req, fallbacks);
        tracing::debug!(purpose = %req.purpose, model = %req.model.model, "anthropic request");
        let url = self.url("messages");

        let mut attempt = 0u32;
        let resp = loop {
            let rb = self.apply_auth(self.client.post(&url), fallbacks.then_some(FALLBACK_BETA)).json(&body);
            let fut = rb.send();
            let resp = tokio::select! { _ = cancel.cancelled() => return Err(CaretError::cancelled()), r = fut => r };
            match resp {
                Ok(r) if r.status().is_success() => break r,
                Ok(r) => {
                    let status = r.status().as_u16();
                    let headers = r.headers().clone();
                    let text = r.text().await.unwrap_or_default();
                    let (err, retryable) = classify_http_error(status, &text);
                    if retryable && attempt < 2 { attempt += 1; tokio::time::sleep(retry_delay(&headers, attempt)).await; continue; }
                    let _ = tx.send(LlmEvent::Error { message: err.message.clone(), retryable }).await;
                    return Err(err);
                }
                Err(e) => {
                    let retryable = e.is_connect() || e.is_timeout();
                    if retryable && attempt < 2 { attempt += 1; tokio::time::sleep(std::time::Duration::from_millis(600 * (attempt as u64 + 1))).await; continue; }
                    let err: CaretError = e.into();
                    let _ = tx.send(LlmEvent::Error { message: err.message.clone(), retryable }).await;
                    return Err(err);
                }
            }
        };

        let mut stream = resp.bytes_stream();
        let mut parser = SseParser::new();
        let mut sent_input_usage = false;
        let mut got_stop = false;
        loop {
            let next = tokio::select! { _ = cancel.cancelled() => return Err(CaretError::cancelled()), n = stream.next() => n };
            let Some(chunk) = next else { break };
            let chunk = match chunk { Ok(c) => c, Err(e) => { let msg = format!("stream read failed: {e}"); let _ = tx.send(LlmEvent::Error { message: msg.clone(), retryable: true }).await; return Err(CaretError::provider(msg)); } };
            for ev in parser.feed(&chunk) {
                for out in translate_event(ev.event.as_deref(), &ev.data, &mut sent_input_usage) {
                    match &out { LlmEvent::Stop { .. } => got_stop = true, LlmEvent::Error { message, .. } => { let m = message.clone(); let _ = tx.send(out).await; return Err(CaretError::provider(m)); } _ => {} }
                    if tx.send(out).await.is_err() { return Ok(()); }
                }
            }
        }
        if let Some(ev) = parser.finish() {
            for out in translate_event(ev.event.as_deref(), &ev.data, &mut sent_input_usage) { if matches!(out, LlmEvent::Stop { .. }) { got_stop = true; } let _ = tx.send(out).await; }
        }
        if !got_stop {
            let _ = tx.send(LlmEvent::Error { message: "stream ended before completion".into(), retryable: true }).await;
            return Err(CaretError::provider("stream ended before completion"));
        }
        Ok(())
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let mut ids = vec![];
        let mut after: Option<String> = None;
        for _ in 0..10 {
            let mut url = format!("{}?limit=100", self.url("models"));
            if let Some(a) = &after { url.push_str(&format!("&after_id={a}")); }
            let r = self.apply_auth(self.client.get(&url), None).send().await?;
            if !r.status().is_success() { let s = r.status().as_u16(); let t = r.text().await.unwrap_or_default(); return Err(classify_http_error(s, &t).0); }
            let v: Value = r.json().await?;
            let data = v["data"].as_array().cloned().unwrap_or_default();
            for m in &data { if let Some(id) = m["id"].as_str() { ids.push(id.to_string()); } }
            if v["has_more"].as_bool() != Some(true) { break; }
            after = v["last_id"].as_str().map(String::from).or_else(|| data.last().and_then(|m| m["id"].as_str().map(String::from)));
            if after.is_none() { break; }
        }
        Ok(ids)
    }

    async fn embed(&self, model: &str, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        if self.is_first_party() { return Err(CaretError::provider("Anthropic does not offer an embeddings endpoint; configure an OpenAI-compatible or Ollama provider for embeddings.")); }
        let url = self.url("embeddings");
        let r = self.apply_auth(self.client.post(&url), None).json(&json!({"model": model, "input": inputs})).send().await?;
        if !r.status().is_success() { let s = r.status().as_u16(); let t = r.text().await.unwrap_or_default(); return Err(classify_http_error(s, &t).0); }
        let v: Value = r.json().await?;
        super::openai_compat::parse_embeddings(&v, inputs.len())
    }

    fn name(&self) -> &'static str { "anthropic" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::llm::{Effort, Message, SystemBlock, ToolDef};
    use crate::core::settings::ModelConfig;

    fn model(id: &str, thinking: bool, effort: bool) -> ModelConfig {
        ModelConfig { id: id.into(), model: id.into(), provider_id: "anthropic".into(), supports_thinking: thinking, supports_effort: effort, max_output: 32_000, ..Default::default() }
    }

    #[test]
    fn body_uses_adaptive_thinking_effort_and_caching() {
        let mut req = LlmRequest::new(model("claude-opus-5-5", true, true));
        req.system = vec![SystemBlock { text: "base".into(), cache: true }, SystemBlock { text: "volatile".into(), cache: false }];
        req.messages = vec![Message::user_text("hi")];
        req.tools = vec![ToolDef { name: "t".into(), description: "d".into(), input_schema: json!({"type":"object"}), strict: true }];
        req.thinking.effort = Effort::Xhigh;
        req.temperature = Some(0.2);
        let b = build_body(&req, true);
        assert_eq!(b["thinking"], json!({"type": "adaptive", "display": "summarized"}));
        assert_eq!(b["output_config"]["effort"], "xhigh");
        assert!(b.get("temperature").is_none(), "no sampling params with thinking");
        assert!(b["thinking"].get("budget_tokens").is_none());
        assert_eq!(b["system"][0]["cache_control"]["type"], "ephemeral");
        assert!(b["system"][1].get("cache_control").is_none());
        assert_eq!(b["messages"][0]["content"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(b["tools"][0]["strict"], true);
        assert_eq!(b["tool_choice"], json!({"type": "auto", "disable_parallel_tool_use": false}));
        assert_eq!(b["fallbacks"], "default");
        assert_eq!(b["stream"], true);
    }

    #[test]
    fn thinking_disabled_rules() {
        // Older model: thinking omitted, temperature allowed.
        let mut req = LlmRequest::new(model("claude-sonnet-4-5", true, false));
        req.messages = vec![Message::user_text("hi")];
        req.temperature = Some(0.1);
        let b = build_body(&req, false);
        assert!(b.get("thinking").is_none());
        assert_eq!(b["temperature"], 0.1);
        // Cannot-disable model with thinking off: omit thinking, effort low, no temperature.
        let mut req = LlmRequest::new(model("claude-opus-5-5", true, true)).no_thinking();
        req.messages = vec![Message::user_text("hi")];
        req.temperature = Some(0.1);
        let b = build_body(&req, false);
        assert!(b.get("thinking").is_none());
        assert_eq!(b["output_config"]["effort"], "low");
        assert!(b.get("temperature").is_none());
        assert!(b.get("fallbacks").is_none());
    }

    #[test]
    fn thinking_blocks_without_signature_are_dropped() {
        let mut req = LlmRequest::new(model("claude-opus-5-5", true, true));
        req.messages = vec![Message::user_text("q"), Message { role: Role::Assistant, content: vec![ContentBlock::Thinking { text: "t".into(), signature: None }, ContentBlock::Thinking { text: "t2".into(), signature: Some("sig".into()) }, ContentBlock::Text { text: "a".into() }] }, Message::user_text("next")];
        let b = build_body(&req, false);
        let asst = &b["messages"][1]["content"];
        assert_eq!(asst.as_array().unwrap().len(), 2);
        assert_eq!(asst[0]["type"], "thinking");
        assert_eq!(asst[0]["signature"], "sig");
    }

    #[test]
    fn translates_stream_events() {
        let mut flag = false;
        let evs = translate_event(Some("message_start"), r#"{"type":"message_start","message":{"model":"claude-opus-5-5","usage":{"input_tokens":10,"cache_read_input_tokens":5,"cache_creation_input_tokens":0,"output_tokens":1}}}"#, &mut flag);
        assert!(matches!(&evs[0], LlmEvent::MessageStart { model } if model == "claude-opus-5-5"));
        assert!(matches!(&evs[1], LlmEvent::Usage(u) if u.input_tokens == 10 && u.cache_read_tokens == 5 && u.output_tokens == 0));
        let evs = translate_event(Some("content_block_start"), r#"{"index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"read_file","input":{}}}"#, &mut flag);
        assert!(matches!(&evs[0], LlmEvent::ToolUseStart { index: 1, id, name } if id == "toolu_1" && name == "read_file"));
        let evs = translate_event(Some("content_block_delta"), r#"{"index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":"}}"#, &mut flag);
        assert!(matches!(&evs[0], LlmEvent::ToolInputDelta { index: 1, partial_json } if partial_json == "{\"path\":"));
        let evs = translate_event(Some("message_delta"), r#"{"delta":{"stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber","explanation":"nope"}},"usage":{"output_tokens":42,"input_tokens":10}}"#, &mut flag);
        assert!(matches!(&evs[0], LlmEvent::Usage(u) if u.output_tokens == 42 && u.input_tokens == 0));
        assert!(matches!(&evs[1], LlmEvent::Stop { reason: StopReason::Refusal, detail: Some(d) } if d == "cyber: nope"));
        let evs = translate_event(Some("error"), r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#, &mut flag);
        assert!(matches!(&evs[0], LlmEvent::Error { retryable: true, .. }));
    }

    #[test]
    fn classifies_http_errors() {
        assert_eq!(classify_http_error(401, r#"{"error":{"message":"invalid x-api-key"}}"#).0.code, "provider");
        assert!(classify_http_error(429, "").1);
        assert!(classify_http_error(503, "").1);
        assert!(!classify_http_error(400, r#"{"error":{"message":"bad"}}"#).1);
        assert!(classify_http_error(400, r#"{"error":{"message":"budget_tokens unsupported"}}"#).0.message.contains("budget_tokens unsupported"));
    }
}

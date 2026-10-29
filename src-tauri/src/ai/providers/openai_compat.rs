//! OpenAI-compatible chat/completions + embeddings provider. Covers OpenAI, Azure OpenAI,
//! OpenRouter, Gemini (OpenAI endpoint), Ollama and any `/v1`-style proxy.
use super::anthropic::{classify_http_error, retry_delay};
use super::http_client;
use super::sse::SseParser;
use crate::ai::llm::{ContentBlock, Effort, LlmEvent, LlmProvider, LlmRequest, ResolvedModel, Role, StopReason, ToolResultContent, UsageDelta};
use crate::core::settings::ProviderKind;
use crate::error::{CaretError, Result};
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub const DEFAULT_BASE: &str = "https://api.openai.com/v1";
/// Block index offsets used for the unified event stream (OpenAI has no block indices).
pub const TEXT_INDEX: u32 = 0;
pub const THINKING_INDEX: u32 = 1;
pub const TOOL_INDEX_BASE: u32 = 100;

pub struct OpenAiCompatProvider {
    pub resolved: ResolvedModel,
    client: reqwest::Client,
}

impl OpenAiCompatProvider {
    pub fn new(resolved: &ResolvedModel) -> Result<Self> { Ok(Self { resolved: resolved.clone(), client: http_client()? }) }

    fn kind(&self) -> &ProviderKind { &self.resolved.provider.kind }
    fn base(&self) -> String {
        self.resolved.provider.base_url.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| DEFAULT_BASE.into()).trim_end_matches('/').to_string()
    }
    fn url(&self, path: &str) -> String {
        let base = self.base();
        if matches!(self.kind(), ProviderKind::AzureOpenai) {
            let ver = self.resolved.provider.api_version.clone().unwrap_or_else(|| "2024-10-21".into());
            return format!("{base}/{path}?api-version={ver}");
        }
        format!("{base}/{path}")
    }
    fn apply_auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut rb = rb.header("content-type", "application/json");
        if let Some(key) = &self.resolved.api_key {
            rb = match (self.kind(), self.resolved.provider.auth_style.as_deref()) {
                (ProviderKind::AzureOpenai, _) | (_, Some("api-key")) => rb.header("api-key", key.clone()),
                (_, Some("x-api-key")) => rb.header("x-api-key", key.clone()),
                _ => rb.header("authorization", format!("Bearer {key}")),
            };
        }
        if matches!(self.kind(), ProviderKind::Openrouter) { rb = rb.header("HTTP-Referer", "https://caret.dev").header("X-Title", "Caret"); }
        if let Some(h) = &self.resolved.provider.headers { for (k, v) in h { rb = rb.header(k.as_str(), v.as_str()); } }
        rb
    }
}

// ───────────────────────── request body ─────────────────────────

fn is_openai_reasoning_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.starts_with("o1") || m.starts_with("o3") || m.starts_with("o4") || m.starts_with("gpt-5") || m.contains("/o1") || m.contains("/o3")
}

fn map_effort_openai(e: Effort) -> &'static str { match e { Effort::Low => "low", Effort::Medium => "medium", _ => "high" } }

/// Build the chat/completions body. Pure function (unit-tested).
pub fn build_body(req: &LlmRequest, kind: &ProviderKind, include_stream_options: bool) -> Value {
    let model = &req.model;
    let mut messages: Vec<Value> = Vec::new();

    let system_text: String = req.system.iter().map(|b| b.text.as_str()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n\n");
    if !system_text.is_empty() {
        let role = if matches!(kind, ProviderKind::Openai | ProviderKind::AzureOpenai) && is_openai_reasoning_model(&model.model) { "developer" } else { "system" };
        messages.push(json!({"role": role, "content": system_text}));
    }

    for m in &req.messages {
        match m.role {
            Role::User => {
                // tool results first (must directly follow the assistant tool_calls), then remaining content as a user message
                let mut parts: Vec<Value> = vec![];
                for b in &m.content {
                    match b {
                        ContentBlock::ToolResult { tool_use_id, content, is_error } => {
                            let mut text: String = content.iter().map(|c| match c { ToolResultContent::Text { text } => text.clone(), ToolResultContent::Image { .. } => "[image]".to_string() }).collect::<Vec<_>>().join("\n");
                            if *is_error && !text.starts_with("Error") { text = format!("Error: {text}"); }
                            messages.push(json!({"role": "tool", "tool_call_id": tool_use_id, "content": text}));
                        }
                        ContentBlock::Text { text } => { if !text.is_empty() { parts.push(json!({"type": "text", "text": text})); } }
                        ContentBlock::Image { media_type, data } => parts.push(json!({"type": "image_url", "image_url": {"url": format!("data:{media_type};base64,{data}")}})),
                        _ => {}
                    }
                }
                if !parts.is_empty() {
                    let content = if parts.len() == 1 && parts[0]["type"] == "text" { parts[0]["text"].clone() } else { Value::Array(parts) };
                    messages.push(json!({"role": "user", "content": content}));
                }
            }
            Role::Assistant => {
                let text: String = m.content.iter().filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }).collect::<Vec<_>>().join("");
                let tool_calls: Vec<Value> = m.content.iter().filter_map(|b| if let ContentBlock::ToolUse { id, name, input } = b {
                    Some(json!({"id": id, "type": "function", "function": {"name": name, "arguments": if input.is_null() { "{}".to_string() } else { input.to_string() }}}))
                } else { None }).collect();
                if text.is_empty() && tool_calls.is_empty() { continue; }
                let mut v = json!({"role": "assistant", "content": if text.is_empty() { Value::Null } else { Value::String(text) }});
                if !tool_calls.is_empty() { v["tool_calls"] = Value::Array(tool_calls); }
                messages.push(v);
            }
        }
    }

    let mut body = json!({"model": model.model, "messages": messages, "stream": true});
    if include_stream_options && !matches!(kind, ProviderKind::Gemini) { body["stream_options"] = json!({"include_usage": true}); }
    let max = req.max_tokens.min(model.max_output.max(1));
    if matches!(kind, ProviderKind::Openai | ProviderKind::AzureOpenai) { body["max_completion_tokens"] = json!(max); } else { body["max_tokens"] = json!(max); }

    if !req.tools.is_empty() {
        body["tools"] = Value::Array(req.tools.iter().map(|t| {
            let mut f = json!({"name": t.name, "description": t.description, "parameters": t.input_schema});
            if t.strict { f["strict"] = json!(true); }
            json!({"type": "function", "function": f})
        }).collect());
        body["tool_choice"] = json!("auto");
        if !matches!(kind, ProviderKind::Gemini | ProviderKind::Ollama) { body["parallel_tool_calls"] = json!(req.parallel_tool_calls); }
    }
    if !req.stop.is_empty() { body["stop"] = json!(req.stop); }

    let reasoning = req.thinking.enabled && (model.supports_thinking || model.supports_effort);
    let mut sampling_ok = true;
    match kind {
        ProviderKind::Openai | ProviderKind::AzureOpenai => {
            if is_openai_reasoning_model(&model.model) {
                sampling_ok = false;
                if reasoning && model.supports_effort { body["reasoning_effort"] = json!(map_effort_openai(req.thinking.effort)); }
            } else if reasoning && model.supports_effort { body["reasoning_effort"] = json!(map_effort_openai(req.thinking.effort)); sampling_ok = false; }
        }
        ProviderKind::Openrouter => {
            if reasoning { body["reasoning"] = json!({"effort": map_effort_openai(req.thinking.effort)}); }
            // Hybrid models (DeepSeek V4, Qwen3, …) reason by default; without an explicit off switch a small-budget
            // request (Tab: 96–256 tokens) spends everything on hidden reasoning and returns an empty body.
            else if !req.thinking.enabled { body["reasoning"] = json!({"enabled": false}); }
        }
        ProviderKind::OpenaiCompatible => { if reasoning && model.supports_effort { body["reasoning_effort"] = json!(map_effort_openai(req.thinking.effort)); sampling_ok = false; } }
        _ => {}
    }
    if sampling_ok { if let Some(t) = req.temperature { body["temperature"] = json!((t as f64 * 1000.0).round() / 1000.0); } }
    body
}

// ───────────────────────── streaming ─────────────────────────

#[derive(Default)]
pub struct ChunkState {
    /// OpenAI tool index → (unified block index, id)
    tools: BTreeMap<u64, (u32, String)>,
    pub finish: Option<StopReason>,
    pub finish_detail: Option<String>,
    pub saw_text: bool,
    pub saw_thinking: bool,
    usage_sent: bool,
    pub usage: Option<UsageDelta>,
}

fn map_finish(r: &str) -> StopReason {
    match r { "stop" => StopReason::EndTurn, "tool_calls" | "function_call" => StopReason::ToolUse, "length" => StopReason::MaxTokens, "content_filter" => StopReason::Refusal, _ => StopReason::Other }
}

/// Translate one `data:` JSON chunk into LlmEvents. Pure (unit-tested).
pub fn translate_chunk(data: &str, st: &mut ChunkState) -> Vec<LlmEvent> {
    let Ok(v) = serde_json::from_str::<Value>(data) else { return vec![] };
    if let Some(err) = v.get("error") {
        let msg = err.get("message").and_then(Value::as_str).unwrap_or("stream error").to_string();
        return vec![LlmEvent::Error { message: msg, retryable: false }];
    }
    let mut out = vec![];
    if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
        let ud = UsageDelta {
            input_tokens: u["prompt_tokens"].as_u64().unwrap_or(0).saturating_sub(u["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0)),
            output_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
            cache_read_tokens: u["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0),
            cache_write_tokens: 0,
            thinking_tokens: u["completion_tokens_details"]["reasoning_tokens"].as_u64().unwrap_or(0),
        };
        st.usage = Some(ud);
    }
    let Some(choice) = v["choices"].as_array().and_then(|a| a.first()) else { return out };
    let delta = &choice["delta"];
    if let Some(t) = delta.get("content") {
        match t {
            Value::String(s) if !s.is_empty() => { st.saw_text = true; out.push(LlmEvent::TextDelta { index: TEXT_INDEX, text: s.clone() }); }
            Value::Array(parts) => { for p in parts { if let Some(s) = p["text"].as_str() { if !s.is_empty() { st.saw_text = true; out.push(LlmEvent::TextDelta { index: TEXT_INDEX, text: s.to_string() }); } } } }
            _ => {}
        }
    }
    for key in ["reasoning_content", "reasoning"] {
        if let Some(s) = delta.get(key).and_then(Value::as_str) { if !s.is_empty() { st.saw_thinking = true; out.push(LlmEvent::ThinkingDelta { index: THINKING_INDEX, text: s.to_string() }); } }
    }
    if let Some(tcs) = delta["tool_calls"].as_array() {
        for tc in tcs {
            let oi = tc["index"].as_u64().unwrap_or(st.tools.len() as u64);
            let f = &tc["function"];
            if !st.tools.contains_key(&oi) {
                let idx = TOOL_INDEX_BASE + oi as u32;
                let id = tc["id"].as_str().map(String::from).filter(|s| !s.is_empty()).unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4().simple()));
                st.tools.insert(oi, (idx, id.clone()));
                out.push(LlmEvent::ToolUseStart { index: idx, id, name: f["name"].as_str().unwrap_or("").to_string() });
            }
            if let Some(args) = f["arguments"].as_str() { if !args.is_empty() { let idx = st.tools[&oi].0; out.push(LlmEvent::ToolInputDelta { index: idx, partial_json: args.to_string() }); } }
        }
    }
    if let Some(fr) = choice["finish_reason"].as_str() {
        st.finish = Some(map_finish(fr));
        if fr == "content_filter" { st.finish_detail = Some("content_filter".into()); }
    }
    out
}

/// Events to emit once the stream is complete.
pub fn finalize(st: &mut ChunkState) -> Vec<LlmEvent> {
    let mut out = vec![];
    for (_, (idx, _)) in st.tools.iter() { out.push(LlmEvent::BlockStop { index: *idx }); }
    if let Some(u) = st.usage.take() { if !st.usage_sent { st.usage_sent = true; out.push(LlmEvent::Usage(u)); } }
    let reason = match st.finish { Some(r) => r, None => if st.tools.is_empty() { StopReason::EndTurn } else { StopReason::ToolUse } };
    // Some providers report "stop" even when tool calls were produced.
    let reason = if reason == StopReason::EndTurn && !st.tools.is_empty() { StopReason::ToolUse } else { reason };
    out.push(LlmEvent::Stop { reason, detail: st.finish_detail.take() });
    out
}

pub fn parse_embeddings(v: &Value, expected: usize) -> Result<Vec<Vec<f32>>> {
    let data = v["data"].as_array().ok_or_else(|| CaretError::provider("embeddings response missing data"))?;
    let mut rows: Vec<(usize, Vec<f32>)> = data.iter().enumerate().map(|(i, d)| {
        let idx = d["index"].as_u64().map(|x| x as usize).unwrap_or(i);
        let emb = d["embedding"].as_array().map(|a| a.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect()).unwrap_or_default();
        (idx, emb)
    }).collect();
    rows.sort_by_key(|(i, _)| *i);
    let out: Vec<Vec<f32>> = rows.into_iter().map(|(_, e)| e).collect();
    if out.len() != expected { return Err(CaretError::provider(format!("embeddings count mismatch: got {}, expected {expected}", out.len()))); }
    Ok(out)
}

#[async_trait::async_trait]
impl LlmProvider for OpenAiCompatProvider {
    async fn stream(&self, req: &LlmRequest, tx: mpsc::Sender<LlmEvent>, cancel: CancellationToken) -> Result<()> {
        if req.messages.is_empty() { return Err(CaretError::invalid("No messages to send")); }
        if self.resolved.api_key.is_none() && matches!(self.kind(), ProviderKind::Openai | ProviderKind::AzureOpenai | ProviderKind::Openrouter | ProviderKind::Gemini) {
            let e = CaretError::config(format!("No API key configured for provider '{}'. Add it in Settings → Models.", self.resolved.provider.name));
            let _ = tx.send(LlmEvent::Error { message: e.message.clone(), retryable: false }).await;
            return Err(e);
        }
        let url = self.url("chat/completions");
        tracing::debug!(purpose = %req.purpose, model = %req.model.model, provider = %self.resolved.provider.id, "openai-compatible request");
        let mut include_stream_options = true;
        let mut attempt = 0u32;
        let resp = loop {
            let body = build_body(req, self.kind(), include_stream_options);
            let fut = self.apply_auth(self.client.post(&url)).json(&body).send();
            let resp = tokio::select! { _ = cancel.cancelled() => return Err(CaretError::cancelled()), r = fut => r };
            match resp {
                Ok(r) if r.status().is_success() => break r,
                Ok(r) => {
                    let status = r.status().as_u16();
                    let headers = r.headers().clone();
                    let text = r.text().await.unwrap_or_default();
                    if status == 400 && include_stream_options && text.contains("stream_options") { include_stream_options = false; continue; }
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
        let _ = tx.send(LlmEvent::MessageStart { model: req.model.model.clone() }).await;
        let is_sse = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).map(|c| c.contains("text/event-stream")).unwrap_or(true);
        if !is_sse {
            // Non-streaming JSON reply (some proxies ignore stream:true).
            let v: Value = resp.json().await?;
            let mut st = ChunkState::default();
            let msg = &v["choices"][0]["message"];
            if let Some(t) = msg["content"].as_str() { if !t.is_empty() { let _ = tx.send(LlmEvent::TextDelta { index: TEXT_INDEX, text: t.to_string() }).await; } }
            if let Some(tcs) = msg["tool_calls"].as_array() {
                for (i, tc) in tcs.iter().enumerate() {
                    let idx = TOOL_INDEX_BASE + i as u32; let id = tc["id"].as_str().unwrap_or("call").to_string();
                    st.tools.insert(i as u64, (idx, id.clone()));
                    let _ = tx.send(LlmEvent::ToolUseStart { index: idx, id, name: tc["function"]["name"].as_str().unwrap_or("").into() }).await;
                    let _ = tx.send(LlmEvent::ToolInputDelta { index: idx, partial_json: tc["function"]["arguments"].as_str().unwrap_or("{}").into() }).await;
                }
            }
            if let Some(fr) = v["choices"][0]["finish_reason"].as_str() { st.finish = Some(map_finish(fr)); }
            let _ = translate_chunk(&json!({"usage": v["usage"], "choices": []}).to_string(), &mut st);
            for ev in finalize(&mut st) { let _ = tx.send(ev).await; }
            return Ok(());
        }
        let mut stream = resp.bytes_stream();
        let mut parser = SseParser::new();
        let mut st = ChunkState::default();
        let mut done = false;
        'outer: loop {
            let next = tokio::select! { _ = cancel.cancelled() => return Err(CaretError::cancelled()), n = stream.next() => n };
            let Some(chunk) = next else { break };
            let chunk = match chunk { Ok(c) => c, Err(e) => { let msg = format!("stream read failed: {e}"); let _ = tx.send(LlmEvent::Error { message: msg.clone(), retryable: true }).await; return Err(CaretError::provider(msg)); } };
            for ev in parser.feed(&chunk) {
                if ev.data.trim() == "[DONE]" { done = true; break 'outer; }
                for out in translate_chunk(&ev.data, &mut st) {
                    if let LlmEvent::Error { message, .. } = &out { let m = message.clone(); let _ = tx.send(out).await; return Err(CaretError::provider(m)); }
                    if tx.send(out).await.is_err() { return Ok(()); }
                }
            }
        }
        if !done { if let Some(ev) = parser.finish() { if ev.data.trim() != "[DONE]" { for out in translate_chunk(&ev.data, &mut st) { let _ = tx.send(out).await; } } } }
        if !done && !st.saw_text && st.tools.is_empty() && st.finish.is_none() {
            let _ = tx.send(LlmEvent::Error { message: "stream ended before completion".into(), retryable: true }).await;
            return Err(CaretError::provider("stream ended before completion"));
        }
        for ev in finalize(&mut st) { let _ = tx.send(ev).await; }
        Ok(())
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let r = self.apply_auth(self.client.get(self.url("models"))).send().await?;
        if !r.status().is_success() { let s = r.status().as_u16(); let t = r.text().await.unwrap_or_default(); return Err(classify_http_error(s, &t).0); }
        let v: Value = r.json().await?;
        let arr = v["data"].as_array().cloned().or_else(|| v["models"].as_array().cloned()).unwrap_or_default();
        let mut ids: Vec<String> = arr.iter().filter_map(|m| m["id"].as_str().or_else(|| m["name"].as_str()).map(String::from)).collect();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    async fn embed(&self, model: &str, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(inputs.len());
        for batch in inputs.chunks(64) {
            let r = self.apply_auth(self.client.post(self.url("embeddings"))).json(&json!({"model": model, "input": batch, "encoding_format": "float"})).send().await?;
            if !r.status().is_success() { let s = r.status().as_u16(); let t = r.text().await.unwrap_or_default(); return Err(classify_http_error(s, &t).0); }
            let v: Value = r.json().await?;
            out.extend(parse_embeddings(&v, batch.len())?);
        }
        Ok(out)
    }

    fn name(&self) -> &'static str { "openai-compatible" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::llm::{Message, SystemBlock, ToolDef};
    use crate::core::settings::ModelConfig;

    fn model(id: &str) -> ModelConfig { ModelConfig { id: id.into(), model: id.into(), provider_id: "openai".into(), supports_tools: true, supports_thinking: true, supports_effort: true, max_output: 8000, ..Default::default() } }

    #[test]
    fn maps_tools_and_tool_results() {
        let mut req = LlmRequest::new(model("gpt-4o"));
        req.system = vec![SystemBlock { text: "sys".into(), cache: true }];
        req.tools = vec![ToolDef { name: "read_file".into(), description: "Read".into(), input_schema: json!({"type":"object","properties":{"path":{"type":"string"}}}), strict: false }];
        req.messages = vec![
            Message::user_text("open it"),
            Message { role: Role::Assistant, content: vec![ContentBlock::Thinking { text: "hmm".into(), signature: None }, ContentBlock::ToolUse { id: "call_1".into(), name: "read_file".into(), input: json!({"path": "a.rs"}) }] },
            Message { role: Role::User, content: vec![ContentBlock::ToolResult { tool_use_id: "call_1".into(), content: vec![ToolResultContent::Text { text: "fn main(){}".into() }], is_error: false }, ContentBlock::Text { text: "thanks".into() }] },
        ];
        req.temperature = Some(0.3);
        let b = build_body(&req, &ProviderKind::OpenaiCompatible, true);
        let msgs = b["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["content"], "open it");
        assert_eq!(msgs[2]["role"], "assistant");
        assert_eq!(msgs[2]["tool_calls"][0]["function"]["arguments"], "{\"path\":\"a.rs\"}");
        assert_eq!(msgs[3]["role"], "tool");
        assert_eq!(msgs[3]["tool_call_id"], "call_1");
        assert_eq!(msgs[4]["role"], "user");
        assert_eq!(b["tools"][0]["function"]["name"], "read_file");
        assert_eq!(b["tool_choice"], "auto");
        assert_eq!(b["parallel_tool_calls"], true);
        assert_eq!(b["max_tokens"], 8000);
        assert_eq!(b["stream_options"]["include_usage"], true);
        // supports_effort + thinking → reasoning_effort, no temperature
        assert_eq!(b["reasoning_effort"], "high");
        assert!(b.get("temperature").is_none());
    }

    #[test]
    fn openrouter_disables_reasoning_when_thinking_is_off() {
        let req = LlmRequest::new(model("deepseek/deepseek-v4.1-flash")).no_thinking().max_tokens(200);
        let b = build_body(&req, &ProviderKind::Openrouter, true);
        assert_eq!(b["reasoning"]["enabled"], false);
        let mut on = LlmRequest::new(model("deepseek/deepseek-v4.1-flash")).max_tokens(200); on.thinking.enabled = true; on.thinking.effort = Effort::Low;
        let b = build_body(&on, &ProviderKind::Openrouter, true);
        assert_eq!(b["reasoning"]["effort"], "low");
        assert!(b["reasoning"].get("enabled").is_none());
    }

    #[test]
    fn openai_reasoning_models_use_developer_role_and_max_completion_tokens() {
        let mut req = LlmRequest::new(model("gpt-5"));
        req.system = vec![SystemBlock { text: "sys".into(), cache: true }];
        req.messages = vec![Message::user_text("hi")];
        req.thinking.effort = Effort::Xhigh;
        let b = build_body(&req, &ProviderKind::Openai, true);
        assert_eq!(b["messages"][0]["role"], "developer");
        assert_eq!(b["max_completion_tokens"], 8000);
        assert_eq!(b["reasoning_effort"], "high");
        let mut m = model("gpt-4o"); m.supports_effort = false; m.supports_thinking = false;
        let mut req = LlmRequest::new(m); req.messages = vec![Message::user_text("hi")]; req.temperature = Some(0.5);
        let b = build_body(&req, &ProviderKind::Openai, true);
        assert_eq!(b["messages"][0]["role"], "user");
        assert_eq!(b["temperature"], 0.5);
        assert!(b.get("reasoning_effort").is_none());
        let b = build_body(&req, &ProviderKind::Gemini, true);
        assert!(b.get("stream_options").is_none());
    }

    #[test]
    fn translates_chunks_with_split_tool_arguments() {
        let mut st = ChunkState::default();
        let e = translate_chunk(r#"{"choices":[{"delta":{"role":"assistant","content":"Hel"}}]}"#, &mut st);
        assert!(matches!(&e[0], LlmEvent::TextDelta { index: 0, text } if text == "Hel"));
        let e = translate_chunk(r#"{"choices":[{"delta":{"reasoning_content":"think"}}]}"#, &mut st);
        assert!(matches!(&e[0], LlmEvent::ThinkingDelta { index: 1, .. }));
        let e = translate_chunk(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_9","type":"function","function":{"name":"grep","arguments":""}}]}}]}"#, &mut st);
        assert!(matches!(&e[0], LlmEvent::ToolUseStart { index: 100, id, name } if id == "call_9" && name == "grep"));
        assert_eq!(e.len(), 1);
        let e = translate_chunk(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"q\":"}}]}}]}"#, &mut st);
        assert!(matches!(&e[0], LlmEvent::ToolInputDelta { index: 100, partial_json } if partial_json == "{\"q\":"));
        let e = translate_chunk(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"x\"}"}}]},"finish_reason":"tool_calls"}]}"#, &mut st);
        assert_eq!(e.len(), 1);
        let e = translate_chunk(r#"{"choices":[],"usage":{"prompt_tokens":20,"completion_tokens":7,"prompt_tokens_details":{"cached_tokens":5}}}"#, &mut st);
        assert!(e.is_empty());
        let fin = finalize(&mut st);
        assert!(matches!(&fin[0], LlmEvent::BlockStop { index: 100 }));
        assert!(matches!(&fin[1], LlmEvent::Usage(u) if u.input_tokens == 15 && u.cache_read_tokens == 5 && u.output_tokens == 7));
        assert!(matches!(&fin[2], LlmEvent::Stop { reason: StopReason::ToolUse, .. }));
    }

    #[test]
    fn finalize_without_finish_reason_and_error_chunks() {
        let mut st = ChunkState::default();
        translate_chunk(r#"{"choices":[{"delta":{"content":"x"}}]}"#, &mut st);
        let fin = finalize(&mut st);
        assert!(matches!(fin.last().unwrap(), LlmEvent::Stop { reason: StopReason::EndTurn, .. }));
        let e = translate_chunk(r#"{"error":{"message":"boom","type":"server_error"}}"#, &mut ChunkState::default());
        assert!(matches!(&e[0], LlmEvent::Error { message, .. } if message == "boom"));
    }

    #[test]
    fn parses_embeddings_in_index_order() {
        let v = json!({"data": [{"index": 1, "embedding": [0.5, 0.5]}, {"index": 0, "embedding": [1.0, 0.0]}]});
        let e = parse_embeddings(&v, 2).unwrap();
        assert_eq!(e[0], vec![1.0, 0.0]);
        assert!(parse_embeddings(&v, 3).is_err());
    }
}

#[cfg(test)]
mod live_tests {
    //! Live smoke test against an OpenAI-compatible proxy. Run with:
    //! CARET_LIVE_TEST=1 LYNKR_API_KEY=… cargo test live_openai_compat -- --nocapture
    use super::*;
    use crate::ai::llm::{run_stream, Message, SystemBlock, ToolDef};
    use crate::core::settings::{ModelConfig, ProviderConfig};
    use std::sync::Arc;

    #[tokio::test]
    async fn live_openai_compat_stream_with_tool_call() {
        if std::env::var("CARET_LIVE_TEST").ok().as_deref() != Some("1") { eprintln!("skipped (set CARET_LIVE_TEST=1)"); return; }
        let base = std::env::var("CARET_LIVE_BASE").unwrap_or_else(|_| "http://localhost:8081/v1".into());
        let model_id = std::env::var("CARET_LIVE_MODEL").unwrap_or_else(|_| "claude-sonnet-4.5".into());
        let key = std::env::var("LYNKR_API_KEY").ok();
        let provider = ProviderConfig { id: "local".into(), kind: ProviderKind::OpenaiCompatible, name: "local".into(), base_url: Some(base), api_key_ref: Some("local".into()), enabled: true, ..Default::default() };
        let model = ModelConfig { id: model_id.clone(), model: model_id, provider_id: "local".into(), display_name: "live".into(), max_output: 512, supports_tools: true, ..Default::default() };
        let resolved = ResolvedModel { model: model.clone(), provider, api_key: key };
        let p: Arc<dyn LlmProvider> = Arc::new(OpenAiCompatProvider::new(&resolved).unwrap());
        let mut req = LlmRequest::new(model).no_thinking().max_tokens(200);
        req.system = vec![SystemBlock { text: "You are a terse assistant. When asked about weather you MUST call the get_weather tool.".into(), cache: false }];
        req.messages = vec![Message::user_text("What's the weather in Paris? Use the tool.")];
        req.tools = vec![ToolDef { name: "get_weather".into(), description: "Get weather for a city".into(), input_schema: json!({"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}), strict: false }];
        req.cache = false;
        let mut events = vec![];
        let turn = run_stream(p.clone(), &req, CancellationToken::new(), |e| events.push(format!("{e:?}"))).await.expect("stream ok");
        eprintln!("events={} stop={:?} usage={:?}\ncontent={:?}", events.len(), turn.stop, turn.usage, turn.content);
        let tools = turn.tool_uses();
        assert!(!tools.is_empty(), "expected a tool call, got {:?}", turn.content);
        assert_eq!(tools[0].1, "get_weather");
        assert!(tools[0].2["city"].as_str().map(|c| c.to_ascii_lowercase().contains("paris")).unwrap_or(false), "args={:?}", tools[0].2);
        assert_eq!(turn.stop, Some(StopReason::ToolUse));
        assert!(turn.usage.output_tokens > 0 || turn.usage.input_tokens > 0, "usage should be reported");
        // second turn: provide the tool result and expect text
        let mut req2 = req.clone();
        req2.messages.push(Message { role: Role::Assistant, content: turn.content.clone() });
        req2.messages.push(Message { role: Role::User, content: vec![ContentBlock::ToolResult { tool_use_id: tools[0].0.clone(), content: vec![ToolResultContent::Text { text: "18°C, cloudy".into() }], is_error: false }] });
        let turn2 = run_stream(p, &req2, CancellationToken::new(), |_| {}).await.expect("second stream ok");
        eprintln!("turn2 text={:?} stop={:?}", turn2.text(), turn2.stop);
        assert!(turn2.text().to_ascii_lowercase().contains("18") || turn2.text().to_ascii_lowercase().contains("cloud"), "text={:?}", turn2.text());
        assert_eq!(turn2.stop, Some(StopReason::EndTurn));
    }
}

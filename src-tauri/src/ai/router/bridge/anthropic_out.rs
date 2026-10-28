//! `LlmEvent` → Anthropic SSE frames / full Message JSON, byte-compatible with what `providers/anthropic.rs`
//! expects back. Thinking signatures and redacted data are prefixed with the Caret model id (see `anthropic_in`).
use crate::ai::llm::{AssistantTurn, ContentBlock, LlmEvent, StopReason, UsageDelta};
use crate::error::CaretError;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn frame(event: &str, data: &Value) -> String { format!("event: {event}\ndata: {data}\n\n") }
pub fn ping_frame() -> String { frame("ping", &json!({"type": "ping"})) }
pub fn error_frame(message: &str, retryable: bool) -> String { frame("error", &json!({"type": "error", "error": {"type": if retryable { "overloaded_error" } else { "api_error" }, "message": message}})) }

pub fn stop_reason_str(r: StopReason) -> &'static str {
    match r { StopReason::EndTurn | StopReason::Other => "end_turn", StopReason::ToolUse => "tool_use", StopReason::MaxTokens => "max_tokens", StopReason::StopSequence => "stop_sequence", StopReason::Refusal => "refusal", StopReason::PauseTurn => "pause_turn" }
}
fn usage_json(u: &UsageDelta) -> Value { json!({"input_tokens": u.input_tokens, "output_tokens": u.output_tokens, "cache_read_input_tokens": u.cache_read_tokens, "cache_creation_input_tokens": u.cache_write_tokens}) }

/// HTTP status + Anthropic error type for a failure, chosen so Lynkr's tier-fallback / circuit breakers react:
/// 429 and 503 are retryable upstream, 502 is a hard provider failure, 4xx are our own validation errors.
pub fn status_for(e: &CaretError) -> (u16, &'static str) {
    let msg = e.message.to_ascii_lowercase();
    match e.code.as_str() {
        "rate_limited" => (429, "rate_limit_error"),
        _ if msg.contains("429") || msg.contains("rate limit") => (429, "rate_limit_error"),
        "provider_unavailable" | "timeout" | "connect" => (503, "overloaded_error"),
        _ if msg.contains("stream ended before completion") => (503, "overloaded_error"),
        "cancelled" => (499, "api_error"),
        "invalid" | "json" => (400, "invalid_request_error"),
        "not_found" => (404, "not_found_error"),
        _ => (502, "api_error"),
    }
}
pub fn error_body(e: &CaretError) -> (u16, Value) { let (status, typ) = status_for(e); (status, json!({"type": "error", "error": {"type": typ, "message": e.message}})) }

#[derive(Clone, Copy, PartialEq)]
enum Kind { Text, Thinking, Tool }

/// Incremental SSE writer: feed `LlmEvent`s, get frames to write. `message_start` is emitted lazily so the input
/// usage the provider reports right after `MessageStart` lands inside it (providers that only report usage at the
/// end get a `message_start` without `usage`, and the totals arrive in `message_delta`).
pub struct SseWriter {
    model_id: String,
    started: bool,
    input_usage: Option<UsageDelta>,
    output: UsageDelta,
    open: BTreeMap<u32, Kind>,
    pub stopped: bool,
    pub errored: bool,
}

impl SseWriter {
    pub fn new(model_id: &str) -> Self { Self { model_id: model_id.into(), started: false, input_usage: None, output: UsageDelta::default(), open: BTreeMap::new(), stopped: false, errored: false } }
    fn prefixed(&self, v: &str) -> String { format!("{}|{}", self.model_id, v) }

    fn start_frame(&mut self) -> Option<String> {
        if self.started { return None; }
        self.started = true;
        let mut msg = json!({"id": format!("msg_{}", uuid::Uuid::new_v4().simple()), "type": "message", "role": "assistant", "model": self.model_id, "content": [], "stop_reason": null, "stop_sequence": null});
        if let Some(u) = &self.input_usage { let mut u = u.clone(); u.output_tokens = 0; msg["usage"] = usage_json(&u); }
        Some(frame("message_start", &json!({"type": "message_start", "message": msg})))
    }
    fn open_block(&mut self, index: u32, kind: Kind, out: &mut Vec<String>) {
        if self.open.contains_key(&index) { return; }
        self.open.insert(index, kind);
        let cb = match kind { Kind::Text => json!({"type": "text", "text": ""}), Kind::Thinking => json!({"type": "thinking", "thinking": ""}), Kind::Tool => json!({"type": "tool_use", "id": "", "name": "", "input": {}}) };
        out.push(frame("content_block_start", &json!({"type": "content_block_start", "index": index, "content_block": cb})));
    }
    fn close_block(&mut self, index: u32, out: &mut Vec<String>) {
        if self.open.remove(&index).is_some() { out.push(frame("content_block_stop", &json!({"type": "content_block_stop", "index": index}))); }
    }

    pub fn on_event(&mut self, ev: &LlmEvent) -> Vec<String> {
        let mut out = vec![];
        match ev {
            // Deferred: the frame is written on the first content event so the input usage (reported right after
            // MessageStart by Anthropic-style providers) can be included in it.
            LlmEvent::MessageStart { .. } | LlmEvent::Routed { .. } => return out,
            LlmEvent::Usage(u) if !self.started => { let mut acc = self.input_usage.take().unwrap_or_default(); acc.add(u); self.input_usage = Some(acc); return out; }
            _ => {}
        }
        if let Some(f) = self.start_frame() { out.push(f); }
        match ev {
            LlmEvent::MessageStart { .. } | LlmEvent::Routed { .. } => {}
            LlmEvent::TextDelta { index, text } => { self.open_block(*index, Kind::Text, &mut out); out.push(frame("content_block_delta", &json!({"type": "content_block_delta", "index": index, "delta": {"type": "text_delta", "text": text}}))); }
            LlmEvent::ThinkingDelta { index, text } => { self.open_block(*index, Kind::Thinking, &mut out); out.push(frame("content_block_delta", &json!({"type": "content_block_delta", "index": index, "delta": {"type": "thinking_delta", "thinking": text}}))); }
            LlmEvent::ThinkingSignature { index, signature } => { self.open_block(*index, Kind::Thinking, &mut out); let sig = self.prefixed(signature); out.push(frame("content_block_delta", &json!({"type": "content_block_delta", "index": index, "delta": {"type": "signature_delta", "signature": sig}}))); }
            LlmEvent::RedactedThinking { index, data } => {
                let data = self.prefixed(data);
                out.push(frame("content_block_start", &json!({"type": "content_block_start", "index": index, "content_block": {"type": "redacted_thinking", "data": data}})));
                out.push(frame("content_block_stop", &json!({"type": "content_block_stop", "index": index})));
            }
            LlmEvent::ToolUseStart { index, id, name } => {
                self.open.insert(*index, Kind::Tool);
                out.push(frame("content_block_start", &json!({"type": "content_block_start", "index": index, "content_block": {"type": "tool_use", "id": id, "name": name, "input": {}}})));
            }
            LlmEvent::ToolInputDelta { index, partial_json } => { out.push(frame("content_block_delta", &json!({"type": "content_block_delta", "index": index, "delta": {"type": "input_json_delta", "partial_json": partial_json}}))); }
            LlmEvent::BlockStop { index } => self.close_block(*index, &mut out),
            LlmEvent::Usage(u) => self.output.add(u),
            LlmEvent::Stop { reason, detail } => {
                let open: Vec<u32> = self.open.keys().copied().collect();
                for i in open { self.close_block(i, &mut out); }
                let mut delta = json!({"stop_reason": stop_reason_str(*reason), "stop_sequence": null});
                if let Some(d) = detail { delta["stop_details"] = json!({"explanation": d}); }
                // Input/cache tokens were reported in message_start when known; otherwise send the totals here.
                let usage = if self.input_usage.is_some() { json!({"output_tokens": self.output.output_tokens}) } else { usage_json(&self.output) };
                out.push(frame("message_delta", &json!({"type": "message_delta", "delta": delta, "usage": usage})));
                out.push(frame("message_stop", &json!({"type": "message_stop"})));
                self.stopped = true;
            }
            LlmEvent::Error { message, retryable } => { out.push(error_frame(message, *retryable)); self.errored = true; }
        }
        out
    }
}

/// Full (non-streaming) Message JSON for an assembled turn.
pub fn message_json(turn: &AssistantTurn, model_id: &str) -> Value {
    let prefixed = |v: &str| format!("{model_id}|{v}");
    let content: Vec<Value> = turn.content.iter().filter_map(|b| Some(match b {
        ContentBlock::Text { text } => json!({"type": "text", "text": text}),
        ContentBlock::Thinking { text, signature } => { let mut v = json!({"type": "thinking", "thinking": text}); if let Some(s) = signature { v["signature"] = json!(prefixed(s)); } v }
        ContentBlock::RedactedThinking { data } => json!({"type": "redacted_thinking", "data": prefixed(data)}),
        ContentBlock::ToolUse { id, name, input } => json!({"type": "tool_use", "id": id, "name": name, "input": if input.is_null() { json!({}) } else { input.clone() }}),
        ContentBlock::Image { .. } | ContentBlock::ToolResult { .. } => return None,
    })).collect();
    json!({
        "id": format!("msg_{}", uuid::Uuid::new_v4().simple()), "type": "message", "role": "assistant", "model": model_id,
        "content": content, "stop_reason": turn.stop.map(stop_reason_str).unwrap_or("end_turn"), "stop_sequence": null,
        "usage": usage_json(&turn.usage),
    })
}

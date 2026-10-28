//! Loopback provider bridge — Lynkr's single upstream.
//!
//! Contract (`http://127.0.0.1:<port>`, loopback only):
//! - Auth: every request carries the per-launch token in `x-api-key` or `Authorization: Bearer …`; otherwise 401
//!   `{"type":"error","error":{"type":"authentication_error",…}}`.
//! - `GET /health` → `{"ok":true}`.
//! - `POST /v1/messages` — Anthropic Messages API exactly as Lynkr's `azure-anthropic` slot sends it, with `model`
//!   = a **Caret model id**. Unknown id → 404 `not_found_error` (so Lynkr's tier-fallback climbs). The body is
//!   parsed into an `LlmRequest` (`anthropic_in`), executed with Caret's own provider for that model, and answered
//!   as Anthropic SSE (`stream:true`, `anthropic_out::SseWriter`) or a full Message JSON. `message_start.message.model`
//!   is the Caret model id — the authoritative "served model" signal for the runner.
//! - Streaming responses commit to 200 only after the provider produced its first event; an immediate provider
//!   error becomes an HTTP status (429 / 503 / 502 …, see `anthropic_out::status_for`) so Lynkr can fall back.
//!   Failures after the first byte are `event: error` frames. `event: ping` every 15 s of silence.
//! - Thinking signatures / redacted data are emitted as `"<caret model id>|<opaque>"` and stripped (or the block
//!   dropped when it names another model) on the way back in.
//! - Client disconnect cancels the provider request. Provider keys never leave Caret.
pub mod anthropic_in;
pub mod anthropic_out;

use crate::ai::llm::{self, LlmEvent, LlmProvider};
use crate::core::settings::{ModelConfig, SettingsState};
use crate::error::{CaretError, Result};
use anthropic_out::{error_body, status_for, SseWriter};
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::body::{Body, Bytes, Frame, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Resolves a Caret model id into its config and a ready provider. Production uses settings; tests inject fakes.
pub type ModelFactory = Arc<dyn Fn(&str) -> Result<(ModelConfig, Arc<dyn LlmProvider>)> + Send + Sync>;

pub struct Bridge { pub port: u16, pub token: String, shutdown: CancellationToken }

struct State { token: String, factory: ModelFactory }

type Out = BoxBody<Bytes, Infallible>;

const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

impl Bridge {
    /// Production bridge: models come from Settings → Models, keys from the secrets store.
    pub async fn start(app: AppHandle) -> Result<Arc<Bridge>> {
        let factory: ModelFactory = Arc::new(move |id: &str| {
            let st = app.state::<SettingsState>();
            let settings = st.effective();
            if settings.model(id).is_none() { return Err(CaretError::not_found(format!("model {id}"))); }
            let resolved = llm::resolve_model(&settings, &st, Some(id))?;
            if resolved.model.id != id { return Err(CaretError::not_found(format!("model {id}"))); }
            let provider = llm::provider_for(&resolved)?;
            Ok((resolved.model, provider))
        });
        Self::start_with(factory).await
    }

    /// Bridge over an arbitrary model factory (tests inject an in-memory provider).
    pub async fn start_with(factory: ModelFactory) -> Result<Arc<Bridge>> {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| CaretError::io(format!("bridge bind: {e}")))?;
        let port = listener.local_addr().map_err(|e| CaretError::io(e.to_string()))?.port();
        let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        let shutdown = CancellationToken::new();
        let state = Arc::new(State { token: token.clone(), factory });
        let sd = shutdown.clone();
        tokio::spawn(async move {
            loop {
                let accepted = tokio::select! { _ = sd.cancelled() => break, a = listener.accept() => a };
                let Ok((stream, _)) = accepted else { continue };
                let st = state.clone();
                let sd2 = sd.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let svc = service_fn(move |req| handle(req, st.clone()));
                    let conn = http1::Builder::new().keep_alive(true).serve_connection(io, svc);
                    tokio::select! { _ = sd2.cancelled() => {}, r = conn => { if let Err(e) = r { tracing::debug!(error = %e, "bridge connection ended"); } } }
                });
            }
        });
        tracing::info!(port, "auto-router bridge listening on 127.0.0.1");
        Ok(Arc::new(Bridge { port, token, shutdown }))
    }

    pub fn messages_url(&self) -> String { format!("http://127.0.0.1:{}/v1/messages", self.port) }
    pub fn health_url(&self) -> String { format!("http://127.0.0.1:{}/health", self.port) }
    /// Stop accepting connections and end in-flight ones.
    pub fn shutdown(&self) { self.shutdown.cancel(); }
}
impl Drop for Bridge { fn drop(&mut self) { self.shutdown.cancel(); } }

// ───────────────────────── HTTP plumbing ─────────────────────────

/// Response body fed from a channel; dropping the receiver (client gone) makes the producer's `send` fail.
struct ChannelBody { rx: mpsc::Receiver<Bytes> }
impl Body for ChannelBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<std::result::Result<Frame<Bytes>, Infallible>>> {
        match self.get_mut().rx.poll_recv(cx) { Poll::Ready(Some(b)) => Poll::Ready(Some(Ok(Frame::data(b)))), Poll::Ready(None) => Poll::Ready(None), Poll::Pending => Poll::Pending }
    }
}

fn full(status: u16, content_type: &str, body: impl Into<Bytes>) -> Response<Out> {
    Response::builder().status(StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)).header("content-type", content_type).body(Full::new(body.into()).boxed()).unwrap()
}
fn json_response(status: u16, v: &Value) -> Response<Out> { full(status, "application/json", v.to_string()) }
fn error_response(e: &CaretError) -> Response<Out> { let (status, body) = error_body(e); json_response(status, &body) }

fn authorized(req: &Request<Incoming>, token: &str) -> bool {
    let h = req.headers();
    if h.get("x-api-key").and_then(|v| v.to_str().ok()) == Some(token) { return true; }
    h.get("authorization").and_then(|v| v.to_str().ok()).map(|v| { let v = v.trim(); v.len() > 7 && v[..7].eq_ignore_ascii_case("bearer ") && v[7..].trim() == token }).unwrap_or(false)
}

async fn handle(req: Request<Incoming>, st: Arc<State>) -> std::result::Result<Response<Out>, Infallible> {
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    if method == hyper::Method::GET && path == "/health" { return Ok(json_response(200, &json!({"ok": true}))); }
    if !authorized(&req, &st.token) { return Ok(json_response(401, &json!({"type": "error", "error": {"type": "authentication_error", "message": "invalid bridge token"}}))); }
    if method != hyper::Method::POST || path != "/v1/messages" { return Ok(json_response(404, &json!({"type": "error", "error": {"type": "not_found_error", "message": format!("{method} {path} is not served by the Caret bridge")}}))); }
    let bytes = match req.into_body().collect().await { Ok(c) => c.to_bytes(), Err(e) => return Ok(error_response(&CaretError::invalid(format!("could not read body: {e}")))) };
    let body: Value = match serde_json::from_slice(&bytes) { Ok(v) => v, Err(e) => return Ok(error_response(&CaretError::invalid(format!("invalid JSON: {e}")))) };
    let model_id = body.get("model").and_then(Value::as_str).unwrap_or("").to_string();
    if model_id.is_empty() { return Ok(error_response(&CaretError::invalid("`model` is required"))); }
    let (model, provider) = match (st.factory)(&model_id) { Ok(x) => x, Err(e) => return Ok(error_response(&e)) };
    let parsed = match anthropic_in::parse_request(&body, &model) { Ok(p) => p, Err(e) => return Ok(error_response(&e)) };
    Ok(if parsed.stream { stream_response(model, provider, parsed.req).await } else { full_response(model, provider, parsed.req).await })
}

async fn full_response(model: ModelConfig, provider: Arc<dyn LlmProvider>, req: llm::LlmRequest) -> Response<Out> {
    match llm::run_stream(provider, &req, CancellationToken::new(), |_| {}).await {
        Ok(turn) => json_response(200, &anthropic_out::message_json(&turn, &model.id)),
        Err(e) => error_response(&e),
    }
}

/// Map a provider's first (and only) `Error` event to an HTTP failure so Lynkr can climb tiers.
fn error_event_status(message: &str, retryable: bool) -> CaretError {
    let m = message.to_ascii_lowercase();
    if m.contains("429") || m.contains("rate limit") { CaretError::new("rate_limited", message) }
    else if retryable { CaretError::new("provider_unavailable", message) }
    else { CaretError::provider(message) }
}

/// Write frames to the response body; `false` when the client went away.
async fn send_all(btx: &mpsc::Sender<Bytes>, frames: Vec<String>) -> bool {
    for f in frames { if btx.send(Bytes::from(f)).await.is_err() { return false; } }
    true
}

async fn stream_response(model: ModelConfig, provider: Arc<dyn LlmProvider>, req: llm::LlmRequest) -> Response<Out> {
    let cancel = CancellationToken::new();
    let (etx, mut erx) = mpsc::channel::<LlmEvent>(256);
    let c2 = cancel.clone();
    let task = tokio::spawn(async move { provider.stream(&req, etx, c2).await });
    // Commit to a 200 only once the provider has produced something.
    let first = match erx.recv().await {
        Some(LlmEvent::Error { message, retryable }) => { cancel.cancel(); let _ = task.await; return error_response(&error_event_status(&message, retryable)); }
        Some(ev) => ev,
        None => { let err = match task.await { Ok(Err(e)) => e, Ok(Ok(())) => CaretError::new("provider_unavailable", "stream ended before completion"), Err(e) => CaretError::internal(format!("provider task failed: {e}")) }; return error_response(&err); }
    };
    let (btx, brx) = mpsc::channel::<Bytes>(64);
    let model_id = model.id.clone();
    tokio::spawn(async move {
        let mut w = SseWriter::new(&model_id);
        if !send_all(&btx, w.on_event(&first)).await { cancel.cancel(); return; }
        loop {
            let next = tokio::select! { ev = erx.recv() => Some(ev), _ = tokio::time::sleep(PING_INTERVAL) => None };
            match next {
                None => { if btx.send(Bytes::from(anthropic_out::ping_frame())).await.is_err() { cancel.cancel(); return; } }
                Some(None) => break,
                Some(Some(ev)) => { if !send_all(&btx, w.on_event(&ev)).await { cancel.cancel(); return; } if w.errored { break; } }
            }
        }
        if !w.stopped && !w.errored {
            let msg = match task.await { Ok(Err(e)) => e.message, Err(e) => format!("provider task failed: {e}"), Ok(Ok(())) => "stream ended before completion".into() };
            let _ = btx.send(Bytes::from(anthropic_out::error_frame(&msg, true))).await;
        }
    });
    Response::builder().status(200).header("content-type", "text/event-stream").header("cache-control", "no-cache").header("x-caret-bridge-model", model.id.as_str()).body(ChannelBody { rx: brx }.boxed()).unwrap()
}

// ───────────────────────── tests ─────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::llm::{ContentBlock, Effort, LlmRequest, Message, Role, StopReason, SystemBlock, ThinkingConfig, ToolDef, ToolResultContent, UsageDelta};
    use crate::ai::providers::anthropic::{build_body, translate_event};
    use crate::ai::providers::sse::SseParser;

    fn model(id: &str) -> ModelConfig { ModelConfig { id: id.into(), provider_id: "anthropic".into(), model: "claude-opus-5-5".into(), display_name: id.into(), context_window: 200_000, max_output: 8192, supports_tools: true, supports_vision: true, supports_thinking: true, supports_effort: true, enabled: true, ..Default::default() } }

    fn rich_request() -> LlmRequest {
        let mut req = LlmRequest::new(model("opus"));
        req.purpose = "auto-bridge".into();
        req.system = vec![SystemBlock { text: "You are Caret.".into(), cache: true }, SystemBlock { text: "Rules here".into(), cache: false }];
        req.messages = vec![
            Message { role: Role::User, content: vec![ContentBlock::Text { text: "look".into() }, ContentBlock::Image { media_type: "image/png".into(), data: "AAAA".into() }] },
            Message { role: Role::Assistant, content: vec![ContentBlock::Thinking { text: "hmm".into(), signature: Some("sigABC".into()) }, ContentBlock::Text { text: "Reading".into() }, ContentBlock::ToolUse { id: "t1".into(), name: "read_file".into(), input: json!({"path": "a.ts"}) }] },
            Message { role: Role::User, content: vec![ContentBlock::ToolResult { tool_use_id: "t1".into(), content: vec![ToolResultContent::Text { text: "contents".into() }, ToolResultContent::Image { media_type: "image/jpeg".into(), data: "BBBB".into() }], is_error: true }, ContentBlock::Text { text: "now what".into() }] },
        ];
        req.tools = vec![ToolDef { name: "read_file".into(), description: "Read".into(), input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}), strict: true }];
        req.thinking = ThinkingConfig { enabled: true, effort: Effort::Xhigh, summarized: true };
        req.max_tokens = 4000; req.stop = vec!["END".into()]; req.parallel_tool_calls = false; req.cache = true;
        req
    }

    #[test]
    fn request_round_trips_through_build_body() {
        let req = rich_request();
        let body = build_body(&req, false);
        let parsed = anthropic_in::parse_request(&body, &req.model).unwrap();
        assert!(parsed.stream, "build_body always streams");
        assert_eq!(serde_json::to_value(&parsed.req).unwrap(), serde_json::to_value(&req).unwrap());
    }

    #[test]
    fn signature_prefix_rules() {
        assert_eq!(anthropic_in::strip_signature("opus|sigABC", "opus").as_deref(), Some("sigABC"));
        assert_eq!(anthropic_in::strip_signature("haiku|sigABC", "opus"), None, "another model's block is dropped");
        assert_eq!(anthropic_in::strip_signature("sigABC", "opus").as_deref(), Some("sigABC"), "unprefixed kept");
        assert_eq!(anthropic_in::strip_signature("abc/def+g|x", "opus").as_deref(), Some("abc/def+g|x"), "base64-ish prefix is not a model id");
        let body = json!({"model": "opus", "max_tokens": 10, "messages": [{"role": "assistant", "content": [{"type": "thinking", "thinking": "a", "signature": "haiku|s"}, {"type": "redacted_thinking", "data": "opus|d"}, {"type": "text", "text": "ok"}]}]});
        let parsed = anthropic_in::parse_request(&body, &model("opus")).unwrap();
        assert_eq!(serde_json::to_value(&parsed.req.messages[0].content).unwrap(), json!([{"type": "redacted_thinking", "data": "d"}, {"type": "text", "text": "ok"}]));
    }

    #[test]
    fn events_round_trip_through_sse() {
        let events = vec![
            LlmEvent::MessageStart { model: "ignored".into() }, LlmEvent::Usage(UsageDelta { input_tokens: 10, cache_read_tokens: 3, ..Default::default() }),
            LlmEvent::TextDelta { index: 0, text: "Hel".into() }, LlmEvent::TextDelta { index: 0, text: "lo".into() }, LlmEvent::BlockStop { index: 0 },
            LlmEvent::ThinkingDelta { index: 1, text: "hmm".into() }, LlmEvent::ThinkingSignature { index: 1, signature: "sig".into() }, LlmEvent::BlockStop { index: 1 },
            LlmEvent::RedactedThinking { index: 2, data: "red".into() },
            LlmEvent::ToolUseStart { index: 3, id: "t1".into(), name: "read".into() }, LlmEvent::ToolInputDelta { index: 3, partial_json: "{\"a\":".into() }, LlmEvent::ToolInputDelta { index: 3, partial_json: "1}".into() }, LlmEvent::BlockStop { index: 3 },
            LlmEvent::Usage(UsageDelta { output_tokens: 5, ..Default::default() }), LlmEvent::Stop { reason: StopReason::ToolUse, detail: None },
        ];
        let mut w = SseWriter::new("fake-a");
        let sse: String = events.iter().flat_map(|e| w.on_event(e)).collect();
        assert!(w.stopped);
        let mut p = SseParser::new(); let mut sent = false;
        let got: Vec<LlmEvent> = p.feed(sse.as_bytes()).iter().flat_map(|ev| translate_event(ev.event.as_deref(), &ev.data, &mut sent)).collect();
        let mut expected = events.clone();
        expected[0] = LlmEvent::MessageStart { model: "fake-a".into() };
        expected[6] = LlmEvent::ThinkingSignature { index: 1, signature: "fake-a|sig".into() };
        expected[8] = LlmEvent::RedactedThinking { index: 2, data: "fake-a|red".into() };
        // Like the real API, a redacted block is announced as start+stop, so the reader sees one extra BlockStop.
        expected.insert(9, LlmEvent::BlockStop { index: 2 });
        assert_eq!(serde_json::to_value(&got).unwrap(), serde_json::to_value(&expected).unwrap());
    }

    #[test]
    fn late_usage_is_reported_in_message_delta_when_unknown_at_start() {
        let mut w = SseWriter::new("m");
        let sse: String = [LlmEvent::TextDelta { index: 0, text: "x".into() }, LlmEvent::Usage(UsageDelta { input_tokens: 7, output_tokens: 2, ..Default::default() }), LlmEvent::Stop { reason: StopReason::EndTurn, detail: Some("done".into()) }].iter().flat_map(|e| w.on_event(e)).collect();
        assert!(!sse.contains("\"usage\":{\"input_tokens\":0"), "no usage in message_start when unknown");
        let mut p = SseParser::new(); let mut sent = false;
        let got: Vec<LlmEvent> = p.feed(sse.as_bytes()).iter().flat_map(|ev| translate_event(ev.event.as_deref(), &ev.data, &mut sent)).collect();
        let usage: Vec<&UsageDelta> = got.iter().filter_map(|e| if let LlmEvent::Usage(u) = e { Some(u) } else { None }).collect();
        assert_eq!((usage.len(), usage[0].input_tokens, usage[0].output_tokens), (1, 7, 2));
        assert!(matches!(got.last(), Some(LlmEvent::Stop { reason: StopReason::EndTurn, detail: Some(d) }) if d == "done"));
    }

    #[test]
    fn error_status_mapping() {
        let s = |e: CaretError| status_for(&e).0;
        assert_eq!(s(CaretError::new("rate_limited", "Rate limited (429)")), 429);
        assert_eq!(s(CaretError::provider("HTTP 429: slow down")), 429);
        assert_eq!(s(CaretError::new("provider_unavailable", "Provider error (502)")), 503);
        assert_eq!(s(CaretError::new("timeout", "t")), 503);
        assert_eq!(s(CaretError::new("connect", "c")), 503);
        assert_eq!(s(CaretError::provider("stream ended before completion")), 503);
        assert_eq!(s(CaretError::provider("Invalid API key or unauthorized (401)")), 502, "never forward 401 upstream");
        assert_eq!(s(CaretError::config("no key")), 502);
        assert_eq!(s(CaretError::invalid("bad")), 400);
        assert_eq!(s(CaretError::new("json", "x")), 400);
        assert_eq!(s(CaretError::not_found("model x")), 404);
        assert_eq!(s(CaretError::cancelled()), 499);
        assert_eq!(status_for(&CaretError::new("provider_unavailable", "x")).1, "overloaded_error");
    }

    // ── end to end over real HTTP with a scripted provider ──
    struct Fake { script: Vec<LlmEvent> }
    #[async_trait::async_trait]
    impl LlmProvider for Fake {
        async fn stream(&self, _req: &LlmRequest, tx: mpsc::Sender<LlmEvent>, _cancel: CancellationToken) -> Result<()> {
            for ev in &self.script { let is_err = matches!(ev, LlmEvent::Error { .. }); let msg = if let LlmEvent::Error { message, .. } = ev { message.clone() } else { String::new() }; let _ = tx.send(ev.clone()).await; if is_err { return Err(CaretError::provider(msg)); } }
            Ok(())
        }
        async fn list_models(&self) -> Result<Vec<String>> { Ok(vec![]) }
        async fn embed(&self, _m: &str, _i: &[String]) -> Result<Vec<Vec<f32>>> { Ok(vec![]) }
        fn name(&self) -> &'static str { "fake" }
    }
    fn script_ok() -> Vec<LlmEvent> { vec![LlmEvent::MessageStart { model: "real-upstream".into() }, LlmEvent::Usage(UsageDelta { input_tokens: 12, ..Default::default() }), LlmEvent::TextDelta { index: 0, text: "Hello".into() }, LlmEvent::BlockStop { index: 0 }, LlmEvent::ToolUseStart { index: 1, id: "t1".into(), name: "read".into() }, LlmEvent::ToolInputDelta { index: 1, partial_json: "{}".into() }, LlmEvent::BlockStop { index: 1 }, LlmEvent::Usage(UsageDelta { output_tokens: 4, ..Default::default() }), LlmEvent::Stop { reason: StopReason::ToolUse, detail: None }] }
    fn factory() -> ModelFactory {
        Arc::new(|id: &str| match id {
            "fake-a" => Ok((model("fake-a"), Arc::new(Fake { script: script_ok() }) as Arc<dyn LlmProvider>)),
            "fake-429" => Ok((model("fake-429"), Arc::new(Fake { script: vec![LlmEvent::Error { message: "Rate limited (429): slow".into(), retryable: true }] }) as Arc<dyn LlmProvider>)),
            "fake-down" => Ok((model("fake-down"), Arc::new(Fake { script: vec![LlmEvent::Error { message: "connection refused".into(), retryable: true }] }) as Arc<dyn LlmProvider>)),
            other => Err(CaretError::not_found(format!("model {other}"))),
        })
    }
    fn body(model: &str, stream: bool) -> Value { json!({"model": model, "max_tokens": 100, "stream": stream, "messages": [{"role": "user", "content": "hi"}]}) }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn serves_messages_over_http() {
        let bridge = Bridge::start_with(factory()).await.unwrap();
        let client = reqwest::Client::new();
        let url = bridge.messages_url();
        // health, auth
        assert_eq!(client.get(bridge.health_url()).send().await.unwrap().status(), 200);
        let r = client.post(&url).header("x-api-key", "wrong").json(&body("fake-a", true)).send().await.unwrap();
        assert_eq!(r.status(), 401);
        let r = client.post(&url).bearer_auth(&bridge.token).json(&body("nope", true)).send().await.unwrap();
        assert_eq!(r.status(), 404); assert_eq!(r.json::<Value>().await.unwrap()["error"]["type"], "not_found_error");
        // streaming happy path
        let r = client.post(&url).header("x-api-key", &bridge.token).json(&body("fake-a", true)).send().await.unwrap();
        assert_eq!(r.status(), 200); assert_eq!(r.headers().get("content-type").unwrap(), "text/event-stream");
        let text = r.text().await.unwrap();
        let mut p = SseParser::new(); let mut sent = false;
        let got: Vec<LlmEvent> = p.feed(text.as_bytes()).iter().flat_map(|ev| translate_event(ev.event.as_deref(), &ev.data, &mut sent)).collect();
        assert!(matches!(&got[0], LlmEvent::MessageStart { model } if model == "fake-a"), "served model is the Caret id, not the upstream name: {got:?}");
        assert!(matches!(&got[1], LlmEvent::Usage(u) if u.input_tokens == 12));
        assert!(got.iter().any(|e| matches!(e, LlmEvent::TextDelta { text, .. } if text == "Hello")));
        assert!(got.iter().any(|e| matches!(e, LlmEvent::ToolUseStart { name, .. } if name == "read")));
        assert!(matches!(got.last(), Some(LlmEvent::Stop { reason: StopReason::ToolUse, .. })));
        assert!(text.contains("event: message_stop"));
        // immediate provider failures become HTTP statuses (so Lynkr can climb)
        assert_eq!(client.post(&url).header("x-api-key", &bridge.token).json(&body("fake-429", true)).send().await.unwrap().status(), 429);
        assert_eq!(client.post(&url).header("x-api-key", &bridge.token).json(&body("fake-down", true)).send().await.unwrap().status(), 503);
        // non-stream
        let r = client.post(&url).header("x-api-key", &bridge.token).json(&body("fake-a", false)).send().await.unwrap();
        assert_eq!(r.status(), 200);
        let v: Value = r.json().await.unwrap();
        assert_eq!(v["model"], "fake-a"); assert_eq!(v["stop_reason"], "tool_use"); assert_eq!(v["usage"]["input_tokens"], 12); assert_eq!(v["usage"]["output_tokens"], 4);
        assert_eq!(v["content"][0], json!({"type": "text", "text": "Hello"})); assert_eq!(v["content"][1]["name"], "read");
        // bad json / missing model
        assert_eq!(client.post(&url).header("x-api-key", &bridge.token).body("{nope").header("content-type", "application/json").send().await.unwrap().status(), 400);
        assert_eq!(client.post(&url).header("x-api-key", &bridge.token).json(&json!({"messages": []})).send().await.unwrap().status(), 400);
        bridge.shutdown();
    }
}

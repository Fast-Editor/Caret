//! Loopback MCP server: Streamable HTTP transport (spec revision 2025-06-18) in **stateless** mode.
//!
//! Contract (`http://127.0.0.1:<port>`):
//! - `GET /health` (unauthenticated) -> `{ ok, name: "caret-orchestrator", version }`.
//! - Every other route needs `Authorization: Bearer <token>` or `x-api-key: <token>`; otherwise 401.
//! - `POST /mcp` with a JSON-RPC 2.0 body (single request/notification or a batch). Responses are plain
//!   `application/json` (one object, or an array for batches); a body made only of notifications gets `202 Accepted`
//!   with no content. No SSE stream is ever opened and no session id is issued: every request is self-contained.
//! - `GET /mcp` -> 405 (no server-initiated stream), `DELETE /mcp` -> 200 (nothing to tear down).
//! - Methods: `initialize`, `ping`, `tools/list`, `tools/call`, `notifications/*` (accepted silently); anything
//!   else is JSON-RPC `-32601 Method not found`.
use crate::error::{CaretError, Result};
use crate::orchestrator::tools::{self, Ctx};
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::body::{Body, Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub const SERVER_NAME: &str = "caret";
pub const HEALTH_NAME: &str = "caret-orchestrator";
pub const LATEST_PROTOCOL: &str = "2025-06-18";
pub const SUPPORTED_PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
/// How many ports after the preferred one are tried when it is busy.
pub const PORT_ATTEMPTS: u16 = 10;
const MAX_BODY: usize = 16 * 1024 * 1024;

pub struct Server { pub port: u16, pub token: String, pub started_at: u64, shutdown: CancellationToken }

struct State { ctx: Arc<Ctx>, token: String }

type Out = BoxBody<Bytes, Infallible>;

impl Server {
    /// Bind `127.0.0.1:<preferred>` (or the next `PORT_ATTEMPTS` ports) and serve until `shutdown()`.
    pub async fn start(ctx: Arc<Ctx>, token: String, preferred: u16) -> Result<Arc<Server>> {
        let mut listener = None; let mut last_err = None;
        for off in 0..=PORT_ATTEMPTS {
            let Some(port) = preferred.checked_add(off) else { break };
            match tokio::net::TcpListener::bind(("127.0.0.1", port)).await { Ok(l) => { listener = Some(l); break; } Err(e) => last_err = Some(e) }
        }
        let listener = listener.ok_or_else(|| CaretError::io(format!("orchestrator: no free port in {preferred}..={} ({})", preferred.saturating_add(PORT_ATTEMPTS), last_err.map(|e| e.to_string()).unwrap_or_default())))?;
        let port = listener.local_addr().map_err(|e| CaretError::io(e.to_string()))?.port();
        let shutdown = CancellationToken::new();
        let state = Arc::new(State { ctx, token: token.clone() });
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
                    tokio::select! { _ = sd2.cancelled() => {}, r = conn => { if let Err(e) = r { tracing::debug!(error = %e, "orchestrator connection ended"); } } }
                });
            }
        });
        tracing::info!(port, "orchestrator MCP server listening on 127.0.0.1");
        Ok(Arc::new(Server { port, token, started_at: crate::ai::agent::session::now_ms(), shutdown }))
    }
    pub fn url(&self) -> String { mcp_url(self.port) }
    pub fn health_url(&self) -> String { format!("http://127.0.0.1:{}/health", self.port) }
    pub fn shutdown(&self) { self.shutdown.cancel(); }
}
impl Drop for Server { fn drop(&mut self) { self.shutdown.cancel(); } }

pub fn mcp_url(port: u16) -> String { format!("http://127.0.0.1:{port}/mcp") }

// ───────────────────────── HTTP plumbing ─────────────────────────

fn full(status: u16, content_type: Option<&str>, body: impl Into<Bytes>) -> Response<Out> {
    let mut b = Response::builder().status(StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR));
    if let Some(ct) = content_type { b = b.header("content-type", ct); }
    b.body(Full::new(body.into()).boxed()).unwrap()
}
fn json_response(status: u16, v: &Value) -> Response<Out> { full(status, Some("application/json"), v.to_string()) }

fn authorized(req: &Request<Incoming>, token: &str) -> bool {
    let h = req.headers();
    if h.get("x-api-key").and_then(|v| v.to_str().ok()) == Some(token) { return true; }
    h.get("authorization").and_then(|v| v.to_str().ok()).map(|v| { let v = v.trim(); v.len() > 7 && v[..7].eq_ignore_ascii_case("bearer ") && v[7..].trim() == token }).unwrap_or(false)
}

async fn handle(req: Request<Incoming>, st: Arc<State>) -> std::result::Result<Response<Out>, Infallible> {
    let path = req.uri().path().trim_end_matches('/').to_string();
    let method = req.method().clone();
    if method == Method::GET && path == "/health" { return Ok(json_response(200, &json!({"ok": true, "name": HEALTH_NAME, "version": env!("CARGO_PKG_VERSION")}))); }
    if !authorized(&req, &st.token) { return Ok(json_response(401, &json!({"ok": false, "error": "unauthorized", "message": "missing or invalid bearer token (Settings > MCP > Caret as an MCP server)"}))); }
    if path != "/mcp" { return Ok(json_response(404, &json!({"ok": false, "error": "not_found", "message": format!("{method} {path} is not served; use POST /mcp")}))); }
    match method {
        Method::POST => {}
        Method::DELETE => return Ok(full(200, None, "")),
        Method::GET => return Ok(Response::builder().status(405).header("allow", "POST, DELETE").header("content-type", "application/json").body(Full::new(Bytes::from(json!({"ok": false, "error": "method_not_allowed", "message": "this server is stateless and opens no SSE stream; send JSON-RPC via POST"}).to_string())).boxed()).unwrap()),
        _ => return Ok(Response::builder().status(405).header("allow", "POST, DELETE").body(Full::new(Bytes::new()).boxed()).unwrap()),
    }
    if req.body().size_hint().upper().map(|n| n as usize > MAX_BODY).unwrap_or(false) { return Ok(json_response(413, &rpc_error(Value::Null, -32600, "request body too large"))); }
    let bytes = match req.into_body().collect().await { Ok(c) => c.to_bytes(), Err(e) => return Ok(json_response(400, &rpc_error(Value::Null, -32700, &format!("could not read body: {e}")))) };
    let body: Value = match serde_json::from_slice(&bytes) { Ok(v) => v, Err(e) => return Ok(json_response(400, &rpc_error(Value::Null, -32700, &format!("invalid JSON: {e}")))) };
    Ok(match handle_jsonrpc(&st.ctx, body).await { Some(v) => json_response(200, &v), None => full(202, None, "") })
}

// ───────────────────────── JSON-RPC ─────────────────────────

fn rpc_error(id: Value, code: i64, message: &str) -> Value { json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}) }
fn rpc_result(id: Value, result: Value) -> Value { json!({"jsonrpc": "2.0", "id": id, "result": result}) }

/// Handle one body (single message or batch). `None` means "nothing to answer" (notifications only) -> 202.
pub async fn handle_jsonrpc(ctx: &Arc<Ctx>, body: Value) -> Option<Value> {
    match body {
        Value::Array(items) => {
            if items.is_empty() { return Some(rpc_error(Value::Null, -32600, "empty batch")); }
            let mut out = vec![];
            for item in items { if let Some(r) = handle_message(ctx, item).await { out.push(r); } }
            if out.is_empty() { None } else { Some(Value::Array(out)) }
        }
        other => handle_message(ctx, other).await,
    }
}

async fn handle_message(ctx: &Arc<Ctx>, msg: Value) -> Option<Value> {
    let Some(obj) = msg.as_object() else { return Some(rpc_error(Value::Null, -32600, "request must be a JSON object")) };
    let id = obj.get("id").cloned().filter(|v| !v.is_null());
    let Some(method) = obj.get("method").and_then(Value::as_str).map(String::from) else {
        // A response or malformed message: nothing we can act on.
        return id.map(|id| rpc_error(id, -32600, "missing `method`"));
    };
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    let Some(id) = id else {
        // Notification (`notifications/initialized`, `notifications/cancelled`, ...): accept silently.
        if !method.starts_with("notifications/") { tracing::debug!(method, "orchestrator: ignoring notification"); }
        return None;
    };
    Some(dispatch(ctx, &method, params, id).await)
}

pub fn instructions() -> String {
    "Caret is a local AI code editor. These tools let you spawn and steer Caret chat threads on any configured model provider or external agent: \
`list_agents` shows what can drive a thread; `create_thread` opens one (optionally bound to an ACP agent such as Claude Code or Codex, or to a git worktree); \
`send` posts a user message and, by default, waits for the reply (returns assistant_text, files_changed, usage and any pending approval or question); \
`wait` resumes waiting; `read_thread` pages through the transcript; `pending`, `approve` and `answer` handle approvals and questions the agent raises; \
`stop` cancels a turn and `delete_thread` removes a thread. The human sees every thread live in Caret and can also answer approvals there. \
Thread ids are opaque strings; keep them to continue a conversation.".to_string()
}

async fn dispatch(ctx: &Arc<Ctx>, method: &str, params: Value, id: Value) -> Value {
    match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(LATEST_PROTOCOL);
            let version = if SUPPORTED_PROTOCOLS.contains(&requested) { requested } else { LATEST_PROTOCOL };
            rpc_result(id, json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": SERVER_NAME, "title": "Caret", "version": env!("CARGO_PKG_VERSION") },
                "instructions": instructions(),
            }))
        }
        "ping" => rpc_result(id, json!({})),
        "tools/list" => rpc_result(id, json!({ "tools": tools::specs() })),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else { return rpc_error(id, -32602, "`params.name` is required") };
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            match tools::call(ctx, name, &args).await {
                Ok(v) => rpc_result(id, json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()) }], "structuredContent": v, "isError": false })),
                Err(e) => rpc_result(id, json!({ "content": [{ "type": "text", "text": format!("{} ({})", e.message, e.code) }], "isError": true })),
            }
        }
        _ => rpc_error(id, -32601, &format!("Method not found: {method}")),
    }
}

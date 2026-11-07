//! HTTP + WebSocket server for `caret serve` (hyper 1, same shape as `ai::router::bridge`).
//!
//! Routes:
//! * `GET /health` → `{ ok, name: "caret", version, paired }` (no auth)
//! * `POST /pair { code, device }` → `{ token, device }`; 403 bad/expired code, 429 too many attempts (no auth)
//! * `GET /__remote/pair.html` → pairing page (no auth)
//! * `GET /ws?role=client&token=…` → relay socket for a paired browser (401 unknown token)
//! * `GET /ws?role=executor&secret=…` → the desktop webview's executor socket (loopback only, per-process secret)
//! * `GET /__remote/shim.js`, `GET /` and the Vite assets → the frontend, only with a valid `?token=` or
//!   `caret_token` cookie; `/` without one serves the pairing page. `index.html` gets the shim injected.
//!
//! Text frames are capped at 32 MB; the server pings every 20 s.
use super::pairing::{DeviceStore, PairError, Pairing};
use super::relay::Hub;
use crate::error::{CaretError, Result};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::{Role, WebSocketConfig};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;
use tokio_util::sync::CancellationToken;

pub const SHIM_JS: &str = include_str!("../../remote/shim.js");
pub const PAIR_HTML: &str = include_str!("../../remote/pair.html");
const NO_BUILD_HTML: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Caret remote</title></head><body style=\"font-family:system-ui;background:#161618;color:#e7e7ea;padding:40px\"><h2>Caret remote: frontend not built</h2><p>This Caret was started from a development checkout and <code>dist/</code> does not exist. Run <code>npm run build</code> in the Caret checkout (or use a packaged Caret.app) and restart <code>caret serve</code>.</p></body></html>";
pub const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;
const PING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(20);
const COOKIE: &str = "caret_token";

type Out = BoxBody<Bytes, Infallible>;

/// Where the frontend comes from: the embedded Tauri assets in production, `<repo>/dist` in dev, a map in tests.
pub trait AssetSource: Send + Sync {
    /// `path` starts with `/` (e.g. `/index.html`, `/assets/index-abc.js`). Returns bytes and a mime type.
    fn get(&self, path: &str) -> Option<(Vec<u8>, String)>;
}

impl AssetSource for HashMap<String, (Vec<u8>, String)> {
    fn get(&self, path: &str) -> Option<(Vec<u8>, String)> { HashMap::get(self, path).cloned() }
}

pub struct Deps {
    pub assets: Arc<dyn AssetSource>,
    pub pairing: Arc<Pairing>,
    pub devices: Arc<DeviceStore>,
    pub hub: Arc<Hub>,
    pub executor_secret: String,
    pub version: String,
    /// Fired after a pairing (the UI refreshes its device list).
    pub on_change: Option<Arc<dyn Fn() + Send + Sync>>,
}
impl Deps { fn changed(&self) { if let Some(h) = &self.on_change { h(); } } }

pub struct RemoteServer { pub host: String, pub port: u16, shutdown: CancellationToken, accept_task: parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>> }

impl RemoteServer {
    pub async fn start(host: &str, port: u16, deps: Deps) -> Result<Arc<RemoteServer>> {
        let listener = tokio::net::TcpListener::bind((host, port)).await.map_err(|e| CaretError::io(format!("remote: cannot listen on {host}:{port}: {e}")))?;
        let port = listener.local_addr().map_err(|e| CaretError::io(e.to_string()))?.port();
        let shutdown = CancellationToken::new();
        let deps = Arc::new(deps);
        let sd = shutdown.clone();
        let accept_task = tokio::spawn(async move {
            loop {
                let accepted = tokio::select! { _ = sd.cancelled() => break, a = listener.accept() => a };
                let Ok((stream, peer)) = accepted else { continue };
                let deps = deps.clone();
                let sd_conn = sd.clone();
                let sd_svc = sd.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let svc = service_fn(move |req| handle(req, deps.clone(), peer, sd_svc.clone()));
                    let conn = http1::Builder::new().keep_alive(true).serve_connection(io, svc).with_upgrades();
                    tokio::select! { _ = sd_conn.cancelled() => {}, r = conn => { if let Err(e) = r { tracing::debug!(error = %e, "remote: connection ended"); } } }
                });
            }
        });
        tracing::info!(host, port, "remote: listening");
        Ok(Arc::new(RemoteServer { host: host.to_string(), port, shutdown, accept_task: parking_lot::Mutex::new(Some(accept_task)) }))
    }
    pub fn shutdown(&self) { self.shutdown.cancel(); }
    /// Stop and wait until the listening socket is closed (so the port can be re-bound right away).
    pub async fn shutdown_and_wait(&self) {
        self.shutdown.cancel();
        let task = self.accept_task.lock().take();
        if let Some(t) = task { let _ = t.await; }
    }
    pub fn is_running(&self) -> bool { !self.shutdown.is_cancelled() }
}
impl Drop for RemoteServer { fn drop(&mut self) { self.shutdown.cancel(); } }

// ───────────────────────── addresses ─────────────────────────

/// Non-loopback IPv4 addresses of this machine (LAN, Tailscale `100.x`), sorted and deduplicated.
pub fn lan_ipv4s() -> Vec<Ipv4Addr> {
    let mut v: Vec<Ipv4Addr> = local_ip_address::list_afinet_netifas().map(|list| list.into_iter().filter_map(|(_, ip)| match ip {
        IpAddr::V4(a) if !a.is_loopback() && !a.is_link_local() && !a.is_unspecified() && !a.is_broadcast() => Some(a),
        _ => None,
    }).collect()).unwrap_or_default();
    v.sort(); v.dedup();
    v
}

/// Browser URLs for a bind address: every LAN IPv4 plus loopback for `0.0.0.0`, the address itself otherwise.
pub fn urls_for(host: &str, port: u16) -> Vec<String> {
    match host {
        "0.0.0.0" | "::" | "" | "*" => { let mut v: Vec<String> = lan_ipv4s().into_iter().map(|ip| format!("http://{ip}:{port}")).collect(); v.push(format!("http://127.0.0.1:{port}")); v }
        h => vec![format!("http://{h}:{port}")],
    }
}

// ───────────────────────── HTML rewriting ─────────────────────────

/// Insert the shim as the first child of `<head>` (and `<base href="/">` when asset URLs are relative).
pub fn inject_shim(html: &str) -> String {
    let tag = "<script src=\"/__remote/shim.js\"></script>";
    let relative = ["src=\"./", "href=\"./", "src=\"assets/", "href=\"assets/", "src='./", "href='./"].iter().any(|p| html.contains(p));
    let inject = if relative && !html.contains("<base ") { format!("<base href=\"/\">{tag}") } else { tag.to_string() };
    let head_at = html.find("<head>").or_else(|| html.find("<head "));
    if let Some(start) = head_at {
        if let Some(close) = html[start..].find('>') {
            let at = start + close + 1;
            return format!("{}{}{}", &html[..at], inject, &html[at..]);
        }
    }
    if let Some(start) = html.find("<html") {
        if let Some(close) = html[start..].find('>') {
            let at = start + close + 1;
            return format!("{}<head>{}</head>{}", &html[..at], inject, &html[at..]);
        }
    }
    format!("{inject}{html}")
}

// ───────────────────────── HTTP plumbing ─────────────────────────

fn full(status: u16, content_type: &str, body: impl Into<Bytes>) -> Response<Out> {
    Response::builder().status(StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)).header("content-type", content_type).header("x-content-type-options", "nosniff").body(Full::new(body.into()).boxed()).unwrap()
}
fn json(status: u16, v: &Value) -> Response<Out> { full(status, "application/json", v.to_string()) }
fn html(status: u16, body: &'static str) -> Response<Out> { let mut r = full(status, "text/html; charset=utf-8", body); r.headers_mut().insert("cache-control", "no-store".parse().unwrap()); r }

fn parse_query(q: Option<&str>) -> HashMap<String, String> {
    q.map(|q| url::form_urlencoded::parse(q.as_bytes()).into_owned().collect()).unwrap_or_default()
}

fn client_token(req: &Request<Incoming>, query: &HashMap<String, String>) -> Option<String> {
    if let Some(t) = query.get("token") { if !t.is_empty() { return Some(t.clone()); } }
    for c in req.headers().get_all("cookie") {
        let Ok(s) = c.to_str() else { continue };
        for part in s.split(';') {
            if let Some(v) = part.trim().strip_prefix(COOKIE).and_then(|r| r.strip_prefix('=')) {
                let v = v.trim().trim_matches('"');
                if !v.is_empty() { return Some(v.to_string()); }
            }
        }
    }
    None
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Paths whose last segment has an extension are files; everything else is the SPA entry.
fn is_asset_path(path: &str) -> bool { path.rsplit('/').next().map(|seg| seg.contains('.')).unwrap_or(false) }

async fn handle(mut req: Request<Incoming>, deps: Arc<Deps>, peer: SocketAddr, shutdown: CancellationToken) -> std::result::Result<Response<Out>, Infallible> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = parse_query(req.uri().query());
    let res = match (method, path.as_str()) {
        (Method::GET, "/health") => json(200, &json!({ "ok": true, "name": "caret", "version": deps.version, "paired": deps.devices.len() })),
        (Method::POST, "/pair") => pair(req, &deps, peer).await,
        (Method::GET, "/__remote/pair.html") => html(200, PAIR_HTML),
        (Method::GET, "/ws") => ws_upgrade(&mut req, &query, &deps, peer, shutdown),
        (Method::GET, _) | (Method::HEAD, _) => serve_static(&req, &path, &query, &deps),
        _ => json(405, &json!({ "error": "method not allowed" })),
    };
    Ok(res)
}

async fn pair(req: Request<Incoming>, deps: &Deps, peer: SocketAddr) -> Response<Out> {
    let ua = req.headers().get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let body = match http_body_util::Limited::new(req.into_body(), 64 * 1024).collect().await { Ok(c) => c.to_bytes(), Err(_) => return json(400, &json!({ "error": "could not read body" })) };
    let v: Value = match serde_json::from_slice(&body) { Ok(v) => v, Err(_) => return json(400, &json!({ "error": "invalid JSON" })) };
    let code = v.get("code").and_then(Value::as_str).unwrap_or("");
    let device = v.get("device").and_then(Value::as_str).unwrap_or("Browser");
    match deps.pairing.redeem(code, peer.ip()) {
        Err(PairError::RateLimited) => json(429, &json!({ "error": "Too many attempts. Wait a minute and try again." })),
        Err(PairError::Invalid) => { tracing::warn!(peer = %peer.ip(), "remote: rejected pairing attempt"); json(403, &json!({ "error": "Invalid or expired pairing code." })) }
        Ok(()) => match deps.devices.add(device, &ua) {
            Ok((info, token)) => { tracing::info!(peer = %peer.ip(), device = %info.name, "remote: paired a new device"); deps.changed(); json(200, &json!({ "token": token, "device": info })) }
            Err(e) => json(500, &json!({ "error": e.message })),
        },
    }
}

/// `..` segments, raw or percent-encoded, never reach the asset lookup (the embedded map is safe, the dev `dist/`
/// fallback is a real directory).
fn is_traversal(path: &str) -> bool { let lower = path.to_ascii_lowercase(); lower.contains("..") || lower.contains("%2e") || lower.contains('\\') }

fn serve_static(req: &Request<Incoming>, path: &str, query: &HashMap<String, String>, deps: &Deps) -> Response<Out> {
    if is_traversal(path) { return json(400, &json!({ "error": "bad path" })); }
    let token = client_token(req, query);
    let device = token.as_deref().and_then(|t| deps.devices.authenticate(t));
    let is_asset = is_asset_path(path);
    if device.is_none() {
        if is_asset { return json(401, &json!({ "error": "pair this browser first" })); }
        return html(200, PAIR_HTML);
    }
    if path == "/__remote/shim.js" { return full(200, "application/javascript; charset=utf-8", SHIM_JS); }
    if !is_asset {
        let page = match deps.assets.get("/index.html") { Some((bytes, _)) => inject_shim(&String::from_utf8_lossy(&bytes)), None => NO_BUILD_HTML.to_string() };
        let mut res = full(200, "text/html; charset=utf-8", page);
        res.headers_mut().insert("cache-control", "no-store".parse().unwrap());
        if let (true, Some(t)) = (query.contains_key("token"), token.as_deref()) {
            if let Ok(v) = format!("{COOKIE}={t}; Path=/; SameSite=Lax; Max-Age=31536000").parse() { res.headers_mut().insert("set-cookie", v); }
        }
        return res;
    }
    match deps.assets.get(path) {
        Some((bytes, mime)) => {
            let mut res = full(200, &mime, bytes);
            let cache = if path.starts_with("/assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
            res.headers_mut().insert("cache-control", cache.parse().unwrap());
            res
        }
        None => json(404, &json!({ "error": "not found" })),
    }
}

// ───────────────────────── WebSocket relay ─────────────────────────

enum WsRole { Executor, Client(String) }

fn ws_upgrade(req: &mut Request<Incoming>, query: &HashMap<String, String>, deps: &Arc<Deps>, peer: SocketAddr, shutdown: CancellationToken) -> Response<Out> {
    let role = match query.get("role").map(String::as_str) {
        Some("executor") => {
            let secret_ok = query.get("secret").map(|s| constant_time_eq(s, &deps.executor_secret)).unwrap_or(false);
            if !peer.ip().is_loopback() || !secret_ok { return json(401, &json!({ "error": "bad executor secret" })); }
            WsRole::Executor
        }
        Some("client") => match client_token(req, query).and_then(|t| deps.devices.authenticate(&t)) {
            Some(d) => WsRole::Client(d.id),
            None => return json(401, &json!({ "error": "unknown token" })),
        },
        _ => return json(400, &json!({ "error": "role must be client or executor" })),
    };
    let is_upgrade = req.headers().get("upgrade").and_then(|v| v.to_str().ok()).map(|v| v.eq_ignore_ascii_case("websocket")).unwrap_or(false);
    let key = req.headers().get("sec-websocket-key").and_then(|v| v.to_str().ok()).map(str::to_string);
    let (true, Some(key)) = (is_upgrade, key) else { return json(400, &json!({ "error": "not a websocket handshake" })) };
    let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());
    let on_upgrade = hyper::upgrade::on(req);
    let deps = deps.clone();
    tokio::spawn(async move {
        match on_upgrade.await {
            Ok(upgraded) => {
                let cfg = WebSocketConfig::default().max_message_size(Some(MAX_FRAME_BYTES)).max_frame_size(Some(MAX_FRAME_BYTES));
                let ws = WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Server, Some(cfg)).await;
                run_ws(ws, role, deps, peer, shutdown).await;
            }
            Err(e) => tracing::debug!(error = %e, "remote: websocket upgrade failed"),
        }
    });
    Response::builder().status(101).header("upgrade", "websocket").header("connection", "Upgrade").header("sec-websocket-accept", accept).body(Empty::new().boxed()).unwrap()
}

async fn run_ws<S: AsyncRead + AsyncWrite + Unpin>(ws: WebSocketStream<S>, role: WsRole, deps: Arc<Deps>, peer: SocketAddr, shutdown: CancellationToken) {
    enum Reg { Executor(u64), Client(u64) }
    let (mut sink, mut stream) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let hub = deps.hub.clone();
    let reg = match &role {
        WsRole::Executor => { tracing::info!("remote: desktop executor connected"); Reg::Executor(hub.attach_executor(tx)) }
        WsRole::Client(device) => { tracing::info!(peer = %peer.ip(), device, "remote: client connected"); Reg::Client(hub.add_client(tx, device.clone())) }
    };
    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.tick().await; // the first tick completes immediately
    loop {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(t))) => match &reg { Reg::Executor(_) => hub.on_executor_frame(t.as_str()), Reg::Client(id) => hub.on_client_frame(*id, t.as_str()) },
                Some(Ok(Message::Ping(p))) => { if sink.send(Message::Pong(p)).await.is_err() { break; } }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Err(e)) => { tracing::debug!(error = %e, "remote: socket error"); break; }
                Some(Ok(_)) => {}
            },
            outgoing = rx.recv() => match outgoing {
                Some(s) => { if sink.send(Message::text(s)).await.is_err() { break; } }
                None => break, // replaced by a newer executor
            },
            _ = ping.tick() => { if sink.send(Message::Ping(Bytes::new())).await.is_err() { break; } }
            _ = shutdown.cancelled() => { let _ = sink.send(Message::Close(None)).await; break; }
        }
    }
    match reg {
        Reg::Executor(g) => { hub.detach_executor(g); tracing::info!("remote: desktop executor disconnected"); }
        Reg::Client(id) => { hub.remove_client(id); tracing::info!(peer = %peer.ip(), "remote: client disconnected"); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio_tungstenite::connect_async;

    #[test]
    fn inject_shim_places_script_first_in_head() {
        let html = "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <meta charset=\"UTF-8\" />\n    <script type=\"module\" crossorigin src=\"/assets/index-abc.js\"></script>\n  </head>\n  <body><div id=\"root\"></div></body>\n</html>";
        let out = inject_shim(html);
        let head = out.find("<head>").unwrap();
        assert!(out[head..].starts_with("<head><script src=\"/__remote/shim.js\"></script>"), "{out}");
        assert!(!out.contains("<base "), "absolute asset urls need no base");
        assert_eq!(out.matches("__remote/shim.js").count(), 1);
        // relative assets get a <base>
        let rel = "<html><head><link href=\"./assets/a.css\"></head><body></body></html>";
        let out = inject_shim(rel);
        assert!(out.starts_with("<html><head><base href=\"/\"><script src=\"/__remote/shim.js\"></script><link"), "{out}");
        // <head> with attributes; no head at all
        assert!(inject_shim("<html><head data-x=\"1\"><title>t</title></head></html>").contains("<head data-x=\"1\"><script src=\"/__remote/shim.js\"></script><title>"));
        assert!(inject_shim("<html><body></body></html>").starts_with("<html><head><script src=\"/__remote/shim.js\"></script></head><body>"));
        assert!(inject_shim("plain").starts_with("<script src=\"/__remote/shim.js\"></script>plain"));
    }

    #[test]
    fn asset_paths_and_urls() {
        assert!(is_asset_path("/assets/index-abc.js"));
        assert!(is_asset_path("/favicon.ico"));
        assert!(!is_asset_path("/"));
        assert!(!is_asset_path("/settings"));
        assert_eq!(urls_for("127.0.0.1", 17430), vec!["http://127.0.0.1:17430"]);
        let all = urls_for("0.0.0.0", 1);
        assert_eq!(all.last().map(String::as_str), Some("http://127.0.0.1:1"));
        assert!(all.iter().all(|u| u.starts_with("http://") && u.ends_with(":1")));
        assert!(lan_ipv4s().iter().all(|ip| !ip.is_loopback()));
    }

    fn deps(dir: &std::path::Path) -> (Deps, Arc<Pairing>, Arc<DeviceStore>, Arc<Hub>) {
        let mut assets: HashMap<String, (Vec<u8>, String)> = HashMap::new();
        assets.insert("/index.html".into(), (b"<!doctype html><html><head><title>Caret</title><script src=\"/assets/app.js\"></script></head><body><div id=\"root\"></div></body></html>".to_vec(), "text/html".into()));
        assets.insert("/assets/app.js".into(), (b"console.log('caret')".to_vec(), "application/javascript".into()));
        let pairing = Arc::new(Pairing::default());
        let devices = Arc::new(DeviceStore::load(dir.join("devices.json")));
        let hub = Hub::new();
        let d = Deps { assets: Arc::new(assets), pairing: pairing.clone(), devices: devices.clone(), hub: hub.clone(), executor_secret: "s3cret".into(), version: "9.9.9".into(), on_change: None };
        (d, pairing, devices, hub)
    }

    type Ws = WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
    async fn next_text(ws: &mut Ws) -> Value {
        loop {
            let m = tokio::time::timeout(Duration::from_secs(5), ws.next()).await.expect("frame in time").expect("stream open").expect("ok frame");
            if let Message::Text(t) = m { return serde_json::from_str(t.as_str()).unwrap(); }
        }
    }
    fn status_of(e: tokio_tungstenite::tungstenite::Error) -> u16 {
        match e { tokio_tungstenite::tungstenite::Error::Http(r) => r.status().as_u16(), other => panic!("expected an HTTP error, got {other:?}") }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn http_surface_pairing_and_static_auth() {
        let dir = tempfile::tempdir().unwrap();
        let (d, pairing, devices, _hub) = deps(dir.path());
        let server = RemoteServer::start("127.0.0.1", 0, d).await.unwrap();
        let base = format!("http://127.0.0.1:{}", server.port);
        let c = reqwest::Client::new();
        // health
        let h: Value = c.get(format!("{base}/health")).send().await.unwrap().json().await.unwrap();
        assert_eq!(h["ok"], true); assert_eq!(h["name"], "caret"); assert_eq!(h["version"], "9.9.9"); assert_eq!(h["paired"], 0);
        // unauthenticated: pairing page for the app, 401 for assets, 401 for the shim
        let r = c.get(format!("{base}/")).send().await.unwrap();
        assert_eq!(r.status(), 200);
        let body = r.text().await.unwrap();
        assert!(body.contains("Pair with Caret") && !body.contains("__remote/shim.js"));
        assert_eq!(c.get(format!("{base}/assets/app.js")).send().await.unwrap().status(), 401);
        assert_eq!(c.get(format!("{base}/__remote/shim.js")).send().await.unwrap().status(), 401);
        assert_eq!(c.get(format!("{base}/?token=bogus")).send().await.unwrap().text().await.unwrap().contains("Pair with Caret"), true);
        // traversal attempts are rejected before auth (sent raw: HTTP clients normalise `..` away)
        for raw in ["/assets/../x.js", "/assets/%2e%2e/x.js", "/assets/..%2fx.js"] {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", server.port)).await.unwrap();
            s.write_all(format!("GET {raw} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            let mut buf = String::new(); s.read_to_string(&mut buf).await.unwrap();
            assert!(buf.starts_with("HTTP/1.1 400"), "{raw}: {buf}");
        }
        assert!(is_traversal("/a/../b") && is_traversal("/%2E%2E/x") && !is_traversal("/assets/index-a.b.js"));
        // bad / missing code → 403; bad json → 400
        let r = c.post(format!("{base}/pair")).json(&json!({ "code": "000000", "device": "x" })).send().await.unwrap();
        assert_eq!(r.status(), 403);
        assert_eq!(c.post(format!("{base}/pair")).body("{nope").send().await.unwrap().status(), 400);
        // good code → token (one-time)
        let code = pairing.generate();
        let r = c.post(format!("{base}/pair")).header("user-agent", "TestBrowser/1").json(&json!({ "code": code.code, "device": "Laptop" })).send().await.unwrap();
        assert_eq!(r.status(), 200);
        let v: Value = r.json().await.unwrap();
        let token = v["token"].as_str().unwrap().to_string();
        assert_eq!(token.len(), 64);
        assert_eq!(v["device"]["name"], "Laptop"); assert_eq!(v["device"]["userAgent"], "TestBrowser/1");
        assert_eq!(devices.len(), 1);
        assert_eq!(c.post(format!("{base}/pair")).json(&json!({ "code": code.code })).send().await.unwrap().status(), 403, "code is one-time");
        // too many attempts → 429 (three used above: wrong code, the pairing, the replay; two more are allowed)
        for _ in 0..2 { assert_eq!(c.post(format!("{base}/pair")).json(&json!({ "code": "123456" })).send().await.unwrap().status(), 403); }
        assert_eq!(c.post(format!("{base}/pair")).json(&json!({ "code": "123456" })).send().await.unwrap().status(), 429);
        // authenticated via ?token= → app with the shim injected first in <head>, cookie set
        let r = c.get(format!("{base}/?token={token}")).send().await.unwrap();
        assert_eq!(r.status(), 200);
        let cookie = r.headers().get("set-cookie").unwrap().to_str().unwrap().to_string();
        assert!(cookie.starts_with(&format!("caret_token={token}; Path=/; SameSite=Lax")), "{cookie}");
        let body = r.text().await.unwrap();
        assert!(body.starts_with("<!doctype html><html><head><script src=\"/__remote/shim.js\"></script><title>Caret</title>"), "{body}");
        // cookie auth for assets and the shim
        let r = c.get(format!("{base}/assets/app.js")).header("cookie", format!("other=1; caret_token={token}")).send().await.unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers().get("cache-control").unwrap(), "public, max-age=31536000, immutable");
        assert_eq!(r.text().await.unwrap(), "console.log('caret')");
        let r = c.get(format!("{base}/__remote/shim.js")).header("cookie", format!("caret_token={token}")).send().await.unwrap();
        assert_eq!(r.status(), 200);
        assert!(r.text().await.unwrap().contains("__TAURI_INTERNALS__"));
        assert_eq!(c.get(format!("{base}/assets/missing.js")).header("cookie", format!("caret_token={token}")).send().await.unwrap().status(), 404);
        assert_eq!(c.get(format!("{base}/__remote/pair.html")).send().await.unwrap().status(), 200);
        assert_eq!(c.delete(format!("{base}/health")).send().await.unwrap().status(), 405);
        // revoked token no longer works
        let id = devices.list()[0].id.clone();
        assert!(devices.revoke(&id).unwrap());
        assert_eq!(c.get(format!("{base}/assets/app.js")).header("cookie", format!("caret_token={token}")).send().await.unwrap().status(), 401);
        server.shutdown();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn relay_end_to_end_over_websockets() {
        let dir = tempfile::tempdir().unwrap();
        let (d, _pairing, devices, hub) = deps(dir.path());
        let server = RemoteServer::start("127.0.0.1", 0, d).await.unwrap();
        let ws_base = format!("ws://127.0.0.1:{}/ws", server.port);
        let (_info, token) = devices.add("Phone", "ua").unwrap();
        // auth failures
        assert_eq!(status_of(connect_async(format!("{ws_base}?role=client&token=wrong")).await.err().unwrap()), 401);
        assert_eq!(status_of(connect_async(format!("{ws_base}?role=client")).await.err().unwrap()), 401);
        assert_eq!(status_of(connect_async(format!("{ws_base}?role=executor&secret=nope")).await.err().unwrap()), 401);
        assert_eq!(status_of(connect_async(format!("{ws_base}?role=other")).await.err().unwrap()), 400);
        // client without an executor gets a local error reply
        let (mut client, _) = connect_async(format!("{ws_base}?role=client&token={token}")).await.unwrap();
        client.send(Message::text(json!({ "t": "invoke", "id": 1, "cmd": "app_info", "args": {} }).to_string())).await.unwrap();
        let r = next_text(&mut client).await;
        assert_eq!(r, json!({ "t": "reply", "id": 1, "ok": false, "error": super::super::relay::NO_EXECUTOR }));
        // executor joins
        let (mut exec, _) = connect_async(format!("{ws_base}?role=executor&secret=s3cret")).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async { while !hub.executor_connected() { tokio::time::sleep(Duration::from_millis(10)).await; } }).await.unwrap();
        assert_eq!(hub.client_count(), 1);
        // invoke → executor
        client.send(Message::text(json!({ "t": "invoke", "id": 2, "cmd": "fs_read_file", "args": { "path": "/tmp/x", "onEvent": "__CHANNEL__:77" } }).to_string())).await.unwrap();
        let fwd = next_text(&mut exec).await;
        assert_eq!(fwd["t"], "invoke"); assert_eq!(fwd["id"], 2); assert_eq!(fwd["cmd"], "fs_read_file"); assert_eq!(fwd["args"]["onEvent"], "__CHANNEL__:77");
        let client_id = fwd["client"].as_u64().unwrap();
        // cb then reply → client, in order
        exec.send(Message::text(json!({ "t": "cb", "client": client_id, "callback": 77, "payload": { "index": 0, "message": "chunk" } }).to_string())).await.unwrap();
        exec.send(Message::text(json!({ "t": "reply", "client": client_id, "id": 2, "ok": true, "value": { "content": "hello" } }).to_string())).await.unwrap();
        assert_eq!(next_text(&mut client).await, json!({ "t": "cb", "callback": 77, "payload": { "index": 0, "message": "chunk" } }));
        assert_eq!(next_text(&mut client).await, json!({ "t": "reply", "id": 2, "ok": true, "value": { "content": "hello" } }));
        // error replies
        exec.send(Message::text(json!({ "t": "reply", "client": client_id, "id": 3, "ok": false, "error": { "code": "denied", "message": "no" } }).to_string())).await.unwrap();
        assert_eq!(next_text(&mut client).await["error"]["code"], "denied");
        // client leaves → executor told
        client.close(None).await.unwrap();
        let gone = next_text(&mut exec).await;
        assert_eq!(gone, json!({ "t": "client_gone", "client": client_id }));
        tokio::time::timeout(Duration::from_secs(5), async { while hub.client_count() != 0 { tokio::time::sleep(Duration::from_millis(10)).await; } }).await.unwrap();
        // a second executor replaces the first (the first socket is closed by the server)
        let (mut exec2, _) = connect_async(format!("{ws_base}?role=executor&secret=s3cret")).await.unwrap();
        let ended = tokio::time::timeout(Duration::from_secs(5), async { loop { match exec.next().await { None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break true, _ => {} } } }).await.unwrap();
        assert!(ended, "old executor socket should be closed");
        assert!(hub.executor_connected());
        let (mut client2, _) = connect_async(format!("{ws_base}?role=client&token={token}")).await.unwrap();
        client2.send(Message::text(json!({ "t": "invoke", "id": 9, "cmd": "x" }).to_string())).await.unwrap();
        assert_eq!(next_text(&mut exec2).await["cmd"], "x");
        // shutdown closes everything
        server.shutdown();
        let closed = tokio::time::timeout(Duration::from_secs(5), async { loop { match client2.next().await { None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break true, _ => {} } } }).await.unwrap();
        assert!(closed);
    }
}

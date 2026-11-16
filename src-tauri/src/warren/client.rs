//! Warren HTTP client: the Sandbox API on the API host (`Authorization: Bearer sk_live_…`) and the files API on
//! instance hosts (`http(s)://<id>.<base>[:port]/v1/files…`, `X-Warren-Key`).
//!
//! `connect_via` (an SSH tunnel such as `127.0.0.1:7337`) makes every request, API and instance host alike, open its
//! TCP connection to that address while the request line carries the logical host in an explicit `Host` header; the
//! daemon's edge routes on `Host`, so `api.warren.localhost` and every dynamic `<id>.warren.localhost` work through one
//! tunnel. (reqwest's `resolve_to_addrs` would need one client per instance host and ignores the port of the address.)
//!
//! Warren errors `{ error: { code, message } }` become [`CaretError`] with the Warren code kept; a 401 says plainly that
//! the Warren API key is invalid.
use super::types::wire;
use crate::error::{CaretError, Result};
use bytes::Bytes;
use reqwest::header::{HeaderValue, CONTENT_TYPE, HOST};
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;
use std::time::{Duration, Instant};
use url::Url;

/// Header carrying the key on instance hosts.
pub const HEADER_API_KEY: &str = "x-warren-key";
/// The browser terminal (ttyd) inside every built-in image.
pub const TERMINAL_PORT: u16 = 7681;
/// How often [`WarrenClient::wait_running`] polls.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// Warren's own default for `exec` without `timeout_seconds`.
pub const DEFAULT_EXEC_TIMEOUT_SECS: u32 = 15 * 60;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Sandbox API requests have a 60 s deadline on the daemon; match it.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Tarballs can be large and the instance may be waking up first.
const FILE_TIMEOUT: Duration = Duration::from_secs(600);
/// Added to the command timeout so a long command is not cut short by the HTTP layer.
const EXEC_MARGIN: Duration = Duration::from_secs(60);
/// The default Sandbox API URL (Warren's `DEFAULT_API_URL`).
pub const DEFAULT_API_URL: &str = "http://api.warren.localhost:7337/v1";

/// Client for one Warren deployment and key. Cloning is cheap (the HTTP client is reference counted).
#[derive(Debug, Clone)]
pub struct WarrenClient {
    http: reqwest::Client,
    /// Normalised API base, e.g. `http://api.warren.localhost:7337/v1` (path without trailing slash).
    api_url: Url,
    api_key: String,
    /// `(host, port)` connections go to instead of the URL's host; `None` port keeps the logical URL's port.
    connect_via: Option<(String, Option<u16>)>,
    /// Base domain of instance hosts (`warren.localhost` for `api.warren.localhost`).
    base_host: String,
    poll_interval: Duration,
}

impl WarrenClient {
    /// `api_url` may omit `/v1` (added when missing); `connect_via` is `host[:port]` or `None`.
    /// Fails with a `config` error on an unparsable URL, a URL without a host, a malformed `connect_via`, or
    /// `connect_via` with an `https` URL (TLS would be negotiated with the tunnel address, not the Warren host).
    pub fn new(api_url: &str, api_key: &str, connect_via: Option<&str>) -> Result<Self> {
        let api_url = Url::parse(&normalize_api_url(api_url)).map_err(|e| CaretError::config(format!("invalid Warren API URL `{api_url}`: {e}")))?;
        let host = api_url.host_str().ok_or_else(|| CaretError::config(format!("Warren API URL `{api_url}` has no host")))?.to_string();
        if !matches!(api_url.scheme(), "http" | "https") { return Err(CaretError::config(format!("Warren API URL `{api_url}` must be http or https"))); }
        let connect_via = match connect_via.map(str::trim).filter(|s| !s.is_empty()) {
            None => None,
            Some(via) => {
                if api_url.scheme() == "https" { return Err(CaretError::config("warren.connectVia needs a plain-http apiUrl: a TLS handshake with the tunnel address would not match the Warren host")); }
                let u = Url::parse(&format!("http://{via}")).map_err(|e| CaretError::config(format!("invalid warren.connectVia `{via}`: {e}")))?;
                let h = u.host_str().filter(|h| !h.is_empty()).ok_or_else(|| CaretError::config(format!("invalid warren.connectVia `{via}`: expected host:port")))?;
                if u.path() != "/" || u.query().is_some() || !u.username().is_empty() { return Err(CaretError::config(format!("invalid warren.connectVia `{via}`: expected host:port"))); }
                Some((h.to_string(), u.port()))
            }
        };
        let base_host = host.strip_prefix("api.").unwrap_or(&host).to_string();
        let http = reqwest::Client::builder()
            .user_agent(concat!("Caret/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .pool_max_idle_per_host(4)
            .build()
            .map_err(|e| CaretError::internal(format!("warren http client: {e}")))?;
        Ok(Self { http, api_url, api_key: api_key.trim().to_string(), connect_via, base_host, poll_interval: POLL_INTERVAL })
    }

    /// Poll interval of [`wait_running`](Self::wait_running) (tests shorten it).
    pub fn with_poll_interval(mut self, interval: Duration) -> Self { self.poll_interval = interval; self }

    /// The normalised API base, e.g. `http://api.warren.localhost:7337/v1`.
    pub fn api_url(&self) -> &str { self.api_url.as_str().trim_end_matches('/') }
    /// A key is set.
    pub fn has_key(&self) -> bool { !self.api_key.is_empty() }
    /// Logical URL of an instance's default port: `http://<id>.<base>[:port]`.
    pub fn instance_url(&self, id: &str) -> String { let mut u = self.instance_base(id); u.set_path("/"); u.as_str().trim_end_matches('/').to_string() }

    // ───────────────────────── request plumbing ─────────────────────────

    /// Where the connection goes and which `Host` to send. Without `connect_via` the logical URL is used as is.
    fn target(&self, logical: &Url) -> (Url, Option<HeaderValue>) {
        match &self.connect_via {
            None => (logical.clone(), None),
            Some((host, port)) => {
                let header = host_header(logical);
                let mut u = logical.clone();
                let _ = u.set_host(Some(host));
                if let Some(p) = port { let _ = u.set_port(Some(*p)); }
                (u, HeaderValue::from_str(&header).ok())
            }
        }
    }

    fn send_to(&self, method: Method, logical: Url) -> RequestBuilder {
        let (target, host) = self.target(&logical);
        let mut rb = self.http.request(method, target);
        if let Some(h) = host { rb = rb.header(HOST, h); }
        rb
    }

    /// Sandbox API request: `<api_url><path>?query`, bearer auth unless `anonymous`.
    fn api(&self, method: Method, path: &str, query: &[(&str, String)], anonymous: bool) -> RequestBuilder {
        let mut url = self.api_url.clone();
        let base = self.api_url.path().trim_end_matches('/').to_string();
        url.set_path(&format!("{base}{path}"));
        for (k, v) in query { url.query_pairs_mut().append_pair(k, v); }
        let mut rb = self.send_to(method, url);
        if !anonymous && !self.api_key.is_empty() { rb = rb.bearer_auth(&self.api_key); }
        rb
    }

    fn instance_base(&self, id: &str) -> Url {
        let mut u = self.api_url.clone();
        let _ = u.set_host(Some(&format!("{id}.{}", self.base_host)));
        u.set_query(None);
        u
    }

    /// Instance-host request: `<scheme>://<id>.<base>[:port]/v1<path>?query` with `X-Warren-Key`.
    fn instance_host(&self, method: Method, id: &str, path: &str, query: &[(&str, String)]) -> RequestBuilder {
        let mut url = self.instance_base(id);
        url.set_path(&format!("/v1{path}"));
        for (k, v) in query { url.query_pairs_mut().append_pair(k, v); }
        self.send_to(method, url).header(HEADER_API_KEY, &self.api_key).timeout(FILE_TIMEOUT)
    }

    fn instance_path(id: &str, suffix: &str) -> Result<String> { check_id(id)?; Ok(format!("/instances/{id}{suffix}")) }

    async fn json<T: DeserializeOwned>(&self, rb: RequestBuilder, what: &str) -> Result<T> {
        let body = self.bytes(rb, what).await?;
        serde_json::from_slice(&body).map_err(|e| CaretError::new("warren_decode", format!("Warren {what}: unexpected response ({e}): {}", snippet(&body))))
    }

    async fn list<T: DeserializeOwned>(&self, rb: RequestBuilder, what: &str) -> Result<Vec<T>> { self.json::<wire::List<T>>(rb, what).await.map(|l| l.data) }

    async fn bytes(&self, rb: RequestBuilder, what: &str) -> Result<Bytes> {
        let resp = rb.send().await.map_err(|e| net_error(e, what))?;
        let status = resp.status();
        let body = resp.bytes().await.map_err(|e| net_error(e, what))?;
        if !status.is_success() { return Err(api_error(status, &body)); }
        Ok(body)
    }

    async fn empty(&self, rb: RequestBuilder, what: &str) -> Result<()> { self.bytes(rb, what).await.map(|_| ()) }

    // ───────────────────────── Sandbox API ─────────────────────────

    /// `GET /v1/health` (no auth).
    pub async fn health(&self) -> Result<wire::Health> { self.json(self.api(Method::GET, "/health", &[], true), "health").await }
    /// `GET /v1/workspace`: the workspace behind the key plus awake counters.
    pub async fn workspace(&self) -> Result<wire::Workspace> { self.json(self.api(Method::GET, "/workspace", &[], false), "workspace").await }
    /// `GET /v1/billing`.
    pub async fn billing(&self) -> Result<wire::Billing> { self.json(self.api(Method::GET, "/billing", &[], false), "billing").await }
    /// `GET /v1/templates`.
    pub async fn templates(&self) -> Result<Vec<wire::Template>> { self.list(self.api(Method::GET, "/templates", &[], false), "templates").await }
    /// `GET /v1/instances`, newest first.
    pub async fn instances(&self) -> Result<Vec<wire::Instance>> { self.list(self.api(Method::GET, "/instances", &[], false), "instances").await }
    /// `GET /v1/instances/{id}`.
    pub async fn instance(&self, id: &str) -> Result<wire::Instance> { self.json(self.api(Method::GET, &Self::instance_path(id, "")?, &[], false), "instance").await }
    /// `POST /v1/instances`; returns the instance in `provisioning` (see [`wait_running`](Self::wait_running)).
    pub async fn create_instance(&self, req: &wire::CreateInstanceRequest) -> Result<wire::Instance> {
        self.json(self.api(Method::POST, "/instances", &[], false).json(req), "create instance").await
    }
    /// `DELETE /v1/instances/{id}` → `{ id, status: "deleting" }`.
    pub async fn delete_instance(&self, id: &str) -> Result<wire::StatusResponse> { self.json(self.api(Method::DELETE, &Self::instance_path(id, "")?, &[], false), "delete instance").await }
    /// `POST /v1/instances/{id}/start`.
    pub async fn start(&self, id: &str) -> Result<wire::StatusResponse> { self.lifecycle(id, "start").await }
    /// `POST /v1/instances/{id}/stop`.
    pub async fn stop(&self, id: &str) -> Result<wire::StatusResponse> { self.lifecycle(id, "stop").await }
    /// `POST /v1/instances/{id}/sleep`.
    pub async fn sleep(&self, id: &str) -> Result<wire::StatusResponse> { self.lifecycle(id, "sleep").await }
    /// `POST /v1/instances/{id}/restart`.
    pub async fn restart(&self, id: &str) -> Result<wire::StatusResponse> { self.lifecycle(id, "restart").await }
    async fn lifecycle(&self, id: &str, op: &str) -> Result<wire::StatusResponse> {
        self.json(self.api(Method::POST, &Self::instance_path(id, &format!("/{op}"))?, &[], false).json(&serde_json::json!({})), op).await
    }

    /// `POST /v1/instances/{id}/exec`. The HTTP timeout is the command timeout (Warren's default when zero) plus a
    /// margin. Wakes a sleeping instance; `409 conflict` when it is stopped.
    pub async fn exec(&self, id: &str, req: &wire::ExecRequest) -> Result<wire::ExecResult> {
        let secs = if req.timeout_seconds > 0 { req.timeout_seconds } else { DEFAULT_EXEC_TIMEOUT_SECS };
        let rb = self.api(Method::POST, &Self::instance_path(id, "/exec")?, &[], false).timeout(Duration::from_secs(u64::from(secs)) + EXEC_MARGIN).json(req);
        self.json(rb, "exec").await
    }

    /// `GET /v1/instances/{id}/signed-url?port=&ttl=`; `port` `None` means the default port, `ttl` `None` the server
    /// default (TTL range 60..86400 s).
    pub async fn signed_url(&self, id: &str, port: Option<u16>, ttl: Option<u64>) -> Result<wire::SignedUrl> {
        let mut q = Vec::new();
        if let Some(p) = port { q.push(("port", p.to_string())); }
        if let Some(t) = ttl { q.push(("ttl", t.to_string())); }
        self.json(self.api(Method::GET, &Self::instance_path(id, "/signed-url")?, &q, false), "signed url").await
    }

    /// `GET /v1/instances/{id}/logs?tail=`.
    pub async fn logs(&self, id: &str, tail: Option<u32>) -> Result<String> {
        let q: Vec<(&str, String)> = tail.map(|t| vec![("tail", t.to_string())]).unwrap_or_default();
        self.json::<wire::Logs>(self.api(Method::GET, &Self::instance_path(id, "/logs")?, &q, false), "logs").await.map(|l| l.logs)
    }

    /// Poll `GET /v1/instances/{id}` every [`POLL_INTERVAL`] until the instance is `running`. Errors with
    /// `instance_failed` on `failed`, `not_found` on `deleting`/`deleted`, `conflict` at once on `sleeping`/`stopped`
    /// (nothing changes those by itself; see [`ensure_awake`](Self::ensure_awake)), `timeout` after `timeout`.
    pub async fn wait_running(&self, id: &str, timeout: Duration) -> Result<wire::Instance> {
        let start = Instant::now();
        loop {
            let inst = self.instance(id).await?;
            match inst.status.as_str() {
                "running" => return Ok(inst),
                "failed" => {
                    let reason = if inst.status_reason.is_empty() { String::new() } else { format!(": {}", inst.status_reason) };
                    return Err(CaretError::new("instance_failed", format!("Warren instance {id} failed{reason}")));
                }
                "deleting" | "deleted" => return Err(CaretError::not_found(format!("Warren instance {id}"))),
                s @ ("sleeping" | "stopped") => return Err(CaretError::new("conflict", format!("Warren instance {id} is {s}; it has to be woken or started first"))),
                other => {
                    if start.elapsed() + self.poll_interval > timeout {
                        return Err(CaretError::new("timeout", format!("Warren instance {id} did not reach running within {}s (last status: {other})", timeout.as_secs())));
                    }
                }
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }

    /// Make the instance `running`: a `stopped` one is started, a `sleeping` one is woken by an `exec true` (the
    /// daemon holds the request until the instance is up), anything in transit is waited for (`wait_running`).
    /// The executor calls this before a command; a running instance costs one `GET`.
    pub async fn ensure_awake(&self, id: &str, timeout: Duration) -> Result<wire::Instance> {
        let inst = self.instance(id).await?;
        match inst.status.as_str() {
            "running" => Ok(inst),
            "stopped" => { self.start(id).await?; self.wait_running(id, timeout).await }
            "sleeping" => {
                let secs = timeout.as_secs().clamp(30, u64::from(u32::MAX)) as u32;
                self.exec(id, &wire::ExecRequest { timeout_seconds: secs, ..wire::ExecRequest::new("true") }).await?;
                self.wait_running(id, timeout).await
            }
            _ => self.wait_running(id, timeout).await,
        }
    }

    // ───────────────────────── files API (instance host) ─────────────────────────

    /// `GET /v1/files?path=`: entries of a directory relative to the persistent home.
    pub async fn files_list(&self, id: &str, path: &str) -> Result<Vec<wire::FileEntry>> {
        check_id(id)?;
        self.json::<wire::FileListing>(self.instance_host(Method::GET, id, "/files", &[("path", path.to_string())]), "files list").await.map(|l| l.entries)
    }
    /// `GET /v1/files/content?path=`: raw bytes of a file.
    pub async fn files_read(&self, id: &str, path: &str) -> Result<Bytes> {
        check_id(id)?;
        self.bytes(self.instance_host(Method::GET, id, "/files/content", &[("path", path.to_string())]), "files read").await
    }
    /// `PUT /v1/files/content?path=` with a raw body; parent directories are created.
    pub async fn files_write(&self, id: &str, path: &str, body: impl Into<Bytes>) -> Result<wire::FileWritten> {
        check_id(id)?;
        let rb = self.instance_host(Method::PUT, id, "/files/content", &[("path", path.to_string())]).header(CONTENT_TYPE, "application/octet-stream").body(body.into());
        self.json(rb, "files write").await
    }
    /// `DELETE /v1/files?path=`.
    pub async fn files_delete(&self, id: &str, path: &str) -> Result<()> {
        check_id(id)?;
        self.empty(self.instance_host(Method::DELETE, id, "/files", &[("path", path.to_string())]), "files delete").await
    }
    /// `POST /v1/files/move` `{ from, to }`.
    pub async fn files_move(&self, id: &str, from: &str, to: &str) -> Result<()> {
        check_id(id)?;
        self.empty(self.instance_host(Method::POST, id, "/files/move", &[]).json(&serde_json::json!({ "from": from, "to": to })), "files move").await
    }
    /// `GET /v1/files/download?path=`: a gzip tarball of a directory.
    pub async fn files_download(&self, id: &str, path: &str) -> Result<Bytes> {
        check_id(id)?;
        self.bytes(self.instance_host(Method::GET, id, "/files/download", &[("path", path.to_string())]), "files download").await
    }
}

/// Trim, drop trailing slashes, add `/v1` when missing.
pub fn normalize_api_url(url: &str) -> String {
    let t = url.trim().trim_end_matches('/');
    if t.is_empty() { return DEFAULT_API_URL.to_string(); }
    if t.ends_with("/v1") { t.to_string() } else { format!("{t}/v1") }
}

/// Instance ids are 10 lowercase alphanumerics; anything else never reaches a URL or a hostname.
fn check_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 64 || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
        return Err(CaretError::invalid(format!("invalid Warren instance id `{id}`")));
    }
    Ok(())
}

/// `host[:port]` as a `Host` header value for `u` (the port only when it is not the scheme's default).
fn host_header(u: &Url) -> String {
    let host = u.host_str().unwrap_or("");
    match u.port() { Some(p) => format!("{host}:{p}"), None => host.to_string() }
}

fn snippet(body: &[u8]) -> String {
    let s = String::from_utf8_lossy(body);
    let s = s.trim();
    if s.len() > 200 { format!("{}…", &s[..s.char_indices().nth(200).map(|(i, _)| i).unwrap_or(s.len())]) } else { s.to_string() }
}

/// Transport failure → `timeout` / `connect` / `http` with the operation named.
fn net_error(e: reqwest::Error, what: &str) -> CaretError {
    let base = CaretError::from(e);
    let hint = match base.code.as_str() { "connect" => " (is the daemon running and the tunnel up?)", "timeout" => " (timed out)", _ => "" };
    CaretError::new(base.code.clone(), format!("Warren {what} failed{hint}: {}", base.message))
}

/// Warren's `{ error: { code, message } }` → `CaretError` with the same code; 401 names the key.
pub fn api_error(status: StatusCode, body: &[u8]) -> CaretError {
    let parsed = serde_json::from_slice::<wire::ApiErrorEnvelope>(body).ok().map(|e| e.error);
    let (code, message) = match parsed {
        Some(e) => {
            let code = if e.code.is_empty() { format!("http_{}", status.as_u16()) } else { e.code };
            let message = if e.message.is_empty() { format!("Warren returned HTTP {status}") } else { e.message };
            (code, message)
        }
        None => (format!("http_{}", status.as_u16()), format!("Warren returned HTTP {status}: {}", snippet(body))),
    };
    if status == StatusCode::UNAUTHORIZED || code == "invalid_api_key" {
        return CaretError::new("invalid_api_key", format!("Invalid Warren API key (Settings → Sandboxes): {message}"));
    }
    CaretError::new(code, message)
}

#[cfg(test)]
mod live {
    //! Against a real daemon; ignored unless `WARREN_API_URL` and `WARREN_API_KEY` are set. Optional:
    //! `WARREN_CONNECT_VIA` (tunnel), `WARREN_SMOKE_ID` (a running instance; without it only health and the instance
    //! list are checked). Never creates or deletes instances; files it writes live under `~/.caret-live-smoke` and are
    //! removed afterwards.
    //! `WARREN_API_URL=http://api.warren.localhost:7337/v1 WARREN_API_KEY=$(cat /tmp/warren-key) WARREN_CONNECT_VIA=127.0.0.1:7337 WARREN_SMOKE_ID=$(cat /tmp/warren-smoke-id) cargo test --lib warren::client::live -- --ignored --nocapture`
    use super::*;
    use crate::warren::sync;
    use crate::warren::types::SandboxBinding;

    #[tokio::test]
    #[ignore]
    async fn live_smoke() {
        let (Ok(url), Ok(key)) = (std::env::var("WARREN_API_URL"), std::env::var("WARREN_API_KEY")) else { eprintln!("WARREN_API_URL / WARREN_API_KEY unset; skipping"); return };
        let via = std::env::var("WARREN_CONNECT_VIA").ok();
        let c = WarrenClient::new(&url, &key, via.as_deref()).unwrap();
        let h = c.health().await.unwrap();
        eprintln!("health: healthy={} version={} dev_mode={} runtimes={:?}", h.healthy, h.version, h.dev_mode, h.runtimes);
        assert!(h.healthy);
        let ws = c.workspace().await.unwrap();
        eprintln!("workspace: {} ({}) wallet={} awake {}/{}", ws.name, ws.id, ws.wallet_micros, ws.awake_count, ws.awake_limit);
        let list = c.instances().await.unwrap();
        eprintln!("instances: {}", list.iter().map(|i| format!("{} {} {}", i.id, i.name, i.status)).collect::<Vec<_>>().join(", "));
        let Ok(id) = std::env::var("WARREN_SMOKE_ID") else { return };
        let before = c.instance(&id).await.unwrap().status;
        let inst = c.ensure_awake(&id, Duration::from_secs(120)).await.unwrap();
        eprintln!("instance {id}: {before} -> {} template={} mount={}", inst.status, inst.template, inst.mount_path);
        let r = c.exec(&id, &wire::ExecRequest { timeout_seconds: 30, ..wire::ExecRequest::new("echo ok") }).await.unwrap();
        eprintln!("exec echo ok: exit={} stdout={:?} stderr={:?} {}ms", r.exit_code, r.stdout, r.stderr, r.duration_ms);
        assert_eq!((r.exit_code, r.stdout.as_str()), (0, "ok\n"));
        let w = c.files_write(&id, ".caret-live-smoke/hello.txt", b"hello from caret".to_vec()).await.unwrap();
        eprintln!("files_write: {} ({} bytes)", w.path, w.size);
        let back = c.files_read(&id, ".caret-live-smoke/hello.txt").await.unwrap();
        assert_eq!(&back[..], b"hello from caret");
        let entries = c.files_list(&id, ".caret-live-smoke").await.unwrap();
        eprintln!("files_list: {:?}", entries.iter().map(|e| (&e.name, &e.kind, e.size)).collect::<Vec<_>>());
        assert!(entries.iter().any(|e| e.name == "hello.txt" && e.kind == "file" && e.size == 16));

        // A real push / pull round trip under a scratch root.
        let local = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(local.path().join("src")).unwrap();
        std::fs::write(local.path().join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(local.path().join(".gitignore"), "target/\n").unwrap();
        std::fs::create_dir_all(local.path().join("target")).unwrap();
        std::fs::write(local.path().join("target/x.bin"), "x").unwrap();
        let root = format!("{}/.caret-live-smoke/proj", inst.mount_path.trim_end_matches('/'));
        let binding = SandboxBinding::new(&id, "smoke", &root);
        let excludes = vec!["node_modules".to_string(), "target".to_string()];
        let now = sync::scan(local.path(), &excludes).unwrap();
        let pushed = sync::push(&c, &binding, local.path(), &sync::plan_push(&sync::Manifest::default(), &now)).await.unwrap();
        eprintln!("push: {pushed:?}");
        assert_eq!(pushed.files, 2);
        let quiet = sync::pull(&c, &binding, local.path(), &now, &excludes).await.unwrap();
        eprintln!("quiet pull: changed={} deleted={:?} {}ms", quiet.changed.len(), quiet.deleted, quiet.ms);
        assert!(quiet.changed.is_empty() && quiet.deleted.is_empty());
        let r = c.exec(&id, &wire::ExecRequest::new(format!("cd {} && sleep 1 && echo generated > gen.txt && printf 'fn main() {{ formatted() }}' > src/main.rs && rm .gitignore", sync::sh_quote(&root)))).await.unwrap();
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        let got = sync::pull(&c, &binding, local.path(), &now, &excludes).await.unwrap();
        eprintln!("pull: changed={:?} deleted={:?} bytes={} {}ms", got.changed.iter().map(|(r, b)| (r.clone(), String::from_utf8_lossy(b).into_owned())).collect::<Vec<_>>(), got.deleted, got.bytes, got.ms);
        assert_eq!(got.changed.iter().map(|(r, b)| (r.as_str(), std::str::from_utf8(b).unwrap())).collect::<Vec<_>>(), vec![("gen.txt", "generated\n"), ("src/main.rs", "fn main() { formatted() }")]);
        assert_eq!(got.deleted, vec![".gitignore".to_string()]);
        let r = c.exec(&id, &wire::ExecRequest::new("rm -rf ~/.caret-live-smoke ~/.caret-sync")).await.unwrap();
        assert_eq!(r.exit_code, 0);
        eprintln!("cleaned up");
    }
}

#[cfg(test)]
pub(crate) mod testserver {
    //! A minimal HTTP/1.1 server for client tests: records every request and answers from a handler.
    use http_body_util::{BodyExt, Full};
    use hyper::body::{Bytes, Incoming};
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{Request, Response, StatusCode};
    use hyper_util::rt::TokioIo;
    use parking_lot::Mutex;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    /// One request as the server saw it.
    #[derive(Debug, Clone)]
    pub struct Recorded { pub method: String, pub path: String, pub query: Option<String>, pub headers: BTreeMap<String, String>, pub body: Vec<u8> }
    impl Recorded {
        pub fn header(&self, name: &str) -> Option<&str> { self.headers.get(&name.to_ascii_lowercase()).map(String::as_str) }
    }

    pub type Handler = Arc<dyn Fn(&Recorded) -> (u16, String) + Send + Sync>;

    pub struct TestServer { pub port: u16, pub requests: Arc<Mutex<Vec<Recorded>>>, stop: tokio_util::sync::CancellationToken }
    impl Drop for TestServer { fn drop(&mut self) { self.stop.cancel(); } }

    impl TestServer {
        pub async fn start(handler: Handler) -> TestServer {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let requests: Arc<Mutex<Vec<Recorded>>> = Arc::default();
            let stop = tokio_util::sync::CancellationToken::new();
            let (reqs, sd) = (requests.clone(), stop.clone());
            tokio::spawn(async move {
                loop {
                    let accepted = tokio::select! { _ = sd.cancelled() => break, a = listener.accept() => a };
                    let Ok((stream, _)) = accepted else { continue };
                    let (reqs, handler) = (reqs.clone(), handler.clone());
                    tokio::spawn(async move {
                        let svc = service_fn(move |req: Request<Incoming>| {
                            let (reqs, handler) = (reqs.clone(), handler.clone());
                            async move {
                                let (parts, body) = req.into_parts();
                                let body = body.collect().await.map(|b| b.to_bytes().to_vec()).unwrap_or_default();
                                let headers = parts.headers.iter().map(|(k, v)| (k.as_str().to_ascii_lowercase(), v.to_str().unwrap_or("").to_string())).collect();
                                let rec = Recorded { method: parts.method.to_string(), path: parts.uri.path().to_string(), query: parts.uri.query().map(str::to_string), headers, body };
                                let (status, body) = handler(&rec);
                                reqs.lock().push(rec);
                                Ok::<_, std::convert::Infallible>(Response::builder().status(StatusCode::from_u16(status).unwrap()).header("content-type", "application/json").body(Full::new(Bytes::from(body))).unwrap())
                            }
                        });
                        let _ = http1::Builder::new().keep_alive(true).serve_connection(TokioIo::new(stream), svc).await;
                    });
                }
            });
            TestServer { port, requests, stop }
        }
        pub fn recorded(&self) -> Vec<Recorded> { self.requests.lock().clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::testserver::{Handler, TestServer};
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    const INSTANCE: &str = r#"{"id":"ab12cd34ef","name":"n","template":"warren-base","type":"default","status":"running","resources":{"cpu":2,"memory":4,"disk":4},"url":"http://ab12cd34ef.warren.localhost:7337","default_port":3737,"mount_path":"/home/agent","created_at":1,"updated_at":2,"runtime":"docker"}"#;

    fn ok(body: &str) -> (u16, String) { (200, body.to_string()) }

    fn canned() -> Handler {
        Arc::new(|r| match (r.method.as_str(), r.path.as_str()) {
            ("GET", "/v1/health") => ok(r#"{"healthy":true,"version":"0.1.0","runtimes":["docker"],"dev_mode":true,"time":1}"#),
            ("GET", "/v1/instances") => ok(&format!(r#"{{"data":[{INSTANCE}]}}"#)),
            ("GET", "/v1/instances/ab12cd34ef") => ok(INSTANCE),
            ("POST", "/v1/instances/ab12cd34ef/exec") => ok(r#"{"exit_code":0,"stdout":"ok\n","stderr":"","duration_ms":12}"#),
            ("POST", "/v1/instances/ab12cd34ef/stop") => ok(r#"{"id":"ab12cd34ef","status":"stopped"}"#),
            ("GET", "/v1/instances/ab12cd34ef/signed-url") => ok(r#"{"url":"http://ab12cd34ef-7681.warren.localhost:7337/?warren_token=t","expires_at":99}"#),
            ("GET", "/v1/files/content") => ok("hello"),
            ("PUT", "/v1/files/content") => ok(r#"{"path":"a/b.txt","size":5}"#),
            ("GET", "/v1/files") => ok(r#"{"path":".","entries":[{"name":"x","type":"file","size":1,"modified_at":5}]}"#),
            ("DELETE", "/v1/files") => ok(r#"{"path":"a","deleted":true}"#),
            ("GET", "/v1/workspace") if r.header("authorization") != Some("Bearer sk_live_good") => (401, r#"{"error":{"code":"invalid_api_key","message":"Invalid API key."}}"#.into()),
            ("GET", "/v1/workspace") => ok(r#"{"id":"ws_1","name":"caret-dev","wallet_micros":5000000,"awake_limit":3,"awake_count":1}"#),
            ("GET", "/v1/instances/zzzzzzzzzz") => (404, r#"{"error":{"code":"not_found","message":"instance not found"}}"#.into()),
            ("GET", "/v1/instances/boom000000") => (500, "<html>nope</html>".into()),
            _ => (404, r#"{"error":{"code":"not_found","message":"no route"}}"#.into()),
        })
    }

    #[test]
    fn new_normalises_the_url_and_derives_the_instance_base() {
        let c = WarrenClient::new("http://api.warren.localhost:7337", "sk_live_x", None).unwrap();
        assert_eq!(c.api_url(), "http://api.warren.localhost:7337/v1");
        assert_eq!(c.instance_url("ab12cd34ef"), "http://ab12cd34ef.warren.localhost:7337");
        let c = WarrenClient::new(" https://warren.example.com/v1/ ", " k ", None).unwrap();
        assert_eq!(c.api_url(), "https://warren.example.com/v1");
        assert_eq!(c.instance_url("ab12cd34ef"), "https://ab12cd34ef.warren.example.com");
        assert_eq!(normalize_api_url(""), DEFAULT_API_URL);
        assert!(WarrenClient::new("not a url", "k", None).is_err());
        assert!(WarrenClient::new("ftp://x/v1", "k", None).is_err());
        assert_eq!(WarrenClient::new("https://api.w.example/v1", "k", Some("127.0.0.1:7337")).unwrap_err().code, "config", "TLS + connectVia is refused");
        assert_eq!(WarrenClient::new("http://api.w.example/v1", "k", Some("127.0.0.1:7337/x")).unwrap_err().code, "config");
        assert!(WarrenClient::new("http://api.w.example/v1", "k", Some("  ")).unwrap().connect_via.is_none(), "blank connectVia means none");
    }

    #[test]
    fn target_rewrites_the_connection_and_keeps_the_logical_host() {
        let c = WarrenClient::new("http://api.warren.localhost:7337/v1", "k", Some("127.0.0.1:9999")).unwrap();
        let (t, h) = c.target(&Url::parse("http://ab12cd34ef.warren.localhost:7337/v1/files?path=.").unwrap());
        assert_eq!(t.as_str(), "http://127.0.0.1:9999/v1/files?path=.");
        assert_eq!(h.unwrap().to_str().unwrap(), "ab12cd34ef.warren.localhost:7337");
        let c = WarrenClient::new("http://api.warren.example/v1", "k", Some("[::1]")).unwrap();
        let (t, h) = c.target(&Url::parse("http://api.warren.example/v1/health").unwrap());
        assert_eq!(t.as_str(), "http://[::1]/v1/health", "no port in connectVia keeps the URL's port");
        assert_eq!(h.unwrap().to_str().unwrap(), "api.warren.example", "default port is left out of Host");
        assert!(check_id("ab12cd34ef").is_ok());
        assert!(check_id("../evil").is_err());
        assert!(check_id("").is_err());
    }

    #[tokio::test]
    async fn direct_requests_carry_bearer_auth_and_the_servers_host() {
        let srv = TestServer::start(canned()).await;
        let c = WarrenClient::new(&format!("http://127.0.0.1:{}/v1", srv.port), "sk_live_good", None).unwrap();
        let h = c.health().await.unwrap();
        assert!(h.healthy && h.dev_mode && h.version == "0.1.0");
        let list = c.instances().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, "default");
        let ws = c.workspace().await.unwrap();
        assert_eq!(ws.awake_limit, 3);
        let r = c.exec("ab12cd34ef", &wire::ExecRequest::new("echo ok")).await.unwrap();
        assert_eq!(r.stdout, "ok\n");
        let s = c.stop("ab12cd34ef").await.unwrap();
        assert_eq!(s.status, "stopped");
        let su = c.signed_url("ab12cd34ef", Some(TERMINAL_PORT), Some(600)).await.unwrap();
        assert_eq!(su.expires_at, 99);
        let reqs = srv.recorded();
        assert_eq!(reqs[0].path, "/v1/health");
        assert!(reqs[0].header("authorization").is_none(), "health is anonymous");
        assert_eq!(reqs[0].header("host"), Some(format!("127.0.0.1:{}", srv.port).as_str()));
        assert_eq!(reqs[1].header("authorization"), Some("Bearer sk_live_good"));
        let exec = &reqs[3];
        assert_eq!(exec.header("content-type"), Some("application/json"));
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&exec.body).unwrap(), serde_json::json!({ "command": "echo ok" }), "only set fields are sent");
        assert_eq!(reqs[4].body, b"{}", "lifecycle posts an empty object");
        assert_eq!(reqs[5].query.as_deref(), Some("port=7681&ttl=600"));
    }

    #[tokio::test]
    async fn connect_via_sends_everything_to_the_tunnel_with_logical_hosts() {
        let srv = TestServer::start(canned()).await;
        let c = WarrenClient::new("http://api.warren.localhost:7337/v1", "sk_live_good", Some(&format!("127.0.0.1:{}", srv.port))).unwrap();
        let inst = c.instance("ab12cd34ef").await.unwrap();
        assert_eq!(inst.id, "ab12cd34ef");
        let bytes = c.files_read("ab12cd34ef", "a/b.txt").await.unwrap();
        assert_eq!(&bytes[..], b"hello");
        let w = c.files_write("ab12cd34ef", "a/b.txt", b"hello".to_vec()).await.unwrap();
        assert_eq!(w.size, 5);
        let entries = c.files_list("ab12cd34ef", ".").await.unwrap();
        assert_eq!(entries[0].kind, "file");
        c.files_delete("ab12cd34ef", "a").await.unwrap();
        let reqs = srv.recorded();
        assert_eq!(reqs[0].header("host"), Some("api.warren.localhost:7337"));
        assert_eq!(reqs[0].path, "/v1/instances/ab12cd34ef");
        assert_eq!(reqs[0].header("authorization"), Some("Bearer sk_live_good"));
        let read = &reqs[1];
        assert_eq!(read.header("host"), Some("ab12cd34ef.warren.localhost:7337"));
        assert_eq!((read.method.as_str(), read.path.as_str(), read.query.as_deref()), ("GET", "/v1/files/content", Some("path=a%2Fb.txt")));
        assert_eq!(read.header("x-warren-key"), Some("sk_live_good"));
        assert!(read.header("authorization").is_none(), "instance hosts use X-Warren-Key only");
        let write = &reqs[2];
        assert_eq!((write.method.as_str(), write.header("content-type"), &write.body[..]), ("PUT", Some("application/octet-stream"), &b"hello"[..]));
        assert_eq!(reqs[3].query.as_deref(), Some("path=."));
        assert_eq!((reqs[4].method.as_str(), reqs[4].path.as_str()), ("DELETE", "/v1/files"));
    }

    #[tokio::test]
    async fn errors_keep_warrens_code_and_name_the_key_on_401() {
        let srv = TestServer::start(canned()).await;
        let bad = WarrenClient::new(&format!("http://127.0.0.1:{}/v1", srv.port), "sk_live_bad", None).unwrap();
        let e = bad.workspace().await.unwrap_err();
        assert_eq!(e.code, "invalid_api_key");
        assert!(e.message.contains("Invalid Warren API key") && e.message.contains("Invalid API key."), "{}", e.message);
        let e = bad.instance("zzzzzzzzzz").await.unwrap_err();
        assert_eq!((e.code.as_str(), e.message.as_str()), ("not_found", "instance not found"));
        let e = bad.instance("boom000000").await.unwrap_err();
        assert_eq!(e.code, "http_500");
        assert!(e.message.contains("HTTP 500") && e.message.contains("nope"), "{}", e.message);
        let e = bad.instance("../x").await.unwrap_err();
        assert_eq!(e.code, "invalid");
        drop(srv);
        let gone = WarrenClient::new("http://127.0.0.1:1/v1", "k", None).unwrap();
        let e = gone.health().await.unwrap_err();
        assert_eq!(e.code, "connect");
        assert!(e.message.contains("tunnel"), "{}", e.message);
    }

    fn status_sequence(statuses: &'static [&'static str]) -> (Handler, Arc<AtomicUsize>) {
        let n = Arc::new(AtomicUsize::new(0));
        let calls = n.clone();
        let h: Handler = Arc::new(move |_r| {
            let i = calls.fetch_add(1, Ordering::SeqCst);
            let s = statuses[i.min(statuses.len() - 1)];
            (200, format!(r#"{{"id":"ab12cd34ef","status":"{s}","status_reason":"{}"}}"#, if s == "failed" { "image pull failed" } else { "" }))
        });
        (h, n)
    }

    #[tokio::test]
    async fn wait_running_polls_until_running() {
        let (h, calls) = status_sequence(&["provisioning", "starting", "running"]);
        let srv = TestServer::start(h).await;
        let c = WarrenClient::new(&format!("http://127.0.0.1:{}/v1", srv.port), "k", None).unwrap().with_poll_interval(Duration::from_millis(5));
        let inst = c.wait_running("ab12cd34ef", Duration::from_secs(5)).await.unwrap();
        assert_eq!(inst.status, "running");
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn wait_running_reports_failed_and_timeout() {
        let (h, _) = status_sequence(&["provisioning", "failed"]);
        let srv = TestServer::start(h).await;
        let c = WarrenClient::new(&format!("http://127.0.0.1:{}/v1", srv.port), "k", None).unwrap().with_poll_interval(Duration::from_millis(5));
        let e = c.wait_running("ab12cd34ef", Duration::from_secs(5)).await.unwrap_err();
        assert_eq!(e.code, "instance_failed");
        assert!(e.message.contains("image pull failed"), "{}", e.message);
        let (h, _) = status_sequence(&["provisioning"]);
        let srv = TestServer::start(h).await;
        let c = WarrenClient::new(&format!("http://127.0.0.1:{}/v1", srv.port), "k", None).unwrap().with_poll_interval(Duration::from_millis(5));
        let e = c.wait_running("ab12cd34ef", Duration::from_millis(40)).await.unwrap_err();
        assert_eq!(e.code, "timeout");
        assert!(e.message.contains("last status: provisioning"), "{}", e.message);
        let (h, calls) = status_sequence(&["sleeping"]);
        let srv = TestServer::start(h).await;
        let c = WarrenClient::new(&format!("http://127.0.0.1:{}/v1", srv.port), "k", None).unwrap().with_poll_interval(Duration::from_millis(5));
        let e = c.wait_running("ab12cd34ef", Duration::from_secs(5)).await.unwrap_err();
        assert_eq!(e.code, "conflict");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "a sleeping instance is not polled");
    }

    /// Instance reads return `first` until the server has seen a request whose path ends in `wake_suffix`, then `running`.
    fn wakes_on(first: &'static str, wake_suffix: &'static str) -> (Handler, Arc<AtomicUsize>) {
        let woke = Arc::new(AtomicUsize::new(0));
        let w = woke.clone();
        let h: Handler = Arc::new(move |r| {
            if r.method == "POST" && r.path.ends_with(wake_suffix) { w.store(1, Ordering::SeqCst); return (200, r#"{"id":"ab12cd34ef","status":"starting"}"#.into()); }
            if r.method == "POST" { return (409, r#"{"error":{"code":"conflict","message":"unexpected call"}}"#.into()); }
            let s = if w.load(Ordering::SeqCst) == 1 { "running" } else { first };
            (200, format!(r#"{{"id":"ab12cd34ef","status":"{s}"}}"#))
        });
        (h, woke)
    }

    #[tokio::test]
    async fn ensure_awake_wakes_sleeping_and_starts_stopped_instances() {
        let (h, _) = wakes_on("sleeping", "/exec");
        let srv = TestServer::start(h).await;
        let c = WarrenClient::new(&format!("http://127.0.0.1:{}/v1", srv.port), "k", None).unwrap().with_poll_interval(Duration::from_millis(5));
        assert_eq!(c.ensure_awake("ab12cd34ef", Duration::from_secs(90)).await.unwrap().status, "running");
        let reqs = srv.recorded();
        let exec = reqs.iter().find(|r| r.path.ends_with("/exec")).expect("woken by an exec");
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&exec.body).unwrap(), serde_json::json!({ "command": "true", "timeout_seconds": 90 }));
        let (h, _) = wakes_on("stopped", "/start");
        let srv = TestServer::start(h).await;
        let c = WarrenClient::new(&format!("http://127.0.0.1:{}/v1", srv.port), "k", None).unwrap().with_poll_interval(Duration::from_millis(5));
        assert_eq!(c.ensure_awake("ab12cd34ef", Duration::from_secs(5)).await.unwrap().status, "running");
        assert!(srv.recorded().iter().any(|r| r.path.ends_with("/start")));
        let (h, _) = wakes_on("running", "/never");
        let srv = TestServer::start(h).await;
        let c = WarrenClient::new(&format!("http://127.0.0.1:{}/v1", srv.port), "k", None).unwrap();
        c.ensure_awake("ab12cd34ef", Duration::from_secs(5)).await.unwrap();
        assert_eq!(srv.recorded().len(), 1, "a running instance costs one GET");
    }
}

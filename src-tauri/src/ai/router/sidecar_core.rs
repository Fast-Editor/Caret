//! App-free sidecar core: writes the `.env`, spawns `node <lynkr>/index.js`, waits for `/health`, keeps a monitor
//! that restarts with backoff, and exposes the endpoint only while healthy. `RouterManager` wraps this with the
//! Tauri app (settings, bridge, events); tests drive it directly against `tests/fixtures/fake_lynkr.mjs`.
use crate::ai::router::env::{price_overrides_json, render_env, EnvConfig};
use crate::ai::router::runtime::LynkrRuntime;
use crate::ai::router::{Assignment, Endpoint, RouterStatus};
use crate::core::settings::ModelConfig;
use crate::error::{CaretError, Result};
use parking_lot::{Mutex, RwLock};
use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};

const HEALTH_POLL: Duration = Duration::from_millis(500);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(30);
const MONITOR_EVERY: Duration = Duration::from_secs(10);
const RESTART_BACKOFF_SECS: [u64; 3] = [1, 3, 9];
const LOG_LINES: usize = 500;

#[derive(Debug, Clone)]
pub struct BridgeInfo { pub port: u16, pub token: String }

/// Everything needed to configure and launch one sidecar process.
#[derive(Debug, Clone)]
pub struct StartSpec {
    pub assignment: Assignment,
    pub models: Vec<ModelConfig>,
    pub sticky_sessions: bool,
    pub tpm_limit: Option<u32>,
    /// (Ollama base URL, embeddings model) when an Ollama provider is configured for Lynkr embeddings.
    pub ollama_embeddings: Option<(String, String)>,
    pub bridge: BridgeInfo,
    pub version: Option<String>,
    pub native_ok: bool,
    /// Installed Lynkr predates the loopback-bind/API-key patch.
    pub lan_warning: bool,
    /// Extra environment for the child (tests only).
    pub extra_env: Vec<(String, String)>,
}
impl StartSpec {
    /// Anything in here needs a process restart: Lynkr parses `MODEL_PRICE_OVERRIDES` once and `PORT` is fixed.
    pub fn restart_signature(&self) -> String {
        format!("{}|{}|{:?}|{:?}|{}|{}", price_overrides_json(&self.assignment, &self.models), self.sticky_sessions, self.tpm_limit, self.ollama_embeddings, self.bridge.port, self.bridge.token)
    }
    /// Differences only here (the `TIER_*` lines) are picked up by Lynkr's `.env` hot reload.
    pub fn env_signature(&self) -> String { format!("{}|{}", self.assignment.hash, self.restart_signature()) }
    fn warnings(&self) -> Vec<String> {
        let mut w = self.assignment.warnings.clone();
        if self.lan_warning { w.push("Installed Lynkr predates the loopback patch: the sidecar is reachable from your LAN (provider keys stay behind the bridge token).".into()); }
        if !self.native_ok { w.push("better-sqlite3 is unavailable: session pins, telemetry and de-escalation are off.".into()); }
        w
    }
}

pub type StatusSink = Arc<dyn Fn(&RouterStatus) + Send + Sync>;

pub struct Sidecar {
    runtime: RwLock<LynkrRuntime>,
    status: RwLock<RouterStatus>,
    logs: Mutex<VecDeque<String>>,
    last_stderr: Mutex<Option<String>>,
    endpoint: RwLock<Option<Endpoint>>,
    spec: Mutex<Option<StartSpec>>,
    api_key: Mutex<Option<String>>,
    port: AtomicU32,
    pid: AtomicU32,
    /// Bumped on every start/stop so stale monitors exit.
    generation: AtomicU64,
    stopping: AtomicBool,
    restarts: AtomicU32,
    sink: StatusSink,
    start_lock: tokio::sync::Mutex<()>,
}

impl Sidecar {
    pub fn new(runtime: LynkrRuntime, sink: StatusSink) -> Arc<Self> {
        Arc::new(Self { runtime: RwLock::new(runtime), status: RwLock::new(RouterStatus::default()), logs: Mutex::new(VecDeque::new()), last_stderr: Mutex::new(None), endpoint: RwLock::new(None), spec: Mutex::new(None), api_key: Mutex::new(None), port: AtomicU32::new(0), pid: AtomicU32::new(0), generation: AtomicU64::new(0), stopping: AtomicBool::new(false), restarts: AtomicU32::new(0), sink, start_lock: tokio::sync::Mutex::new(()) })
    }
    pub fn runtime(&self) -> LynkrRuntime { self.runtime.read().clone() }
    pub fn set_runtime(&self, rt: LynkrRuntime) { *self.runtime.write() = rt; }
    pub fn status(&self) -> RouterStatus { self.status.read().clone() }
    pub fn endpoint(&self) -> Option<Endpoint> { self.endpoint.read().clone() }
    pub fn spec(&self) -> Option<StartSpec> { self.spec.lock().clone() }
    pub fn pid(&self) -> Option<u32> { match self.pid.load(Ordering::SeqCst) { 0 => None, p => Some(p) } }
    pub fn logs(&self) -> Vec<String> { self.logs.lock().iter().cloned().collect() }
    pub fn log(&self, line: impl Into<String>) {
        let line = line.into();
        tracing::debug!(target: "caret::router", "{line}");
        let mut l = self.logs.lock();
        l.push_back(format!("{} {}", chrono::Local::now().format("%H:%M:%S"), line));
        while l.len() > LOG_LINES { l.pop_front(); }
    }
    /// Mutate the status and broadcast it.
    pub fn update_status(&self, f: impl FnOnce(&mut RouterStatus)) {
        let snapshot = { let mut s = self.status.write(); f(&mut s); s.clone() };
        (self.sink)(&snapshot);
    }
    pub fn set_state(&self, state: &str, reason: Option<String>) { self.update_status(|s| { s.state = state.into(); s.reason = reason; }); }

    fn render(&self, spec: &StartSpec, port: u16, api_key: &str) -> String {
        let rt = self.runtime();
        render_env(&EnvConfig {
            port, bridge_messages_url: format!("http://127.0.0.1:{}/v1/messages", spec.bridge.port), bridge_token: spec.bridge.token.clone(), lynkr_api_key: api_key.to_string(),
            assignment: &spec.assignment, models: &spec.models, sticky_sessions: spec.sticky_sessions, tpm_limit: spec.tpm_limit, prefix: rt.prefix(), ollama_embeddings: spec.ollama_embeddings.clone(),
        })
    }

    /// Rewrite `.env` for a running sidecar (Lynkr hot-reloads `TIER_*`); updates the published assignment.
    pub fn rewrite_env(&self, spec: &StartSpec) -> Result<()> {
        let port = self.port.load(Ordering::SeqCst) as u16;
        let key = self.api_key.lock().clone();
        let (Some(key), true) = (key, port != 0) else { return Err(CaretError::internal("sidecar is not running")) };
        let rt = self.runtime();
        write_env_atomic(&rt.prefix().join(".env"), &self.render(spec, port, &key))?;
        if let Some(ep) = self.endpoint.write().as_mut() { ep.assignment = spec.assignment.clone(); }
        *self.spec.lock() = Some(spec.clone());
        self.update_status(|s| { s.assignment = spec.assignment.clone(); s.warnings = spec.warnings(); });
        self.log("[router] .env rewritten (tier assignment changed; Lynkr hot-reloads it)");
        Ok(())
    }

    /// Full (re)start: stops any running child, writes `.env`, spawns, waits for `/health`, then publishes the endpoint.
    pub async fn start(self: &Arc<Self>, spec: StartSpec) -> Result<RouterStatus> {
        self.restarts.store(0, Ordering::SeqCst);
        self.start_inner(spec).await
    }

    /// Boxed (not `async fn`) to break the start → monitor → restart → start type cycle for `Send` inference.
    fn start_inner<'a>(self: &'a Arc<Self>, spec: StartSpec) -> futures::future::BoxFuture<'a, Result<RouterStatus>> {
        Box::pin(async move {
        let _g = self.start_lock.lock().await;
        self.stopping.store(false, Ordering::SeqCst);
        // Bump the generation BEFORE killing the previous child so its monitor sees a superseded generation and exits
        // quietly instead of treating the kill as a crash and restarting (which would ping-pong forever).
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.kill_current().await;
        let rt = self.runtime();
        rt.ensure_dirs()?;
        let node = LynkrRuntime::node_bin().ok_or_else(|| CaretError::not_found(LynkrRuntime::NODE_MISSING))?;
        let entry = rt.entry_js();
        if !entry.is_file() { return Err(CaretError::not_found(format!("Lynkr entry {}", entry.display()))); }
        let port = free_port()?;
        let api_key = random_hex64();
        self.update_status(|s| {
            s.state = "starting".into(); s.reason = None; s.installed = true; s.version = spec.version.clone(); s.native_ok = spec.native_ok; s.bridge_port = Some(spec.bridge.port);
            s.assignment = spec.assignment.clone(); s.warnings = spec.warnings(); s.port = Some(port); s.pid = None;
        });
        write_env_atomic(&rt.prefix().join(".env"), &self.render(&spec, port, &api_key))?;
        *self.api_key.lock() = Some(api_key.clone());
        self.port.store(port as u32, Ordering::SeqCst);
        *self.spec.lock() = Some(spec.clone());

        let mut cmd = tokio::process::Command::new(&node);
        cmd.arg(&entry).current_dir(rt.prefix()).env("PATH", crate::acp::process::augmented_path()).env("NODE_ENV", "production").env("PORT", port.to_string())
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        for (k, v) in &spec.extra_env { cmd.env(k, v); }
        #[cfg(unix)] { cmd.process_group(0); }
        let mut child = cmd.spawn().map_err(|e| { let m = format!("failed to start node: {e}"); self.set_state("error", Some(m.clone())); CaretError::io(m) })?;
        let pid = child.id().unwrap_or(0);
        self.pid.store(pid, Ordering::SeqCst);
        self.log(format!("[router] started lynkr (pid {pid}) on 127.0.0.1:{port}, bridge :{}", spec.bridge.port));
        if let Some(out) = child.stdout.take() { let me = self.clone(); tokio::spawn(async move { let mut l = BufReader::new(out).lines(); while let Ok(Some(line)) = l.next_line().await { if !line.trim().is_empty() { me.log(format!("[lynkr] {}", line.chars().take(2000).collect::<String>())); } } }); }
        if let Some(err) = child.stderr.take() { let me = self.clone(); tokio::spawn(async move { let mut l = BufReader::new(err).lines(); while let Ok(Some(line)) = l.next_line().await { if !line.trim().is_empty() { *me.last_stderr.lock() = Some(line.chars().take(400).collect()); me.log(format!("[lynkr:err] {}", line.chars().take(2000).collect::<String>())); } } }); }

        // Wait for /health.
        let started = std::time::Instant::now();
        loop {
            if let Ok(Some(st)) = child.try_wait() {
                let reason = format!("Lynkr exited during startup (code {}){}", st.code().unwrap_or(-1), self.last_stderr.lock().as_deref().map(|l| format!(": {l}")).unwrap_or_default());
                self.pid.store(0, Ordering::SeqCst); self.update_status(|s| { s.state = "error".into(); s.reason = Some(reason.clone()); s.pid = None; });
                return Err(CaretError::io(reason));
            }
            if health_ok(port).await { break; }
            if started.elapsed() > HEALTH_TIMEOUT {
                let reason = format!("Lynkr did not become healthy within {}s{}", HEALTH_TIMEOUT.as_secs(), self.last_stderr.lock().as_deref().map(|l| format!(": {l}")).unwrap_or_default());
                let _ = child.start_kill(); let _ = child.wait().await; self.pid.store(0, Ordering::SeqCst);
                self.update_status(|s| { s.state = "error".into(); s.reason = Some(reason.clone()); s.pid = None; });
                return Err(CaretError::io(reason));
            }
            tokio::time::sleep(HEALTH_POLL).await;
        }
        *self.endpoint.write() = Some(Endpoint { base_url: format!("http://127.0.0.1:{port}"), api_key: api_key.clone(), assignment: spec.assignment.clone() });
        self.update_status(|s| { s.state = "healthy".into(); s.reason = None; s.pid = Some(pid); s.port = Some(port); });
        self.log(format!("[router] healthy after {} ms", started.elapsed().as_millis()));
        let me = self.clone();
        tokio::spawn(async move { me.monitor(gen, child, spec).await; });
        Ok(self.status())
        })
    }

    /// Health loop for one child; restarts with backoff on exit or two missed checks.
    async fn monitor(self: Arc<Self>, gen: u64, mut child: tokio::process::Child, spec: StartSpec) {
        let mut misses = 0u32;
        let exit_reason: String = loop {
            tokio::select! {
                res = child.wait() => {
                    if self.generation.load(Ordering::SeqCst) != gen || self.stopping.load(Ordering::SeqCst) { return; }
                    let code = res.ok().and_then(|s| s.code()).unwrap_or(-1);
                    break format!("Lynkr exited (code {code}){}", self.last_stderr.lock().as_deref().map(|l| format!(": {l}")).unwrap_or_default());
                }
                _ = tokio::time::sleep(MONITOR_EVERY) => {
                    if self.generation.load(Ordering::SeqCst) != gen || self.stopping.load(Ordering::SeqCst) { return; }
                    if health_ok(self.port.load(Ordering::SeqCst) as u16).await { misses = 0; continue; }
                    misses += 1; self.log(format!("[router] health check missed ({misses}/2)"));
                    if misses >= 2 { let _ = child.start_kill(); let _ = child.wait().await; break "Lynkr stopped answering /health".to_string(); }
                }
            }
        };
        self.pid.store(0, Ordering::SeqCst);
        *self.endpoint.write() = None;
        self.log(format!("[router] {exit_reason}"));
        self.restart_loop(spec, exit_reason, gen).await;
    }

    /// Restart with backoff after a crash; gives up when a newer start/stop superseded generation `gen`.
    async fn restart_loop(self: &Arc<Self>, spec: StartSpec, mut last: String, gen: u64) {
        loop {
            if self.stopping.load(Ordering::SeqCst) || self.generation.load(Ordering::SeqCst) != gen { return; }
            let attempt = self.restarts.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt as usize > RESTART_BACKOFF_SECS.len() {
                self.update_status(|s| { s.state = "error".into(); s.reason = Some(format!("Lynkr keeps exiting; giving up after {} restarts. Last error: {last}", RESTART_BACKOFF_SECS.len())); s.pid = None; });
                return;
            }
            let delay = RESTART_BACKOFF_SECS[attempt as usize - 1];
            self.update_status(|s| { s.state = "starting".into(); s.reason = Some(format!("{last} — restarting in {delay}s (attempt {attempt}/{})", RESTART_BACKOFF_SECS.len())); s.pid = None; });
            tokio::time::sleep(Duration::from_secs(delay)).await;
            if self.stopping.load(Ordering::SeqCst) || self.generation.load(Ordering::SeqCst) != gen { return; }
            match self.start_inner(spec.clone()).await { Ok(_) => return, Err(e) => { last = e.message; } }
        }
    }

    async fn kill_current(&self) {
        let pid = self.pid.swap(0, Ordering::SeqCst);
        *self.endpoint.write() = None;
        if pid != 0 { self.log(format!("[router] stopping lynkr (pid {pid})")); crate::acp::process::kill_pid(pid).await; }
    }

    /// Stop and clear the endpoint; status left for the caller to set.
    pub async fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.kill_current().await;
        self.port.store(0, Ordering::SeqCst);
        self.update_status(|s| { s.pid = None; s.port = None; });
    }

    /// Synchronous stop for app exit / the Stop button: SIGTERM now, SIGKILL after 3 s in the background.
    pub fn stop_sync(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
        let pid = self.pid.swap(0, Ordering::SeqCst);
        *self.endpoint.write() = None;
        self.port.store(0, Ordering::SeqCst);
        if pid != 0 {
            self.log(format!("[router] stopping lynkr (pid {pid})"));
            kill_signal(pid, "-TERM");
            std::thread::spawn(move || { std::thread::sleep(Duration::from_secs(3)); kill_signal(pid, "-KILL"); });
        }
        self.update_status(|s| { s.pid = None; s.port = None; });
    }
}

fn kill_signal(pid: u32, sig: &str) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill").args([sig, &format!("-{pid}")]).stdout(Stdio::null()).stderr(Stdio::null()).status();
        let _ = std::process::Command::new("kill").args([sig, &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(not(unix))]
    { let _ = (pid, sig); }
}

static HTTP: once_cell::sync::Lazy<reqwest::Client> = once_cell::sync::Lazy::new(|| reqwest::Client::builder().timeout(Duration::from_secs(2)).no_proxy().build().expect("http client"));

pub async fn health_ok(port: u16) -> bool {
    if port == 0 { return false; }
    matches!(HTTP.get(format!("http://127.0.0.1:{port}/health")).send().await, Ok(r) if r.status().is_success())
}

/// Ask the OS for a free loopback port (bind :0, read, release).
pub fn free_port() -> Result<u16> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| CaretError::io(format!("no free loopback port: {e}")))?;
    Ok(l.local_addr().map_err(|e| CaretError::io(e.to_string()))?.port())
}

/// 64 hex chars from two v4 UUIDs (CSPRNG-backed).
pub fn random_hex64() -> String { format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple()) }

/// Write `contents` to `path` atomically (temp + rename) with owner-only permissions.
pub fn write_env_atomic(path: &Path, contents: &str) -> Result<()> {
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir)?; }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; opts.mode(0o600); }
        let mut f = opts.open(&tmp)?;
        f.write_all(contents.as_bytes())?; f.sync_all()?;
    }
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)); }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::router::{Tier, TierSlot};
    use std::path::PathBuf;

    fn fixture() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_lynkr.mjs") }
    fn model(id: &str, ctx: u32, inp: f64) -> ModelConfig { ModelConfig { id: id.into(), model: id.into(), display_name: id.into(), context_window: ctx, max_output: 8192, input_price_per_m: Some(inp), output_price_per_m: Some(inp * 5.0), enabled: true, ..Default::default() } }
    fn spec(extra_env: Vec<(String, String)>) -> StartSpec {
        let mut a = Assignment::default();
        for (t, id) in [(Tier::Simple, "haiku"), (Tier::Medium, "sonnet"), (Tier::Complex, "sonnet"), (Tier::Reasoning, "opus")] { a.slots.insert(t.key().into(), TierSlot { model_id: id.into(), source: "auto".into(), reason: None }); }
        a.hash = "deadbeef".into();
        StartSpec { assignment: a, models: vec![model("haiku", 200_000, 1.0), model("sonnet", 1_000_000, 3.0), model("opus", 1_000_000, 15.0)], sticky_sessions: true, tpm_limit: None, ollama_embeddings: None, bridge: BridgeInfo { port: 1, token: "tok".into() }, version: Some("9.14.22".into()), native_ok: false, lan_warning: true, extra_env }
    }
    fn sidecar(prefix: &Path) -> (Arc<Sidecar>, Arc<Mutex<Vec<String>>>) {
        let seen = Arc::new(Mutex::new(vec![]));
        let s2 = seen.clone();
        let sink: StatusSink = Arc::new(move |s: &RouterStatus| s2.lock().push(s.state.clone()));
        (Sidecar::new(LynkrRuntime::managed_at(prefix.to_path_buf(), Some(fixture())), sink), seen)
    }
    fn alive(pid: u32) -> bool { std::process::Command::new("kill").args(["-0", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false) }

    #[test]
    fn free_ports_are_bindable_and_distinct() {
        let a = free_port().unwrap(); let b = free_port().unwrap();
        assert!(a > 1024 && b > 1024);
        let _l = std::net::TcpListener::bind(("127.0.0.1", a)).expect("port is free");
        assert!(random_hex64().len() == 64 && random_hex64() != random_hex64());
    }

    #[test]
    fn env_is_written_atomically_with_owner_only_mode() {
        let dir = tempfile::tempdir().unwrap(); let p = dir.path().join("lynkr/.env");
        write_env_atomic(&p, "PORT=1\n").unwrap();
        write_env_atomic(&p, "PORT=2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "PORT=2\n");
        assert!(!dir.path().join("lynkr").read_dir().unwrap().any(|e| e.unwrap().file_name().to_string_lossy().starts_with(".tmp")));
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600); }
    }

    #[test]
    fn restart_signature_ignores_tier_swaps_but_not_prices() {
        let a = spec(vec![]);
        let mut b = a.clone(); b.assignment.slots.insert("simple".into(), TierSlot { model_id: "sonnet".into(), source: "override".into(), reason: None }); b.assignment.hash = "other".into();
        assert_eq!(a.restart_signature(), b.restart_signature(), "same model set → hot reload only");
        assert_ne!(a.env_signature(), b.env_signature());
        let mut c = a.clone(); c.models[0].input_price_per_m = Some(9.0);
        assert_ne!(a.restart_signature(), c.restart_signature(), "price change → restart");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn starts_fake_lynkr_publishes_endpoint_and_stops() {
        if LynkrRuntime::node_bin().is_none() { eprintln!("node not found; skipping"); return; }
        let dir = tempfile::tempdir().unwrap();
        let (sc, seen) = sidecar(dir.path());
        let st = sc.start(spec(vec![])).await.expect("start");
        assert_eq!(st.state, "healthy"); assert!(st.pid.is_some() && st.port.is_some());
        let ep = sc.endpoint().expect("endpoint while healthy");
        assert_eq!(ep.assignment.get(Tier::Simple).unwrap().model_id, "haiku"); assert_eq!(ep.api_key.len(), 64);
        let env = std::fs::read_to_string(dir.path().join(".env")).unwrap();
        assert!(env.contains(&format!("PORT={}", st.port.unwrap())) && env.contains("TIER_SIMPLE=azure-anthropic:haiku") && env.contains(&format!("LYNKR_API_KEY='{}'", ep.api_key)));
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(dir.path().join(".env")).unwrap().permissions().mode() & 0o777, 0o600); }
        // the fake echoes /v1/messages with routing headers
        let r = HTTP.post(format!("{}/v1/messages", ep.base_url)).body("{}").send().await.unwrap();
        assert_eq!(r.headers().get("x-lynkr-model").unwrap(), "haiku");
        assert!(st.warnings.iter().any(|w| w.contains("LAN")), "unpatched Lynkr warns about LAN exposure");
        // hot rewrite keeps the process, changes the published assignment
        let mut s2 = spec(vec![]); s2.assignment.slots.insert("simple".into(), TierSlot { model_id: "sonnet".into(), source: "override".into(), reason: None }); s2.assignment.hash = "x".into();
        sc.rewrite_env(&s2).unwrap();
        assert_eq!(sc.endpoint().unwrap().assignment.get(Tier::Simple).unwrap().model_id, "sonnet");
        assert!(std::fs::read_to_string(dir.path().join(".env")).unwrap().contains("TIER_SIMPLE=azure-anthropic:sonnet"));
        assert_eq!(sc.status().pid, st.pid, "rewrite must not restart");
        let pid = st.pid.unwrap();
        sc.stop().await;
        assert!(sc.endpoint().is_none()); assert!(!alive(pid), "child killed");
        assert!(seen.lock().iter().any(|s| s == "starting") && seen.lock().iter().any(|s| s == "healthy"));
    }

    /// Crash → automatic restart with backoff (≈ 1 s). `CARET_LIVE_TEST=1 cargo test --lib router::sidecar_core::tests::restarts`
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn restarts_after_crash_with_backoff() {
        if std::env::var("CARET_LIVE_TEST").ok().as_deref() != Some("1") || LynkrRuntime::node_bin().is_none() { return; }
        let dir = tempfile::tempdir().unwrap();
        let (sc, _) = sidecar(dir.path());
        let st = sc.start(spec(vec![("FAKE_LYNKR_EXIT_AFTER_MS".into(), "800".into())])).await.unwrap();
        let first = st.pid.unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            let s = sc.status();
            if s.state == "healthy" && s.pid != Some(first) { break; }
            assert!(std::time::Instant::now() < deadline, "no restart observed; status {s:?}");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        assert!(sc.logs().iter().any(|l| l.contains("Lynkr exited")));
        sc.stop().await;
    }
}

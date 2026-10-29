//! Debug adapters Caret knows how to install and launch. Installed under `~/.caret/debug/<id>` — never globally.
use crate::error::{CaretError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};

pub const JS_DEBUG: &str = "js-debug";
pub const DEBUGPY: &str = "debugpy";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterStatus {
    pub id: String, pub name: String, pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub path: Option<String>,
    pub install_hint: String, pub types: Vec<String>, pub languages: Vec<String>,
    /// Runtime the adapter needs (`node` / `python3`) and whether it was found on PATH.
    pub runtime: String, pub runtime_found: bool,
}

pub fn debug_home() -> PathBuf { crate::paths::sub_dir("debug") }
fn js_debug_dir() -> PathBuf { debug_home().join("js-debug") }
fn js_debug_entry() -> PathBuf { js_debug_dir().join("js-debug").join("src").join("dapDebugServer.js") }
fn python_site() -> PathBuf { debug_home().join("python") }

/// Map a launch configuration `type` to an adapter id.
pub fn adapter_for_type(kind: &str) -> Option<&'static str> {
    match kind { "node" | "pwa-node" | "node-terminal" | "chrome" | "pwa-chrome" | "msedge" | "pwa-msedge" | "extensionHost" => Some(JS_DEBUG), "python" | "debugpy" => Some(DEBUGPY), _ => None }
}

/// js-debug speaks `pwa-node` / `pwa-chrome` internally; translate the friendly VS Code names.
pub fn normalize_type(kind: &str) -> String {
    match kind { "node" => "pwa-node".into(), "chrome" => "pwa-chrome".into(), "msedge" => "pwa-msedge".into(), "python" => "debugpy".into(), other => other.into() }
}

fn node() -> Option<PathBuf> { crate::acp::process::resolve_command("node") }
fn python() -> Option<PathBuf> { crate::acp::process::resolve_command("python3").or_else(|| crate::acp::process::resolve_command("python")) }

fn js_debug_version() -> Option<String> {
    let pkg = std::fs::read_to_string(js_debug_dir().join("js-debug").join("package.json")).ok()?;
    serde_json::from_str::<serde_json::Value>(&pkg).ok()?.get("version")?.as_str().map(|s| s.to_string())
        .or_else(|| std::fs::read_to_string(js_debug_dir().join("VERSION")).ok().map(|s| s.trim().to_string()))
}

/// Probing debugpy means spawning a Python interpreter (100–250 ms, and a process spawn briefly stalls the whole app
/// on macOS). Status is polled by the Debug panel and the Settings pages, so the answer is cached for a minute and
/// dropped whenever an install runs.
static DEBUGPY_PROBE: std::sync::Mutex<Option<(std::time::Instant, Option<String>)>> = std::sync::Mutex::new(None);
const DEBUGPY_PROBE_TTL: std::time::Duration = std::time::Duration::from_secs(60);

pub fn invalidate_probe_cache() { *DEBUGPY_PROBE.lock().unwrap_or_else(|e| e.into_inner()) = None; }

fn debugpy_version() -> Option<String> {
    if let Some((at, v)) = DEBUGPY_PROBE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() { if at.elapsed() < DEBUGPY_PROBE_TTL { return v.clone(); } }
    let v = python().and_then(|py| {
        let out = std::process::Command::new(py).args(["-c", "import debugpy, sys; print(debugpy.__version__)"]).env("PYTHONPATH", python_site()).output().ok()?;
        if out.status.success() { Some(String::from_utf8_lossy(&out.stdout).trim().to_string()) } else { None }
    });
    *DEBUGPY_PROBE.lock().unwrap_or_else(|e| e.into_inner()) = Some((std::time::Instant::now(), v.clone()));
    v
}

pub fn status_all() -> Vec<AdapterStatus> {
    let js_ver = if js_debug_entry().is_file() { js_debug_version().or_else(|| Some("installed".into())) } else { None };
    let py_ver = debugpy_version();
    vec![
        AdapterStatus { id: JS_DEBUG.into(), name: "JavaScript / TypeScript (vscode-js-debug)".into(), installed: js_ver.is_some(), version: js_ver, path: js_debug_entry().is_file().then(|| js_debug_entry().to_string_lossy().to_string()),
            install_hint: "Downloads js-debug-dap from github.com/microsoft/vscode-js-debug releases (needs Node ≥ 18)".into(), types: vec!["node".into(), "pwa-node".into(), "chrome".into()], languages: vec!["javascript".into(), "typescript".into()], runtime: "node".into(), runtime_found: node().is_some() },
        AdapterStatus { id: DEBUGPY.into(), name: "Python (debugpy)".into(), installed: py_ver.is_some(), version: py_ver, path: Some(python_site().to_string_lossy().to_string()),
            install_hint: "pip install --target ~/.caret/debug/python debugpy (needs python3)".into(), types: vec!["python".into(), "debugpy".into()], languages: vec!["python".into()], runtime: "python3".into(), runtime_found: python().is_some() },
    ]
}

pub fn status(id: &str) -> Result<AdapterStatus> { status_all().into_iter().find(|a| a.id == id).ok_or_else(|| CaretError::not_found(format!("debug adapter {id}"))) }

async fn run_logged(cmd: &mut tokio::process::Command, what: &str, log: &(dyn Fn(String) + Send + Sync)) -> Result<()> {
    log(format!("[install] {what}"));
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).env("PATH", crate::acp::process::augmented_path());
    let mut child = cmd.spawn().map_err(|e| CaretError::io(format!("{what}: {e}")))?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    if let Some(o) = child.stdout.take() { let tx = tx.clone(); tokio::spawn(async move { let mut l = BufReader::new(o).lines(); while let Ok(Some(line)) = l.next_line().await { let _ = tx.send(line); } }); }
    if let Some(e) = child.stderr.take() { let tx = tx.clone(); tokio::spawn(async move { let mut l = BufReader::new(e).lines(); while let Ok(Some(line)) = l.next_line().await { let _ = tx.send(line); } }); }
    drop(tx);
    let mut tail: Vec<String> = vec![];
    let wait = async { while let Some(l) = rx.recv().await { if !l.trim().is_empty() { tail.push(l.clone()); if tail.len() > 20 { tail.remove(0); } log(format!("[install] {l}")); } } child.wait().await };
    let status = tokio::time::timeout(std::time::Duration::from_secs(900), wait).await.map_err(|_| CaretError::io(format!("{what} timed out")))?.map_err(|e| CaretError::io(e.to_string()))?;
    if !status.success() { return Err(CaretError::io(format!("{what} failed (exit {}): {}", status.code().unwrap_or(-1), tail.join(" | ").chars().take(600).collect::<String>()))); }
    Ok(())
}

/// Install (or update) an adapter. Streams progress lines through `log`.
pub async fn install(id: &str, log: Arc<dyn Fn(String) + Send + Sync>) -> Result<AdapterStatus> {
    invalidate_probe_cache();
    let l = log.as_ref();
    match id {
        JS_DEBUG => {
            node().ok_or_else(|| CaretError::not_found("Node.js (install from nodejs.org, nvm or Homebrew)"))?;
            let client = crate::ai::providers::http_client()?;
            l("[install] resolving the latest vscode-js-debug release…".into());
            let rel: serde_json::Value = client.get("https://api.github.com/repos/microsoft/vscode-js-debug/releases/latest").header("user-agent", "caret-editor").send().await.map_err(|e| CaretError::io(format!("GitHub API: {e}")))?.json().await.map_err(|e| CaretError::io(format!("GitHub API JSON: {e}")))?;
            let tag = rel.get("tag_name").and_then(|t| t.as_str()).unwrap_or("").to_string();
            let asset = rel.get("assets").and_then(|a| a.as_array()).and_then(|a| a.iter().find(|x| x.get("name").and_then(|n| n.as_str()).map(|n| n.starts_with("js-debug-dap-") && n.ends_with(".tar.gz")).unwrap_or(false))).cloned()
                .ok_or_else(|| CaretError::not_found("js-debug-dap-*.tar.gz asset in the latest release"))?;
            let url = asset["browser_download_url"].as_str().unwrap_or("").to_string();
            l(format!("[install] downloading {} ({tag})…", asset["name"].as_str().unwrap_or("js-debug")));
            let bytes = client.get(&url).header("user-agent", "caret-editor").send().await.map_err(|e| CaretError::io(format!("download: {e}")))?.bytes().await.map_err(|e| CaretError::io(format!("download: {e}")))?;
            let dir = js_debug_dir(); let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir)?;
            let tgz = dir.join("js-debug-dap.tar.gz"); std::fs::write(&tgz, &bytes)?;
            run_logged(tokio::process::Command::new("tar").args(["xzf", &tgz.to_string_lossy()]).current_dir(&dir), "tar xzf js-debug-dap.tar.gz", l).await?;
            let _ = std::fs::remove_file(&tgz);
            if !js_debug_entry().is_file() { return Err(CaretError::io(format!("js-debug extracted but {} is missing", js_debug_entry().display()))); }
            std::fs::write(dir.join("VERSION"), tag.trim_start_matches('v'))?;
            l(format!("[install] js-debug {tag} ready"));
        }
        DEBUGPY => {
            let py = python().ok_or_else(|| CaretError::not_found("python3 (install from python.org or Homebrew)"))?;
            std::fs::create_dir_all(python_site())?;
            run_logged(tokio::process::Command::new(&py).args(["-m", "pip", "install", "--upgrade", "--target", &python_site().to_string_lossy(), "debugpy"]), "python3 -m pip install --target ~/.caret/debug/python debugpy", l).await?;
            if debugpy_version().is_none() { return Err(CaretError::io("debugpy installed but `import debugpy` fails")); }
            l(format!("[install] debugpy {} ready", debugpy_version().unwrap_or_default()));
        }
        other => return Err(CaretError::not_found(format!("debug adapter {other}"))),
    }
    status(id)
}

/// A running adapter process plus how to talk to it.
pub struct Launched {
    pub child: Option<tokio::process::Child>,
    pub transport: Transport,
}
pub enum Transport {
    Stdio { stdin: tokio::process::ChildStdin, stdout: tokio::process::ChildStdout },
    /// A DAP server listening on this loopback port; each session (and js-debug child session) opens its own socket.
    Tcp { port: u16 },
}

fn free_port() -> Result<u16> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| CaretError::io(format!("no free port: {e}")))?;
    Ok(l.local_addr().map_err(|e| CaretError::io(e.to_string()))?.port())
}

/// Start the adapter process for `id`. `cwd` is the workspace (or home).
pub async fn launch(id: &str, cwd: &Path, log: Arc<dyn Fn(String) + Send + Sync>) -> Result<Launched> {
    let cwd = if cwd.is_dir() { cwd.to_path_buf() } else { crate::paths::home_dir() };
    match id {
        JS_DEBUG => {
            let node = node().ok_or_else(|| CaretError::not_found("Node.js"))?;
            let entry = js_debug_entry(); if !entry.is_file() { return Err(CaretError::not_found("js-debug (install it from the Run and Debug view)")); }
            let port = free_port()?;
            let mut cmd = tokio::process::Command::new(&node);
            cmd.arg(&entry).arg(port.to_string()).arg("127.0.0.1").current_dir(&cwd).env("PATH", crate::acp::process::augmented_path()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
            #[cfg(unix)] { cmd.process_group(0); }
            let mut child = cmd.spawn().map_err(|e| CaretError::io(format!("failed to start js-debug: {e}")))?;
            for (name, pipe) in [("stdout", child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>)), ("stderr", child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>))] {
                if let Some(p) = pipe { let log = log.clone(); tokio::spawn(async move { let mut l = BufReader::new(p).lines(); while let Ok(Some(line)) = l.next_line().await { if !line.trim().is_empty() { log(format!("[js-debug {name}] {line}")); } } }); }
            }
            // wait for the server socket
            let started = std::time::Instant::now();
            loop {
                if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() { break; }
                if let Ok(Some(st)) = child.try_wait() { return Err(CaretError::io(format!("js-debug exited during startup (code {})", st.code().unwrap_or(-1)))); }
                if started.elapsed() > std::time::Duration::from_secs(20) { let _ = child.start_kill(); return Err(CaretError::io("js-debug did not start listening within 20s")); }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            log(format!("[debug] js-debug listening on 127.0.0.1:{port}"));
            Ok(Launched { child: Some(child), transport: Transport::Tcp { port } })
        }
        DEBUGPY => {
            let py = python().ok_or_else(|| CaretError::not_found("python3"))?;
            if debugpy_version().is_none() { return Err(CaretError::not_found("debugpy (install it from the Run and Debug view)")); }
            let mut cmd = tokio::process::Command::new(&py);
            cmd.args(["-m", "debugpy.adapter"]).current_dir(&cwd).env("PYTHONPATH", python_site()).env("PATH", crate::acp::process::augmented_path()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
            #[cfg(unix)] { cmd.process_group(0); }
            let mut child = cmd.spawn().map_err(|e| CaretError::io(format!("failed to start debugpy: {e}")))?;
            let stdin = child.stdin.take().ok_or_else(|| CaretError::internal("debugpy stdin"))?;
            let stdout = child.stdout.take().ok_or_else(|| CaretError::internal("debugpy stdout"))?;
            if let Some(e) = child.stderr.take() { let log = log.clone(); tokio::spawn(async move { let mut l = BufReader::new(e).lines(); while let Ok(Some(line)) = l.next_line().await { if !line.trim().is_empty() { log(format!("[debugpy] {line}")); } } }); }
            Ok(Launched { child: Some(child), transport: Transport::Stdio { stdin, stdout } })
        }
        other => Err(CaretError::not_found(format!("debug adapter {other}"))),
    }
}

/// Test/automation hook: an arbitrary stdio adapter command (`CARET_DAP_ADAPTER="node fixtures/fake_dap_adapter.mjs"`).
pub async fn launch_custom(command: &str, args: &[String], cwd: &Path, log: Arc<dyn Fn(String) + Send + Sync>) -> Result<Launched> {
    let exe = crate::acp::process::resolve_command(command).unwrap_or_else(|| PathBuf::from(command));
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(args).current_dir(cwd).env("PATH", crate::acp::process::augmented_path()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(unix)] { cmd.process_group(0); }
    let mut child = cmd.spawn().map_err(|e| CaretError::io(format!("failed to start {command}: {e}")))?;
    let stdin = child.stdin.take().ok_or_else(|| CaretError::internal("adapter stdin"))?;
    let stdout = child.stdout.take().ok_or_else(|| CaretError::internal("adapter stdout"))?;
    if let Some(e) = child.stderr.take() { tokio::spawn(async move { let mut l = BufReader::new(e).lines(); while let Ok(Some(line)) = l.next_line().await { if !line.trim().is_empty() { log(format!("[adapter] {line}")); } } }); }
    Ok(Launched { child: Some(child), transport: Transport::Stdio { stdin, stdout } })
}

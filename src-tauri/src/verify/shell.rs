//! Bounded command execution for checks (user shell, sandbox-wrapped, timeout, capped output).
use crate::core::settings::Settings;
use crate::error::{CaretError, Result};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

pub struct CmdOut { pub output: String, pub code: Option<i32>, pub timed_out: bool, pub truncated: bool }

pub fn user_shell(settings: &Settings) -> String {
    settings.general.default_shell.clone().filter(|s| !s.is_empty()).or_else(|| std::env::var("SHELL").ok()).unwrap_or_else(|| "/bin/zsh".into())
}

pub fn which(bin: &str) -> bool { which::which(bin).is_ok() }

/// Run `command` through the user's login shell inside the sandbox policy derived from settings.
pub async fn run(root: &Path, command: &str, timeout: Duration, cancel: &CancellationToken, settings: &Settings, extra_env: &[(&str, &str)]) -> Result<CmdOut> {
    let shell = user_shell(settings);
    let mut cmd = tokio::process::Command::new(&shell);
    cmd.arg("-lc").arg(command).current_dir(root)
        .env("CARET", "1").env("CARET_VERIFY", "1").env("TERM", "dumb").env("NO_COLOR", "1").env("CI", "1").env("FORCE_COLOR", "0").env("GIT_PAGER", "cat").env("PAGER", "cat");
    for (k, v) in extra_env { cmd.env(k, v); }
    #[cfg(unix)] { cmd.process_group(0); }
    let policy = crate::sandbox::SandboxPolicy { workspace_root: root.to_path_buf(), writable: vec![root.to_path_buf(), std::env::temp_dir()], allow_network: settings.sandbox.allow_network, mode: settings.sandbox.mode.clone() };
    let mut cmd = crate::sandbox::wrap(cmd, &policy);
    // stdio must be configured *after* wrapping: the sandbox rebuilds the Command (std has no stdio getters).
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| CaretError::io(format!("failed to start shell {shell}: {e}")))?;
    let mut out = child.stdout.take().ok_or_else(|| CaretError::internal("verify shell: stdout not piped"))?;
    let mut err = child.stderr.take().ok_or_else(|| CaretError::internal("verify shell: stderr not piped"))?;
    let cap = 200 * 1024;
    let buf = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<u8>::new()));
    let (b1, b2) = (buf.clone(), buf.clone());
    let t1 = tokio::spawn(async move { let mut tmp = [0u8; 8192]; loop { match out.read(&mut tmp).await { Ok(0) | Err(_) => break, Ok(n) => { let mut b = b1.lock(); b.extend_from_slice(&tmp[..n]); if b.len() > cap * 4 { let keep = b.len() - cap * 2; b.drain(cap..cap + keep); } } } } });
    let t2 = tokio::spawn(async move { let mut tmp = [0u8; 8192]; loop { match err.read(&mut tmp).await { Ok(0) | Err(_) => break, Ok(n) => { let mut b = b2.lock(); b.extend_from_slice(&tmp[..n]); if b.len() > cap * 4 { let keep = b.len() - cap * 2; b.drain(cap..cap + keep); } } } } });
    let (code, timed_out) = tokio::select! {
        r = child.wait() => (r.ok().and_then(|s| s.code()), false),
        _ = tokio::time::sleep(timeout) => { kill_group(&mut child).await; (None, true) }
        _ = cancel.cancelled() => { kill_group(&mut child).await; (None, false) }
    };
    let _ = tokio::time::timeout(Duration::from_millis(500), async { let _ = t1.await; let _ = t2.await; }).await;
    let bytes = std::mem::take(&mut *buf.lock());
    let s = String::from_utf8_lossy(&bytes).to_string();
    let (output, truncated) = cap_head_tail(&s, cap);
    Ok(CmdOut { output, code, timed_out, truncated })
}

pub fn cap_head_tail(s: &str, cap: usize) -> (String, bool) {
    if s.len() <= cap { return (s.to_string(), false); }
    let head = cap * 2 / 3; let tail = cap - head;
    let mut h = head; while !s.is_char_boundary(h) { h -= 1; }
    let mut t = s.len() - tail; while !s.is_char_boundary(t) { t += 1; }
    (format!("{}\n\n… [{} bytes omitted] …\n\n{}", &s[..h], s.len() - cap, &s[t..]), true)
}

async fn kill_group(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() { unsafe { libc_kill(-(pid as i32), 15); } tokio::time::sleep(Duration::from_millis(300)).await; unsafe { libc_kill(-(pid as i32), 9); } }
    let _ = child.kill().await;
}
#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) { extern "C" { fn kill(pid: i32, sig: i32) -> i32; } kill(pid, sig); }

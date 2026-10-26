//! Spawning ACP agent processes: command resolution (PATH + `~/.caret/acp/node_modules/.bin` + common Node
//! install locations), environment, and a stderr → log pump.
use crate::acp::AcpAgentConfig;
use crate::error::{CaretError, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

pub struct Spawned { pub child: Child, pub stdin: ChildStdin, pub stdout: ChildStdout }

/// `~/.caret/acp` — where Caret installs npm adapters so they never pollute the user's global prefix.
pub fn acp_home() -> PathBuf { crate::paths::sub_dir("acp") }
pub fn bin_dir() -> PathBuf { acp_home().join("node_modules").join(".bin") }

fn extra_dirs() -> Vec<PathBuf> {
    let home = crate::paths::home_dir();
    let mut v = vec![bin_dir(), home.join(".local/bin"), home.join(".cargo/bin"), home.join(".volta/bin"), home.join(".bun/bin"), home.join(".npm-global/bin"), home.join("Library/pnpm"), home.join(".local/share/pnpm"), PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin"), PathBuf::from("/usr/bin"), PathBuf::from("/bin")];
    // nvm / fnm: newest installed node first
    for base in [home.join(".nvm/versions/node"), home.join("Library/Application Support/fnm/node-versions"), home.join(".local/share/fnm/node-versions")] {
        if let Ok(rd) = std::fs::read_dir(&base) {
            let mut vers: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            vers.sort(); vers.reverse();
            for p in vers.into_iter().take(2) { v.push(p.join("bin")); v.push(p.join("installation/bin")); }
        }
    }
    v
}

/// PATH with Caret's adapter bin dir and common Node locations prepended (GUI apps inherit a minimal PATH on macOS).
pub fn augmented_path() -> String {
    let cur = std::env::var("PATH").unwrap_or_default();
    let mut parts: Vec<String> = extra_dirs().into_iter().filter(|p| p.is_dir()).map(|p| p.to_string_lossy().to_string()).collect();
    for p in cur.split(':') { if !p.is_empty() && !parts.iter().any(|x| x == p) { parts.push(p.to_string()); } }
    parts.join(":")
}

fn expand_tilde(s: &str) -> String { if let Some(rest) = s.strip_prefix("~/") { crate::paths::home_dir().join(rest).to_string_lossy().to_string() } else { s.to_string() } }

/// Resolve an agent command to an executable path, or `None` if it is not installed.
pub fn resolve_command(cmd: &str) -> Option<PathBuf> {
    let cmd = expand_tilde(cmd);
    let p = Path::new(&cmd);
    if p.is_absolute() || cmd.contains('/') { return if p.is_file() { Some(p.to_path_buf()) } else { None }; }
    for d in extra_dirs() { let c = d.join(&cmd); if c.is_file() { return Some(c); } }
    which::which_in(&cmd, Some(augmented_path()), std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))).ok()
}

pub fn is_installed(config: &AcpAgentConfig) -> bool { resolve_command(&config.command).is_some() }

/// Spawn the agent with stdio pipes; stderr lines are forwarded to `log`.
pub fn spawn(config: &AcpAgentConfig, cwd: &Path, log: Arc<dyn Fn(String) + Send + Sync>) -> Result<Spawned> {
    let exe = resolve_command(&config.command).ok_or_else(|| CaretError::not_found(format!("agent command `{}`{}", config.command, config.install_hint.as_deref().map(|h| format!(" — install with: {h}")).unwrap_or_default())))?;
    let mut cmd = Command::new(&exe);
    cmd.args(config.args.iter().map(|a| expand_tilde(a)));
    cmd.current_dir(if cwd.is_dir() { cwd.to_path_buf() } else { crate::paths::home_dir() });
    cmd.env("PATH", augmented_path());
    cmd.env("ACP_CLIENT", "caret");
    cmd.env("CARET_ACP", "1");
    if let Some(env) = &config.env { for (k, v) in env { cmd.env(k, expand_tilde(v)); } }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(unix)] { cmd.process_group(0); }
    let mut child = cmd.spawn().map_err(|e| CaretError::io(format!("failed to start `{}`: {e}", exe.display())))?;
    let stdin = child.stdin.take().ok_or_else(|| CaretError::internal("agent stdin unavailable"))?;
    let stdout = child.stdout.take().ok_or_else(|| CaretError::internal("agent stdout unavailable"))?;
    if let Some(stderr) = child.stderr.take() {
        let log = log.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(l)) = lines.next_line().await { if !l.trim().is_empty() { log(format!("[stderr] {}", l.chars().take(2000).collect::<String>())); } }
        });
    }
    log(format!("[acp] started {} {} (pid {})", exe.display(), config.args.join(" "), child.id().unwrap_or(0)));
    Ok(Spawned { child, stdin, stdout })
}

/// Best-effort process-group kill for terminals and agents (SIGTERM, then SIGKILL).
pub async fn kill_pid(pid: u32) {
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("kill").args(["-TERM", &format!("-{pid}")]).status();
        let _ = std::process::Command::new("kill").args(["-TERM", &pid.to_string()]).status();
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        let _ = std::process::Command::new("kill").args(["-KILL", &format!("-{pid}")]).status();
        let _ = std::process::Command::new("kill").args(["-KILL", &pid.to_string()]).status();
    }
    #[cfg(not(unix))]
    { let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]).status(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_absolute_and_path_commands() {
        assert!(resolve_command("/bin/sh").is_some());
        assert!(resolve_command("sh").is_some());
        assert!(resolve_command("definitely-not-a-real-binary-xyz").is_none());
        assert!(resolve_command("/nonexistent/dir/thing").is_none());
    }
    #[test]
    fn augmented_path_contains_bin_dir() { assert!(augmented_path().split(':').count() >= 2); }
}

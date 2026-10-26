//! Command safety & sandboxing.
//!
//! * `classify` runs BELOW the model: a deterministic shell parser (`shell_ast`) feeds a rule-based classifier
//!   (`classify`) that yields stable risk labels; the policy and the approval UI consume them.
//! * `wrap` applies the OS sandbox (macOS Seatbelt via `sandbox-exec`, Linux bubblewrap) to a prepared command:
//!   writes restricted to the workspace/temp/caches, credential directories unreadable, optional network cut-off.
//! * `mcp_pins` pins MCP tool descriptions (trust-on-first-use) so silently changed tools need a fresh approval.
pub mod bwrap;
pub mod classify;
pub mod mcp_pins;
pub mod seatbelt;
pub mod shell_ast;

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::{AppHandle, State};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandSegment { pub text: String, pub risk: String, pub labels: Vec<String>, #[serde(default, skip_serializing_if = "Option::is_none")] pub program: Option<String> }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandClassification {
    pub command: String,
    pub segments: Vec<CommandSegment>,
    pub overall_risk: String, // safe | caution | destructive | blocked
    pub labels: Vec<String>,
    pub blocked: bool,
    pub reasons: Vec<String>,
    pub allowlisted: bool,
    pub denylisted: bool,
}

impl CommandClassification {
    pub fn risk_tier(&self) -> u8 { classify::risk_tier(self) }
    pub fn is_destructive(&self) -> bool { self.overall_risk == "destructive" || self.overall_risk == "blocked" }
    /// Short human summary, e.g. `destructive · delete-recursive, outside-workspace`.
    pub fn summary(&self) -> String { if self.labels.is_empty() { self.overall_risk.clone() } else { format!("{} · {}", self.overall_risk, self.labels.join(", ")) } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxStatus { pub available: bool, pub backend: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub reason: Option<String>, pub network_isolation: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolChange { pub server_id: String, pub tool: String, pub previous_hash: String, pub current_hash: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub previous_description: Option<String>, pub current_description: String, pub detected_at: u64 }

/// Policy inputs for wrapping a command.
#[derive(Debug, Clone)]
pub struct SandboxPolicy { pub workspace_root: PathBuf, pub writable: Vec<PathBuf>, pub allow_network: bool, pub mode: String }

/// Extra wrap options that are not part of the basic policy (kept separate so `SandboxPolicy` literals stay small).
#[derive(Debug, Clone, Copy)]
pub struct WrapOptions { pub allow_read_home: bool }
impl Default for WrapOptions { fn default() -> Self { Self { allow_read_home: true } } }

#[derive(Default)]
pub struct SandboxState;

// ───────────────────────── classification ─────────────────────────

/// Deterministic classification of a shell command (no model involved), without path context.
pub fn classify(command: &str, allowlist: &[String], denylist: &[String]) -> CommandClassification {
    classify::classify_in(command, &classify::ClassifyCtx::new(None, None), allowlist, denylist)
}

/// Classification with cwd / workspace context (enables outside-workspace detection).
pub fn classify_with(command: &str, cwd: Option<&Path>, workspace: Option<&Path>, allowlist: &[String], denylist: &[String]) -> CommandClassification {
    classify::classify_in(command, &classify::ClassifyCtx::new(cwd.map(Path::to_path_buf), workspace.map(Path::to_path_buf)), allowlist, denylist)
}

// ───────────────────────── wrapping ─────────────────────────

fn backend_name() -> &'static str { if seatbelt::available() { "seatbelt" } else if bwrap::available() { "bubblewrap" } else { "none" } }

/// Wrap a prepared command with the OS sandbox according to `policy` (default options). Returns the command unchanged
/// when no backend is available or `policy.mode == "off"`.
/// Wrap a prepared command in the OS sandbox. The wrapped command is rebuilt (program, args, env, cwd are kept), so
/// configure `stdin`/`stdout`/`stderr` and `kill_on_drop` **after** calling this — std exposes no stdio getters.
pub fn wrap(cmd: tokio::process::Command, policy: &SandboxPolicy) -> tokio::process::Command { wrap_with(cmd, policy, WrapOptions::default()) }

pub fn wrap_with(cmd: tokio::process::Command, policy: &SandboxPolicy, opts: WrapOptions) -> tokio::process::Command {
    if policy.mode == "off" { return cmd; }
    match backend_name() {
        "seatbelt" => seatbelt::wrap(cmd, policy, opts.allow_read_home),
        "bubblewrap" => bwrap::wrap(cmd, policy, opts.allow_read_home),
        _ => cmd,
    }
}

static PROBE: OnceLock<SandboxStatus> = OnceLock::new();

/// Probe the sandbox backend once per process (generates a real profile and runs `true` under it).
pub fn status(root: Option<&Path>) -> SandboxStatus {
    PROBE.get_or_init(|| {
        let backend = backend_name();
        if backend == "none" {
            let reason = if cfg!(target_os = "macos") { "sandbox-exec not found" } else if cfg!(target_os = "linux") { "bubblewrap (bwrap) is not installed" } else { "no sandbox backend on this platform" };
            return SandboxStatus { available: false, backend: backend.into(), reason: Some(reason.into()), network_isolation: false };
        }
        let ws = root.map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
        let policy = SandboxPolicy { workspace_root: ws, writable: vec![], allow_network: true, mode: "auto".into() };
        let probe = std::process::Command::new(if cfg!(target_os = "macos") { "/usr/bin/true" } else { "/bin/true" });
        let probe = tokio::process::Command::from(probe);
        let mut wrapped = wrap(probe, &policy).into_std();
        wrapped.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
        match wrapped.output() {
            Ok(o) if o.status.success() => SandboxStatus { available: true, backend: backend.into(), reason: None, network_isolation: true },
            Ok(o) => SandboxStatus { available: false, backend: backend.into(), reason: Some(format!("probe failed: {}", String::from_utf8_lossy(&o.stderr).trim().chars().take(300).collect::<String>())), network_isolation: false },
            Err(e) => SandboxStatus { available: false, backend: backend.into(), reason: Some(format!("probe failed to start: {e}")), network_isolation: false },
        }
    }).clone()
}

/// Build the sandbox policy for a tool run from settings. `Ok(None)` = run unsandboxed (mode off, or auto without a
/// backend); `Err` = strict mode without a working backend.
pub fn policy_for(settings: &crate::core::settings::Settings, root: Option<&Path>, cwd: &Path) -> Result<Option<SandboxPolicy>> {
    let sb = &settings.sandbox;
    if sb.mode == "off" { return Ok(None); }
    let st = status(root);
    if !st.available {
        if sb.mode == "strict" { return Err(crate::error::CaretError::denied(format!("Sandbox mode is strict but no sandbox is available ({}). Switch Settings → Agent → Sandbox to auto/off or install a backend.", st.reason.unwrap_or_default()))); }
        return Ok(None);
    }
    let workspace_root = root.map(Path::to_path_buf).unwrap_or_else(|| cwd.to_path_buf());
    let mut writable: Vec<PathBuf> = vec![cwd.to_path_buf()];
    for p in &sb.extra_writable_paths { let p = p.trim(); if p.is_empty() { continue; } writable.push(crate::paths::resolve(Some(&workspace_root), &expand_home(p))); }
    Ok(Some(SandboxPolicy { workspace_root, writable, allow_network: sb.allow_network, mode: sb.mode.clone() }))
}

fn expand_home(p: &str) -> String { if let Some(rest) = p.strip_prefix("~/") { crate::paths::home_dir().join(rest).to_string_lossy().to_string() } else if p == "~" { crate::paths::home_dir().to_string_lossy().to_string() } else { p.to_string() } }

/// Would a shell command run sandboxed under these settings? (For approval cards; does not error in strict mode.)
pub fn will_sandbox(settings: &crate::core::settings::Settings, root: Option<&Path>) -> bool { settings.sandbox.mode != "off" && status(root).available }

pub fn wrap_options(settings: &crate::core::settings::Settings) -> WrapOptions { WrapOptions { allow_read_home: settings.sandbox.allow_read_home } }

// ───────────────────────── commands ─────────────────────────

#[tauri::command]
pub fn sandbox_status(app: AppHandle, _st: State<'_, SandboxState>) -> Result<SandboxStatus> {
    use tauri::Manager;
    let root = app.state::<crate::core::workspace::WorkspaceState>().root();
    Ok(status(root.as_deref()))
}

#[tauri::command]
pub fn sandbox_classify(app: AppHandle, command: String, cwd: Option<String>) -> Result<CommandClassification> {
    use tauri::Manager;
    let s = app.state::<crate::core::settings::SettingsState>().effective();
    let root = app.state::<crate::core::workspace::WorkspaceState>().root();
    let cwd = cwd.map(|c| crate::paths::resolve(root.as_deref(), &c)).or_else(|| root.clone());
    Ok(classify_with(&command, cwd.as_deref(), root.as_deref(), &s.agent.command_allowlist, &s.agent.command_denylist))
}

#[tauri::command]
pub fn mcp_tool_changes(_st: State<'_, SandboxState>) -> Result<Vec<McpToolChange>> { Ok(mcp_pins::pending_all()) }

#[tauri::command]
pub fn mcp_acknowledge_tool(_st: State<'_, SandboxState>, server_id: String, tool: String) -> Result<()> { mcp_pins::acknowledge(&server_id, &tool); Ok(()) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classify_without_context_still_flags() {
        let c = classify("rm -rf /", &[], &[]);
        assert!(c.blocked);
        let c = classify("ls", &["ls".into()], &[]);
        assert!(c.allowlisted && c.overall_risk == "safe");
    }
    #[test]
    fn wrap_off_is_identity() {
        let p = SandboxPolicy { workspace_root: std::env::temp_dir(), writable: vec![], allow_network: true, mode: "off".into() };
        let c = tokio::process::Command::new("/bin/echo");
        let w = wrap(c, &p);
        assert_eq!(w.as_std().get_program(), "/bin/echo");
    }
    #[test]
    fn status_probe_reports_backend() {
        let s = status(None);
        if cfg!(target_os = "macos") { assert_eq!(s.backend, "seatbelt"); assert!(s.available, "{:?}", s.reason); }
    }
}

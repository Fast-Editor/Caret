//! Shell tools: run_terminal_cmd (foreground/background) and read_terminal.
use super::{arg_bool, arg_str, arg_u64, cap_output, s_bool, s_int, s_str, schema, Tool, ToolContext, ToolOutput};
use crate::ai::agent::policy::ToolEffect;
use crate::core::ports;
use crate::error::{CaretError, Result};
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncReadExt;

pub fn tools() -> Vec<Arc<dyn Tool>> { vec![Arc::new(RunTerminalCmd), Arc::new(ReadTerminal)] }

pub fn user_shell(settings: &crate::core::settings::Settings) -> String {
    settings.general.default_shell.clone().filter(|s| !s.is_empty()).or_else(|| std::env::var("SHELL").ok()).unwrap_or_else(|| "/bin/zsh".into())
}

pub struct CmdResult { pub output: String, pub code: Option<i32>, pub timed_out: bool, pub cancelled: bool }

/// Run `command` through the user's shell with a timeout; combined stdout+stderr (capped at 1 MiB). Unsandboxed.
pub async fn run_shell(shell: &str, command: &str, cwd: &std::path::Path, timeout_secs: u64, cancel: &tokio_util::sync::CancellationToken) -> Result<CmdResult> {
    run_shell_with(shell, command, cwd, timeout_secs, cancel, None, crate::sandbox::WrapOptions::default()).await
}

/// Like `run_shell`, optionally wrapped in the OS sandbox (`sandbox` = Some(policy)).
pub async fn run_shell_with(shell: &str, command: &str, cwd: &std::path::Path, timeout_secs: u64, cancel: &tokio_util::sync::CancellationToken, sandbox: Option<&crate::sandbox::SandboxPolicy>, opts: crate::sandbox::WrapOptions) -> Result<CmdResult> {
    let mut cmd = tokio::process::Command::new(shell);
    cmd.arg("-lc").arg(command).current_dir(cwd)
        .env("CARET", "1").env("TERM", "dumb").env("NO_COLOR", "1").env("CI", "1").env("GIT_PAGER", "cat").env("PAGER", "cat");
    if let Some(p) = sandbox { cmd = crate::sandbox::wrap_with(cmd, p, opts); }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(unix)] { cmd.process_group(0); }
    let mut child = cmd.spawn().map_err(|e| CaretError::io(format!("failed to start shell {shell}: {e}")))?;
    let mut out = child.stdout.take().unwrap(); let mut err = child.stderr.take().unwrap();
    let buf = Arc::new(parking_lot::Mutex::new(Vec::<u8>::new()));
    let (b1, b2) = (buf.clone(), buf.clone());
    let cap = 1024 * 1024;
    let t1 = tokio::spawn(async move { let mut tmp = [0u8; 8192]; loop { match out.read(&mut tmp).await { Ok(0) | Err(_) => break, Ok(n) => { let mut b = b1.lock(); if b.len() < cap { b.extend_from_slice(&tmp[..n]); } } } } });
    let t2 = tokio::spawn(async move { let mut tmp = [0u8; 8192]; loop { match err.read(&mut tmp).await { Ok(0) | Err(_) => break, Ok(n) => { let mut b = b2.lock(); if b.len() < cap { b.extend_from_slice(&tmp[..n]); } } } } });
    let (code, timed_out, cancelled) = tokio::select! {
        r = child.wait() => (r.ok().and_then(|s| s.code()), false, false),
        _ = tokio::time::sleep(std::time::Duration::from_secs(timeout_secs.max(1))) => { kill_group(&mut child).await; (None, true, false) }
        _ = cancel.cancelled() => { kill_group(&mut child).await; (None, false, true) }
    };
    let _ = tokio::time::timeout(std::time::Duration::from_millis(500), async { let _ = t1.await; let _ = t2.await; }).await;
    let bytes = std::mem::take(&mut *buf.lock());
    Ok(CmdResult { output: String::from_utf8_lossy(&bytes).to_string(), code, timed_out, cancelled })
}

async fn kill_group(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() { unsafe { libc_kill(-(pid as i32), 15); } tokio::time::sleep(std::time::Duration::from_millis(300)).await; unsafe { libc_kill(-(pid as i32), 9); } }
    let _ = child.kill().await;
}
#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) { extern "C" { fn kill(pid: i32, sig: i32) -> i32; } let _ = kill(pid, sig); }

/// Heuristic: output mentions a permission failure typical of a sandbox denial.
fn looks_like_sandbox_denial(out: &str) -> bool { let o = out.to_lowercase(); o.contains("operation not permitted") || o.contains("permission denied") || o.contains("read-only file system") || o.contains("eperm") || o.contains("eacces") }

pub struct RunTerminalCmd;
#[async_trait::async_trait]
impl Tool for RunTerminalCmd {
    fn name(&self) -> &str { "run_terminal_cmd" }
    fn display_name(&self) -> &str { "Run command" }
    fn description(&self) -> String { "Run a shell command in the workspace (user's login shell, non-interactive). Use for builds, tests, linters, git, package managers and scripts. Commands may require user approval depending on their settings. Set is_background=true for long-running processes such as dev servers or watchers (output goes to a log file you can tail). Never run interactive commands (use flags like --yes); pipe through `cat` for pagers. Output is capped; prefer targeted commands (e.g. `npm test -- path`).".into() }
    fn input_schema(&self) -> Value { schema(json!({ "command": s_str("The shell command to run"), "explanation": s_str("One sentence on why this command is needed (shown to the user)"), "is_background": s_bool("Run detached in the background (for servers/watchers)"), "timeout_secs": s_int("Timeout for foreground commands (default from settings, max 900)"), "cwd": s_str("Working directory relative to the workspace (default: workspace root)") }), &["command"]) }
    fn readonly(&self) -> bool { false }
    fn group(&self) -> &str { "terminal" }
    fn effect(&self, i: &Value) -> ToolEffect { ToolEffect::Shell(arg_str(i, &["command"]).unwrap_or("").to_string()) }
    fn label(&self, i: &Value) -> Option<String> { arg_str(i, &["command"]).map(|c| c.chars().take(120).collect()) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let command = arg_str(&input, &["command", "cmd"]).ok_or_else(|| CaretError::invalid("command is required"))?.to_string();
        if command.trim().is_empty() { return Ok(ToolOutput::error("command is empty")); }
        let cwd = arg_str(&input, &["cwd"]).map(|c| ctx.resolve(c)).unwrap_or_else(|| ctx.cwd.clone());
        if !cwd.is_dir() { return Ok(ToolOutput::error(format!("cwd {} does not exist", cwd.display()))); }
        let shell = user_shell(&ctx.settings);
        // OS sandbox (Seatbelt / bwrap): None = run unsandboxed (mode off, or auto without a backend); Err = strict mode unmet.
        let sandbox = crate::sandbox::policy_for(&ctx.settings, ctx.root.as_deref(), &cwd)?;
        let wrap_opts = crate::sandbox::wrap_options(&ctx.settings);
        let sandboxed = sandbox.is_some();
        let background = arg_bool(&input, &["is_background", "background"]).unwrap_or(false);
        if background {
            let log_dir = crate::paths::sub_dir("logs").join("bg"); let _ = std::fs::create_dir_all(&log_dir);
            let log = log_dir.join(format!("{}-{}.log", chrono::Local::now().format("%Y%m%d-%H%M%S"), &ctx.tool_call_id.chars().take(8).collect::<String>()));
            let f = std::fs::File::create(&log)?; let f2 = f.try_clone()?;
            let mut cmd = tokio::process::Command::new(&shell);
            cmd.arg("-lc").arg(&command).current_dir(&cwd).env("CARET", "1");
            if let Some(p) = &sandbox { cmd = crate::sandbox::wrap_with(cmd, p, wrap_opts); }
            cmd.stdin(Stdio::null()).stdout(Stdio::from(f)).stderr(Stdio::from(f2));
            #[cfg(unix)] { cmd.process_group(0); }
            let child = cmd.spawn().map_err(|e| CaretError::io(format!("failed to start: {e}")))?;
            let pid = child.id().unwrap_or(0);
            tokio::spawn(async move { let mut c = child; let _ = c.wait().await; });
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
            let head = std::fs::read_to_string(&log).unwrap_or_default();
            let (head, _) = cap_output(&head, 2000);
            return Ok(ToolOutput { text: format!("Started in background (pid {pid}){}.\nLog file: {}\nInitial output:\n{}\n\nUse `tail -n 80 {}` to check progress; `kill {pid}` to stop.", if sandboxed { " [sandboxed]" } else { "" }, log.display(), if head.trim().is_empty() { "(none yet)" } else { head.trim() }, log.display()), command: Some(command.clone()), is_background: true, sandboxed, ..Default::default() }.with_summary(format!("Background: {}", command.chars().take(60).collect::<String>())));
        }
        let timeout = arg_u64(&input, &["timeout_secs", "timeout"]).unwrap_or(ctx.settings.agent.command_timeout_secs.max(5)).min(900);
        let res = run_shell_with(&shell, &command, &cwd, timeout, &ctx.cancel, sandbox.as_ref(), wrap_opts).await?;
        let (text, truncated) = cap_output(&res.output, ctx.max_output());
        let mut status = if res.cancelled { "Cancelled by user".to_string() } else if res.timed_out { format!("Timed out after {timeout}s (process killed). If this is a long-running server, rerun with is_background=true.") } else { format!("Exit code: {}", res.code.map(|c| c.to_string()).unwrap_or_else(|| "signal".into())) };
        if sandboxed && res.code.map(|c| c != 0).unwrap_or(false) && looks_like_sandbox_denial(&res.output) { status.push_str("\nNote: the command ran inside Caret's sandbox (writes limited to the workspace/temp, credentials unreadable). If this denial is expected, ask the user to add the path under Settings → Agent → Sandbox → extra writable paths."); }
        else if sandboxed { status.push_str(" [sandboxed]"); }
        let body = if text.trim().is_empty() { "(no output)".to_string() } else { text };
        let is_error = res.timed_out || res.cancelled || res.code.map(|c| c != 0).unwrap_or(true);
        Ok(ToolOutput { text: format!("$ {command}\n{body}\n\n{status}{}", if truncated { " (output truncated)" } else { "" }), command: Some(command.clone()), exit_code: res.code, is_error, sandboxed, ..Default::default() }.with_summary(format!("{} → {}", command.chars().take(50).collect::<String>(), if res.timed_out { "timeout".into() } else { res.code.map(|c| format!("exit {c}")).unwrap_or_else(|| "killed".into()) })))
    }
}

pub struct ReadTerminal;
#[async_trait::async_trait]
impl Tool for ReadTerminal {
    fn name(&self) -> &str { "read_terminal" }
    fn display_name(&self) -> &str { "Read terminal" }
    fn description(&self) -> String { "Read recent output from one of the user's open integrated terminals (e.g. to see a failing build the user is looking at). Call without terminal_id to list terminals.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "terminal_id": s_str("Terminal id (omit to list terminals)"), "lines": s_int("Number of trailing lines (default 200)") }), &[]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "terminal" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::ReadOnly }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let Some(port) = ports::terminal() else { return Ok(ToolOutput::error("No integrated terminals are available in this session")) };
        let list = port.list();
        match arg_str(&input, &["terminal_id", "id"]) {
            None => { if list.is_empty() { return Ok(ToolOutput::text("No open terminals.")); } Ok(ToolOutput::text(list.iter().map(|(id, t)| format!("{id}: {t}")).collect::<Vec<_>>().join("\n"))) }
            Some(id) => {
                let lines = arg_u64(&input, &["lines"]).unwrap_or(200).clamp(10, 2000) as usize;
                match port.read_buffer(id, lines) { Some(b) => { let (t, _) = cap_output(&b, ctx.max_output()); Ok(ToolOutput::text(if t.trim().is_empty() { "(terminal buffer is empty)".into() } else { t })) } None => Ok(ToolOutput::error(format!("Terminal {id} not found"))) }
            }
        }
    }
}

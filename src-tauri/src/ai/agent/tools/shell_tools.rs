//! Shell tools: run_terminal_cmd (foreground/background) and read_terminal.
//!
//! Commands go through `ctx.executor` (docs/WARREN.md): the local shell (optionally under the OS sandbox) or the
//! thread's Warren instance, in which case the files the command changed come back as pending changes.
use super::{arg_bool, arg_str, arg_u64, cap_output, s_bool, s_int, s_str, schema, Tool, ToolContext, ToolOutput};
use crate::ai::agent::policy::ToolEffect;
use crate::core::ports;
use crate::error::{CaretError, Result};
use crate::warren::binding as sandbox_binding;
use crate::warren::executor::ExecSpec;
use serde_json::{json, Value};
use std::sync::Arc;

pub use crate::warren::executor::{run_shell, run_shell_with, CmdResult};

pub fn tools() -> Vec<Arc<dyn Tool>> { vec![Arc::new(RunTerminalCmd), Arc::new(ReadTerminal)] }

pub fn user_shell(settings: &crate::core::settings::Settings) -> String {
    settings.general.default_shell.clone().filter(|s| !s.is_empty()).or_else(|| std::env::var("SHELL").ok()).unwrap_or_else(|| "/bin/zsh".into())
}

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
        let executor = ctx.executor.clone();
        let remote = executor.kind() != "local";
        let sandboxed = executor.sandboxed();
        let timeout = arg_u64(&input, &["timeout_secs", "timeout"]).unwrap_or(ctx.settings.agent.command_timeout_secs.max(5)).min(900);
        let spec = ExecSpec { shell: user_shell(&ctx.settings), command: command.clone(), cwd: cwd.clone(), timeout_secs: timeout, env: Default::default() };
        let background = arg_bool(&input, &["is_background", "background"]).unwrap_or(false);
        if background {
            if remote { return Ok(ToolOutput::error(format!("Background commands are not available in the {} sandbox: a detached process cannot be tracked or stopped through Warren's exec API. Run it in the foreground with a timeout, or ask the user to start it from the instance terminal.", executor.describe()))); }
            let log_dir = crate::paths::sub_dir("logs").join("bg"); let _ = std::fs::create_dir_all(&log_dir);
            let log = log_dir.join(format!("{}-{}.log", chrono::Local::now().format("%Y%m%d-%H%M%S"), &ctx.tool_call_id.chars().take(8).collect::<String>()));
            let pid = executor.spawn_background(&spec, &log).await?;
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
            let head = std::fs::read_to_string(&log).unwrap_or_default();
            let (head, _) = cap_output(&head, 2000);
            return Ok(ToolOutput { text: format!("Started in background (pid {pid}){}.\nLog file: {}\nInitial output:\n{}\n\nUse `tail -n 80 {}` to check progress; `kill {pid}` to stop.", if sandboxed { " [sandboxed]" } else { "" }, log.display(), if head.trim().is_empty() { "(none yet)" } else { head.trim() }, log.display()), command: Some(command.clone()), is_background: true, sandboxed, ..Default::default() }.with_summary(format!("Background: {}", command.chars().take(60).collect::<String>())));
        }
        if remote { sandbox_binding::set_binding_status(&ctx.manager, &ctx.session_id, "running", None); }
        let res = match executor.run(&spec, &ctx.cancel).await {
            Ok(r) => r,
            Err(e) => { if remote { sandbox_binding::set_binding_status(&ctx.manager, &ctx.session_id, "error", Some(e.message.clone())); } return Err(e); }
        };
        let (text, truncated) = cap_output(&res.output, ctx.max_output());
        let mut status = if res.cancelled { "Cancelled by user".to_string() } else if res.timed_out { format!("Timed out after {timeout}s (process killed). If this is a long-running server, rerun with is_background=true.") } else { format!("Exit code: {}", res.code.map(|c| c.to_string()).unwrap_or_else(|| "signal".into())) };
        if !remote && sandboxed && res.code.map(|c| c != 0).unwrap_or(false) && looks_like_sandbox_denial(&res.output) { status.push_str("\nNote: the command ran inside Caret's sandbox (writes limited to the workspace/temp, credentials unreadable). If this denial is expected, ask the user to add the path under Settings → Agent → Sandbox → extra writable paths."); }
        else if !remote && sandboxed { status.push_str(" [sandboxed]"); }
        // Remote run: files the command created/changed come back as pending changes; the card says where it ran.
        let mut file_changes = vec![];
        let mut tail = String::new();
        if remote {
            let sync = executor.take_remote_sync();
            let pushed = sync.pushed_files;
            let (applied, err) = match sandbox_binding::apply_pulled(ctx, sync).await { Ok(a) => (Some(a), None), Err(e) => (None, Some(e.message)) };
            if let Some(a) = &applied { file_changes = a.changes.clone(); if let Some(d) = sandbox_binding::describe_pull(a) { tail.push_str("\n"); tail.push_str(&d); } }
            if let Some(e) = &err { tail.push_str(&format!("\n[warren] applying the files the command changed failed: {e}")); }
            sandbox_binding::record_run(&ctx.manager, &ctx.session_id, pushed, err);
            tail.push_str(&format!("\n[{}]", executor.describe()));
        }
        let body = if text.trim().is_empty() { "(no output)".to_string() } else { text };
        let is_error = res.timed_out || res.cancelled || res.code.map(|c| c != 0).unwrap_or(true);
        Ok(ToolOutput { text: format!("$ {command}\n{body}\n\n{status}{}{tail}", if truncated { " (output truncated)" } else { "" }), command: Some(command.clone()), exit_code: res.code, is_error, sandboxed, executor: if remote { Some(executor.describe()) } else { None }, file_changes, ..Default::default() }.with_summary(format!("{} → {}", command.chars().take(50).collect::<String>(), if res.timed_out { "timeout".into() } else { res.code.map(|c| format!("exit {c}")).unwrap_or_else(|| "killed".into()) })))
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

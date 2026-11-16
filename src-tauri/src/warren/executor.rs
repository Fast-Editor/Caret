//! Command executors (docs/WARREN.md): where `run_terminal_cmd` runs.
//!
//! `LocalExecutor` is the shell spawn Caret always had (optionally under the OS sandbox); `WarrenExecutor` pushes the
//! workspace to the thread's Warren instance, runs the command in the synced copy and pulls back the files it
//! changed, which the tool layer turns into pending changes. The runner picks one executor per turn from the
//! session's `sandbox` binding and shares it through `ToolContext.executor`.
use super::adapter::{self, SandboxBinding};
use crate::error::{CaretError, Result};
use crate::sandbox::{SandboxPolicy, WrapOptions};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

/// One command to run: `shell -lc command` in `cwd` (a local path; remote executors map it into the synced copy).
/// `env` is added on top of the executor's non-interactive defaults (`CARET=1 TERM=dumb NO_COLOR=1 CI=1 …`).
#[derive(Debug, Clone)]
pub struct ExecSpec { pub shell: String, pub command: String, pub cwd: PathBuf, pub timeout_secs: u64, pub env: BTreeMap<String, String> }

/// Outcome of a foreground command: combined stdout+stderr (capped at 1 MiB), exit code (`None` = killed / cancelled).
#[derive(Debug, Clone, Default)]
pub struct CmdResult { pub output: String, pub code: Option<i32>, pub timed_out: bool, pub cancelled: bool }

/// Environment every executor sets so tools behave non-interactively.
pub const DEFAULT_ENV: &[(&str, &str)] = &[("CARET", "1"), ("TERM", "dumb"), ("NO_COLOR", "1"), ("CI", "1"), ("GIT_PAGER", "cat"), ("PAGER", "cat")];

/// What a remote run moved: files pushed before the command, files the command created/changed (`changed`) and
/// pushed files it deleted (`deleted`), all relative to `local_root`. The tool layer turns them into pending changes.
#[derive(Debug, Clone, Default)]
pub struct RemoteSync { pub local_root: PathBuf, pub pushed_files: u64, pub changed: Vec<(String, Vec<u8>)>, pub deleted: Vec<String> }

#[async_trait]
pub trait CommandExecutor: Send + Sync {
    async fn run(&self, spec: &ExecSpec, cancel: &CancellationToken) -> Result<CmdResult>;
    /// `"local"` or `"warren"`.
    fn kind(&self) -> &'static str;
    /// Human label: `local`, `local (sandboxed)`, `warren <instanceId>`.
    fn describe(&self) -> String;
    /// Commands cannot touch the user's machine (OS sandbox active, or a remote instance).
    fn sandboxed(&self) -> bool;
    /// Start `spec` detached with stdout/stderr appended to `log`; returns the pid. Remote executors refuse.
    async fn spawn_background(&self, _spec: &ExecSpec, _log: &Path) -> Result<u32> {
        Err(CaretError::invalid(format!("background commands are not supported by the {} executor", self.describe())))
    }
    /// Take what the last remote run moved (empty for local executors).
    fn take_remote_sync(&self) -> RemoteSync { RemoteSync::default() }
}

// ───────────────────────── local ─────────────────────────

/// The user's login shell on this machine, optionally wrapped in the OS sandbox (Seatbelt / bubblewrap).
pub struct LocalExecutor {
    pub sandbox: Option<SandboxPolicy>,
    pub wrap: WrapOptions,
    /// Strict sandbox mode without a working backend: every run fails with this message (the per-command error
    /// `run_terminal_cmd` used to raise itself).
    refuse: Option<String>,
}

impl Default for LocalExecutor { fn default() -> Self { Self { sandbox: None, wrap: WrapOptions::default(), refuse: None } } }

impl LocalExecutor {
    pub fn new(sandbox: Option<SandboxPolicy>, wrap: WrapOptions) -> Self { Self { sandbox, wrap, refuse: None } }
    /// `run_terminal_cmd`'s policy: `sandbox::policy_for(settings, root, cwd)` + `wrap_options(settings)`. A strict-mode
    /// failure is kept and reported by every `run` instead of failing the whole turn.
    pub fn from_settings(settings: &crate::core::settings::Settings, root: Option<&Path>, cwd: &Path) -> Self {
        let wrap = crate::sandbox::wrap_options(settings);
        match crate::sandbox::policy_for(settings, root, cwd) {
            Ok(sandbox) => Self { sandbox, wrap, refuse: None },
            Err(e) => Self { sandbox: None, wrap, refuse: Some(e.message) },
        }
    }
    /// The turn policy with the command's `cwd` writable (the per-command policy always made its cwd writable).
    fn policy_for(&self, cwd: &Path) -> Option<SandboxPolicy> {
        self.sandbox.clone().map(|mut p| { if !p.writable.iter().any(|w| w == cwd) { p.writable.push(cwd.to_path_buf()); } p })
    }
}

#[async_trait]
impl CommandExecutor for LocalExecutor {
    async fn run(&self, spec: &ExecSpec, cancel: &CancellationToken) -> Result<CmdResult> {
        if let Some(m) = &self.refuse { return Err(CaretError::denied(m.clone())); }
        let policy = self.policy_for(&spec.cwd);
        spawn_and_wait(&spec.shell, &spec.command, &spec.cwd, spec.timeout_secs, cancel, policy.as_ref(), self.wrap, &spec.env).await
    }
    fn kind(&self) -> &'static str { "local" }
    fn describe(&self) -> String { if self.sandbox.is_some() { "local (sandboxed)".into() } else { "local".into() } }
    fn sandboxed(&self) -> bool { self.sandbox.is_some() }
    async fn spawn_background(&self, spec: &ExecSpec, log: &Path) -> Result<u32> {
        if let Some(m) = &self.refuse { return Err(CaretError::denied(m.clone())); }
        let f = std::fs::File::create(log)?; let f2 = f.try_clone()?;
        let mut cmd = tokio::process::Command::new(&spec.shell);
        cmd.arg("-lc").arg(&spec.command).current_dir(&spec.cwd).env("CARET", "1");
        for (k, v) in &spec.env { cmd.env(k, v); }
        if let Some(p) = self.policy_for(&spec.cwd) { cmd = crate::sandbox::wrap_with(cmd, &p, self.wrap); }
        cmd.stdin(Stdio::null()).stdout(Stdio::from(f)).stderr(Stdio::from(f2));
        #[cfg(unix)] { cmd.process_group(0); }
        let child = cmd.spawn().map_err(|e| CaretError::io(format!("failed to start: {e}")))?;
        let pid = child.id().unwrap_or(0);
        tokio::spawn(async move { let mut c = child; let _ = c.wait().await; });
        Ok(pid)
    }
}

/// Run `command` through the user's shell with a timeout; combined stdout+stderr (capped at 1 MiB). Unsandboxed.
pub async fn run_shell(shell: &str, command: &str, cwd: &Path, timeout_secs: u64, cancel: &CancellationToken) -> Result<CmdResult> {
    run_shell_with(shell, command, cwd, timeout_secs, cancel, None, WrapOptions::default()).await
}

/// Like `run_shell`, optionally wrapped in the OS sandbox (`sandbox` = Some(policy)).
pub async fn run_shell_with(shell: &str, command: &str, cwd: &Path, timeout_secs: u64, cancel: &CancellationToken, sandbox: Option<&SandboxPolicy>, opts: WrapOptions) -> Result<CmdResult> {
    spawn_and_wait(shell, command, cwd, timeout_secs, cancel, sandbox, opts, &BTreeMap::new()).await
}

async fn spawn_and_wait(shell: &str, command: &str, cwd: &Path, timeout_secs: u64, cancel: &CancellationToken, sandbox: Option<&SandboxPolicy>, opts: WrapOptions, env: &BTreeMap<String, String>) -> Result<CmdResult> {
    let mut cmd = tokio::process::Command::new(shell);
    cmd.arg("-lc").arg(command).current_dir(cwd);
    for (k, v) in DEFAULT_ENV { cmd.env(k, v); }
    for (k, v) in env { cmd.env(k, v); }
    if let Some(p) = sandbox { cmd = crate::sandbox::wrap_with(cmd, p, opts); }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(unix)] { cmd.process_group(0); }
    let mut child = cmd.spawn().map_err(|e| CaretError::io(format!("failed to start shell {shell}: {e}")))?;
    let mut out = child.stdout.take().unwrap(); let mut err = child.stderr.take().unwrap();
    let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
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

// ───────────────────────── warren ─────────────────────────

/// Files uploaded / deleted by one push; all zero = nothing changed, nothing was sent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PushStats { pub files: u64, pub bytes: u64, pub deleted: u64 }
/// One exec round-trip.
#[derive(Debug, Clone, Default)]
pub struct RemoteExec { pub exit_code: i32, pub stdout: String, pub stderr: String, pub truncated: bool }
/// Files the instance changed since the last push and pushed files it deleted (relative paths).
#[derive(Debug, Clone, Default)]
pub struct RemotePull { pub changed: Vec<(String, Vec<u8>)>, pub deleted: Vec<String> }

/// What `WarrenExecutor` needs from the sandbox. Production: `adapter::WarrenRemote` (part A's client + sync);
/// tests: a fake.
#[async_trait]
pub trait RemoteOps: Send + Sync {
    /// Scan `local_root` (gitignore-aware, minus `excludes`) and push what changed since the last push to this
    /// instance. Nothing changed → nothing is sent and the stats are zero.
    async fn push_changes(&self, local_root: &Path, excludes: &[String]) -> Result<PushStats>;
    /// Run `command` in the instance with Warren's request timeout (no cancel on the wire).
    async fn exec(&self, command: &str, timeout_secs: u64) -> Result<RemoteExec>;
    /// Files the instance created/changed since the last push (minus `.git` and `excludes`) and pushed files it deleted.
    async fn pull_changes(&self, local_root: &Path, excludes: &[String]) -> Result<RemotePull>;
}

/// Runs commands inside the thread's Warren instance on a synced copy of `local_root` at `binding.remote_root`.
pub struct WarrenExecutor {
    remote: Arc<dyn RemoteOps>,
    binding: SandboxBinding,
    local_root: PathBuf,
    excludes: Vec<String>,
    /// Push/pull of the last `run`, until the tool layer takes it.
    last: Mutex<RemoteSync>,
}

impl WarrenExecutor {
    /// `client` from `warren::client_for(&settings)`, `excludes` = `settings.warren.sync_excludes`.
    pub fn new(client: impl Into<Arc<adapter::WarrenClient>>, binding: SandboxBinding, local_root: PathBuf, excludes: Vec<String>) -> Self {
        let remote = Arc::new(adapter::WarrenRemote::new(client.into(), binding.clone()));
        Self::with_remote(remote, binding, local_root, excludes)
    }
    pub fn with_remote(remote: Arc<dyn RemoteOps>, binding: SandboxBinding, local_root: PathBuf, excludes: Vec<String>) -> Self {
        Self { remote, binding, local_root, excludes, last: Mutex::new(RemoteSync::default()) }
    }
    pub fn binding(&self) -> &SandboxBinding { &self.binding }
    pub fn local_root(&self) -> &Path { &self.local_root }

    /// Where a local `cwd` lives in the synced copy: `<remote_root>/<rel>` under `local_root`, the remote root otherwise.
    pub fn remote_cwd(cwd: &Path, local_root: &Path, remote_root: &str) -> String {
        let root = remote_root.trim_end_matches('/');
        let root = if root.is_empty() { "/" } else { root };
        match crate::paths::normalize(cwd).strip_prefix(crate::paths::normalize(local_root)) {
            Ok(rel) if rel.as_os_str().is_empty() => root.to_string(),
            Ok(rel) => { let rel: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect(); format!("{}/{}", root.trim_end_matches('/'), rel.join("/")) }
            Err(_) => root.to_string(),
        }
    }
    /// The line Warren's exec API runs: `cd '<remote cwd>' && export <defaults + env> && <command>`.
    pub fn remote_command(spec: &ExecSpec, local_root: &Path, remote_root: &str) -> String {
        let cwd = Self::remote_cwd(&spec.cwd, local_root, remote_root);
        let mut env: Vec<String> = DEFAULT_ENV.iter().filter(|(k, _)| !spec.env.contains_key(*k)).map(|(k, v)| format!("{k}={}", sh_quote(v))).collect();
        env.extend(spec.env.iter().map(|(k, v)| format!("{k}={}", sh_quote(v))));
        format!("cd {} && export {} && {}", sh_quote(&cwd), env.join(" "), spec.command)
    }

    async fn run_inner(&self, spec: &ExecSpec) -> Result<CmdResult> {
        let pushed = self.remote.push_changes(&self.local_root, &self.excludes).await?;
        let command = Self::remote_command(spec, &self.local_root, &self.binding.remote_root);
        let (mut output, code, timed_out) = match self.remote.exec(&command, spec.timeout_secs).await {
            Ok(r) => {
                let mut out = r.stdout;
                if !r.stderr.is_empty() { if !out.is_empty() && !out.ends_with('\n') { out.push('\n'); } out.push_str(&r.stderr); }
                if r.truncated { out.push_str("\n[warren] output truncated by the sandbox API"); }
                (out, Some(r.exit_code), false)
            }
            Err(e) if e.code.contains("timeout") || e.message.to_ascii_lowercase().contains("timed out") => (format!("[warren] {}", e.message), None, true),
            Err(e) => return Err(e),
        };
        let pull = match self.remote.pull_changes(&self.local_root, &self.excludes).await {
            Ok(p) => p,
            Err(e) => { output.push_str(&format!("\n[warren] pulling the files the command changed failed: {}", e.message)); RemotePull::default() }
        };
        *self.last.lock() = RemoteSync { local_root: self.local_root.clone(), pushed_files: pushed.files + pushed.deleted, changed: pull.changed, deleted: pull.deleted };
        Ok(CmdResult { output, code, timed_out, cancelled: false })
    }
}

#[async_trait]
impl CommandExecutor for WarrenExecutor {
    async fn run(&self, spec: &ExecSpec, cancel: &CancellationToken) -> Result<CmdResult> {
        tokio::select! {
            r = self.run_inner(spec) => r,
            _ = cancel.cancelled() => Ok(CmdResult {
                output: format!("Cancelled by user. Warren's exec API has no cancel: the command keeps running in {} until it finishes or its {}s timeout hits; files it changes come back with the next command or Sync now.", self.describe(), spec.timeout_secs),
                code: None, timed_out: false, cancelled: true,
            }),
        }
    }
    fn kind(&self) -> &'static str { "warren" }
    fn describe(&self) -> String { format!("warren {}", self.binding.instance_id) }
    fn sandboxed(&self) -> bool { true }
    fn take_remote_sync(&self) -> RemoteSync { std::mem::take(&mut *self.last.lock()) }
}

/// POSIX single-quote `s` for `sh -c`.
pub fn sh_quote(s: &str) -> String { format!("'{}'", s.replace('\'', "'\\''")) }

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    fn spec(cwd: &Path, command: &str) -> ExecSpec { ExecSpec { shell: "/bin/zsh".into(), command: command.into(), cwd: cwd.to_path_buf(), timeout_secs: 30, env: BTreeMap::new() } }
    fn binding() -> SandboxBinding { adapter::new_binding("inst_42", "proj-ab12", "/home/agent/work/proj".into()) }

    #[test]
    fn remote_cwd_maps_inside_root_and_outside() {
        let root = Path::new("/Users/me/proj");
        assert_eq!(WarrenExecutor::remote_cwd(root, root, "/home/agent/work/proj"), "/home/agent/work/proj");
        assert_eq!(WarrenExecutor::remote_cwd(&root.join("src/app"), root, "/home/agent/work/proj/"), "/home/agent/work/proj/src/app");
        assert_eq!(WarrenExecutor::remote_cwd(&root.join("a/../b"), root, "/home/agent/work/proj"), "/home/agent/work/proj/b", "normalised before mapping");
        assert_eq!(WarrenExecutor::remote_cwd(Path::new("/tmp/elsewhere"), root, "/home/agent/work/proj"), "/home/agent/work/proj", "outside the workspace → remote root");
        assert_eq!(WarrenExecutor::remote_cwd(Path::new("/Users/me/proj-other"), root, "/home/agent/work/proj"), "/home/agent/work/proj", "sibling with the same prefix is outside");
    }

    #[test]
    fn remote_command_quotes_cwd_and_env_and_keeps_the_command_verbatim() {
        let root = Path::new("/Users/me/my proj");
        let mut s = spec(&root.join("it's/here"), "npm test -- --grep 'a b' && echo $HOME");
        s.env.insert("FOO".into(), "x'y".into());
        let cmd = WarrenExecutor::remote_command(&s, root, "/home/agent/work/my proj");
        assert!(cmd.starts_with("cd '/home/agent/work/my proj/it'\\''s/here' && export "), "{cmd}");
        assert!(cmd.contains("CARET='1'") && cmd.contains("CI='1'") && cmd.contains("PAGER='cat'"), "{cmd}");
        assert!(cmd.contains("FOO='x'\\''y'"), "{cmd}");
        assert!(cmd.ends_with(" && npm test -- --grep 'a b' && echo $HOME"), "{cmd}");
        // a spec env value overrides the default of the same name
        let mut s2 = spec(root, "true"); s2.env.insert("CI".into(), "0".into());
        let cmd2 = WarrenExecutor::remote_command(&s2, root, "/r");
        assert!(cmd2.contains("CI='0'") && !cmd2.contains("CI='1'"), "{cmd2}");
        assert_eq!(sh_quote("plain"), "'plain'");
    }

    /// Fake sandbox: remembers pushes/execs, scans the local root itself (size+mtime) so an unchanged tree pushes
    /// nothing, and answers exec from a script.
    struct FakeRemote {
        pushes: Mutex<Vec<u64>>, execs: Mutex<Vec<(String, u64)>>, pulls: AtomicU64,
        manifest: Mutex<Option<BTreeMap<String, (u64, u64)>>>,
        exec_result: Mutex<Option<Result<RemoteExec>>>, exec_delay: Option<Duration>,
        pull: Mutex<RemotePull>,
    }
    impl FakeRemote {
        fn new() -> Self { Self { pushes: Mutex::new(vec![]), execs: Mutex::new(vec![]), pulls: AtomicU64::new(0), manifest: Mutex::new(None), exec_result: Mutex::new(Some(Ok(RemoteExec { exit_code: 0, stdout: "out\n".into(), stderr: String::new(), truncated: false }))), exec_delay: None, pull: Mutex::new(RemotePull::default()) } }
        fn scan(root: &Path) -> BTreeMap<String, (u64, u64)> {
            let mut m = BTreeMap::new();
            for e in walkdir::WalkDir::new(root).into_iter().flatten() {
                if !e.file_type().is_file() { continue; }
                let md = e.metadata().unwrap(); let mtime = md.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
                m.insert(e.path().strip_prefix(root).unwrap().to_string_lossy().to_string(), (md.len(), mtime));
            }
            m
        }
    }
    #[async_trait]
    impl RemoteOps for FakeRemote {
        async fn push_changes(&self, local_root: &Path, _excludes: &[String]) -> Result<PushStats> {
            let now = Self::scan(local_root);
            let mut prev = self.manifest.lock();
            let changed = match prev.as_ref() { None => now.len() as u64, Some(p) => now.iter().filter(|(k, v)| p.get(*k) != Some(v)).count() as u64 + p.keys().filter(|k| !now.contains_key(*k)).count() as u64 };
            *prev = Some(now);
            self.pushes.lock().push(changed);
            Ok(PushStats { files: changed, bytes: changed * 10, deleted: 0 })
        }
        async fn exec(&self, command: &str, timeout_secs: u64) -> Result<RemoteExec> {
            self.execs.lock().push((command.to_string(), timeout_secs));
            if let Some(d) = self.exec_delay { tokio::time::sleep(d).await; }
            self.exec_result.lock().take().unwrap_or_else(|| Ok(RemoteExec::default()))
        }
        async fn pull_changes(&self, _local_root: &Path, _excludes: &[String]) -> Result<RemotePull> {
            self.pulls.fetch_add(1, Ordering::SeqCst);
            Ok(std::mem::take(&mut *self.pull.lock()))
        }
    }

    fn workspace() -> tempfile::TempDir { let d = tempfile::tempdir().unwrap(); std::fs::write(d.path().join("a.txt"), "a").unwrap(); std::fs::create_dir_all(d.path().join("src")).unwrap(); std::fs::write(d.path().join("src/b.rs"), "b").unwrap(); d }

    #[tokio::test]
    async fn warren_run_pushes_execs_in_mapped_cwd_and_pulls() {
        let ws = workspace();
        let fake = Arc::new(FakeRemote::new());
        *fake.exec_result.lock() = Some(Ok(RemoteExec { exit_code: 3, stdout: "hello".into(), stderr: "warn: x\n".into(), truncated: true }));
        fake.pull.lock().changed.push(("gen.txt".into(), b"made remotely".to_vec()));
        fake.pull.lock().deleted.push("a.txt".into());
        let ex = WarrenExecutor::with_remote(fake.clone(), binding(), ws.path().to_path_buf(), vec!["node_modules".into()]);
        let res = ex.run(&spec(&ws.path().join("src"), "cargo test"), &CancellationToken::new()).await.unwrap();
        assert_eq!(res.code, Some(3)); assert!(!res.timed_out && !res.cancelled);
        assert!(res.output.starts_with("hello\nwarn: x\n"), "stdout then stderr: {}", res.output);
        assert!(res.output.contains("truncated"));
        let execs = fake.execs.lock().clone();
        assert_eq!(execs.len(), 1); assert_eq!(execs[0].1, 30);
        assert!(execs[0].0.starts_with("cd '/home/agent/work/proj/src' && export "), "{}", execs[0].0);
        assert!(execs[0].0.ends_with(" && cargo test"));
        assert_eq!(fake.pushes.lock().as_slice(), &[2], "whole tree on the first push");
        assert_eq!(fake.pulls.load(Ordering::SeqCst), 1);
        let sync = ex.take_remote_sync();
        assert_eq!(sync.pushed_files, 2); assert_eq!(sync.local_root, ws.path());
        assert_eq!(sync.changed, vec![("gen.txt".to_string(), b"made remotely".to_vec())]); assert_eq!(sync.deleted, vec!["a.txt".to_string()]);
        assert!(ex.take_remote_sync().changed.is_empty(), "taken once");
        assert_eq!(ex.kind(), "warren"); assert_eq!(ex.describe(), "warren inst_42"); assert!(ex.sandboxed());
        assert!(ex.spawn_background(&spec(ws.path(), "sleep 9"), Path::new("/dev/null")).await.is_err(), "remote executors refuse background commands");
    }

    #[tokio::test]
    async fn warren_run_skips_the_push_when_nothing_changed() {
        let ws = workspace();
        let fake = Arc::new(FakeRemote::new());
        let ex = WarrenExecutor::with_remote(fake.clone(), binding(), ws.path().to_path_buf(), vec![]);
        ex.run(&spec(ws.path(), "ls"), &CancellationToken::new()).await.unwrap();
        *fake.exec_result.lock() = Some(Ok(RemoteExec::default()));
        ex.run(&spec(ws.path(), "ls"), &CancellationToken::new()).await.unwrap();
        assert_eq!(ex.take_remote_sync().pushed_files, 0, "second run: nothing changed, nothing pushed");
        std::fs::write(ws.path().join("src/c.rs"), "c").unwrap();
        *fake.exec_result.lock() = Some(Ok(RemoteExec::default()));
        ex.run(&spec(ws.path(), "ls"), &CancellationToken::new()).await.unwrap();
        assert_eq!(fake.pushes.lock().as_slice(), &[2, 0, 1]);
        assert_eq!(ex.take_remote_sync().pushed_files, 1);
    }

    #[tokio::test]
    async fn warren_run_maps_timeout_errors_and_propagates_others() {
        let ws = workspace();
        let fake = Arc::new(FakeRemote::new());
        *fake.exec_result.lock() = Some(Err(CaretError::new("exec_timeout", "command timed out after 30s")));
        let ex = WarrenExecutor::with_remote(fake.clone(), binding(), ws.path().to_path_buf(), vec![]);
        let res = ex.run(&spec(ws.path(), "sleep 99"), &CancellationToken::new()).await.unwrap();
        assert!(res.timed_out && res.code.is_none() && res.output.contains("timed out"), "{res:?}");
        *fake.exec_result.lock() = Some(Err(CaretError::new("http", "502 bad gateway")));
        let err = ex.run(&spec(ws.path(), "ls"), &CancellationToken::new()).await.unwrap_err();
        assert_eq!(err.code, "http");
    }

    #[tokio::test]
    async fn warren_run_returns_early_on_cancel() {
        let ws = workspace();
        let mut fake = FakeRemote::new(); fake.exec_delay = Some(Duration::from_secs(20));
        let fake = Arc::new(fake);
        let ex = WarrenExecutor::with_remote(fake.clone(), binding(), ws.path().to_path_buf(), vec![]);
        let cancel = CancellationToken::new();
        let c2 = cancel.clone();
        tokio::spawn(async move { tokio::time::sleep(Duration::from_millis(50)).await; c2.cancel(); });
        let started = Instant::now();
        let res = ex.run(&spec(ws.path(), "sleep 20"), &cancel).await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(res.cancelled && res.code.is_none() && !res.timed_out);
        assert!(res.output.contains("keeps running"), "{}", res.output);
        assert_eq!(fake.execs.lock().len(), 1, "the exec was issued; Warren has no cancel");
        assert_eq!(fake.pulls.load(Ordering::SeqCst), 0, "no pull after a cancelled run");
    }

    #[tokio::test]
    async fn local_executor_runs_with_default_env_and_cancel() {
        let ws = workspace();
        let ex = LocalExecutor::default();
        assert_eq!(ex.kind(), "local"); assert_eq!(ex.describe(), "local"); assert!(!ex.sandboxed());
        let mut s = spec(ws.path(), "echo \"$CARET-$NO_COLOR-$EXTRA\"; pwd"); s.shell = "/bin/sh".into(); s.env.insert("EXTRA".into(), "e".into());
        let r = ex.run(&s, &CancellationToken::new()).await.unwrap();
        // `-l` sources the user's profile, which may print to stderr first: look for the line, not the prefix.
        assert_eq!(r.code, Some(0)); assert!(r.output.contains("1-1-e\n"), "{}", r.output);
        assert!(r.output.trim_end().ends_with(&ws.path().canonicalize().unwrap().to_string_lossy().to_string()) || r.output.contains(&ws.path().to_string_lossy().to_string()));
        let mut s = spec(ws.path(), "exit 7"); s.shell = "/bin/sh".into();
        assert_eq!(ex.run(&s, &CancellationToken::new()).await.unwrap().code, Some(7));
        let mut s = spec(ws.path(), "sleep 30"); s.shell = "/bin/sh".into(); s.timeout_secs = 1;
        let r = ex.run(&s, &CancellationToken::new()).await.unwrap();
        assert!(r.timed_out && r.code.is_none());
        assert!(ex.take_remote_sync().changed.is_empty());
    }
}

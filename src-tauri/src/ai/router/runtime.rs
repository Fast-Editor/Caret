//! Where Lynkr's files live and how they get there: a managed npm install into `~/.caret/lynkr` (default) or a copy
//! of resources shipped with the app ("bundled"). Nothing here touches the user's own Lynkr installs.
use crate::error::{CaretError, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Exact Lynkr release Caret installs and expects. Bump together with the `.env` template in `env.rs`.
pub const LYNKR_VERSION: &str = "9.14.22";
/// First release that honours `LYNKR_BIND_HOST` / `LYNKR_API_KEY` (upstream patch pending); older builds listen on
/// every interface without authentication, so the UI shows a warning.
pub const LYNKR_PATCHED_VERSION: &str = "9.15.0";
const INSTALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15 * 60);

#[derive(Debug, Clone)]
pub enum LynkrRuntime {
    /// `npm install lynkr@<pin>` into `prefix` (its `node_modules/lynkr`); `prefix` is also the sidecar's cwd.
    Managed { prefix: PathBuf, entry_override: Option<PathBuf> },
    /// Files copied from the app bundle's resources into `prefix` (same layout afterwards).
    Bundled { resource_dir: PathBuf, prefix: PathBuf, entry_override: Option<PathBuf> },
}

#[derive(Debug, Clone, Default)]
pub struct InstallReport { pub version: String, pub native_ok: bool }

pub fn default_prefix() -> PathBuf { crate::paths::config_dir().join("lynkr") }
/// `CARET_LYNKR_ENTRY=<path/to/index.js>` runs that file instead of the installed package (tests, local Lynkr dev).
pub fn entry_override_from_env() -> Option<PathBuf> { std::env::var_os("CARET_LYNKR_ENTRY").map(PathBuf::from).filter(|p| p.is_file()) }

impl LynkrRuntime {
    pub fn managed() -> Self { LynkrRuntime::Managed { prefix: default_prefix(), entry_override: entry_override_from_env() } }
    pub fn managed_at(prefix: PathBuf, entry_override: Option<PathBuf>) -> Self { LynkrRuntime::Managed { prefix, entry_override } }
    /// Pick the runtime for `settings.router.source` ("npm" | "bundled").
    pub fn for_source(source: &str, resource_dir: Option<PathBuf>) -> Self {
        match (source, resource_dir) {
            ("bundled", Some(dir)) => LynkrRuntime::Bundled { resource_dir: dir, prefix: default_prefix(), entry_override: entry_override_from_env() },
            _ => Self::managed(),
        }
    }
    pub fn source(&self) -> &'static str { match self { LynkrRuntime::Managed { .. } => "npm", LynkrRuntime::Bundled { .. } => "bundled" } }
    /// Working directory for the sidecar (its `.env`, `.lynkr/`, `data/`, `logs/`).
    pub fn prefix(&self) -> &Path { match self { LynkrRuntime::Managed { prefix, .. } | LynkrRuntime::Bundled { prefix, .. } => prefix } }
    pub fn entry_override(&self) -> Option<&Path> { match self { LynkrRuntime::Managed { entry_override, .. } | LynkrRuntime::Bundled { entry_override, .. } => entry_override.as_deref() } }
    pub fn entry_js(&self) -> PathBuf { self.entry_override().map(|p| p.to_path_buf()).unwrap_or_else(|| self.prefix().join("node_modules/lynkr/index.js")) }
    pub fn installed_version(&self) -> Option<String> {
        if self.entry_override().is_some() { return Some(LYNKR_VERSION.to_string()); }
        let pkg = std::fs::read_to_string(self.prefix().join("node_modules/lynkr/package.json")).ok()?;
        serde_json::from_str::<serde_json::Value>(&pkg).ok()?.get("version")?.as_str().map(|s| s.to_string())
    }
    pub fn is_installed(&self) -> bool { self.installed_version().as_deref() == Some(LYNKR_VERSION) && self.entry_js().is_file() }
    /// True when the installed build is known to honour the loopback bind + API key.
    pub fn is_patched(&self) -> bool { self.installed_version().map(|v| version_at_least(&v, LYNKR_PATCHED_VERSION)).unwrap_or(false) }

    pub fn node_bin() -> Option<PathBuf> { crate::acp::process::resolve_command("node") }
    pub fn npm_bin() -> Option<PathBuf> { crate::acp::process::resolve_command("npm") }
    pub const NODE_MISSING: &'static str = "Node.js ≥ 20 not found (install from nodejs.org, nvm or Homebrew)";

    /// Create the prefix (0700 on unix) plus `logs/` and `data/`.
    pub fn ensure_dirs(&self) -> Result<()> {
        let p = self.prefix();
        std::fs::create_dir_all(p)?;
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)); }
        std::fs::create_dir_all(p.join("logs"))?; std::fs::create_dir_all(p.join("data"))?;
        Ok(())
    }

    /// `node -e "require('better-sqlite3')"` from the prefix: pins/telemetry/de-escalation need it.
    pub async fn probe_native(&self) -> bool {
        let Some(node) = Self::node_bin() else { return false };
        if self.entry_override().is_some() && !self.prefix().join("node_modules/better-sqlite3").exists() { return false; }
        let out = tokio::process::Command::new(node).args(["-e", "require('better-sqlite3')"]).current_dir(self.prefix()).env("PATH", crate::acp::process::augmented_path()).output().await;
        out.map(|o| o.status.success()).unwrap_or(false)
    }

    /// Install (or reinstall) the pinned Lynkr release. Streams every tool line through `log`.
    pub async fn install(&self, log: &(dyn Fn(String) + Send + Sync)) -> Result<InstallReport> {
        self.ensure_dirs()?;
        match self {
            LynkrRuntime::Bundled { resource_dir, prefix, .. } => {
                let src = resource_dir.join("lynkr");
                if !src.join("node_modules/lynkr/index.js").is_file() {
                    return Err(CaretError::not_found("bundled Lynkr resources in this build (switch Auto routing → Source to npm)"));
                }
                log(format!("[install] copying bundled Lynkr from {} to {}", src.display(), prefix.display()));
                copy_tree(&src, prefix)?;
            }
            LynkrRuntime::Managed { prefix, .. } => {
                let npm = Self::npm_bin().ok_or_else(|| CaretError::not_found(Self::NODE_MISSING))?;
                Self::node_bin().ok_or_else(|| CaretError::not_found(Self::NODE_MISSING))?;
                let pkg = prefix.join("package.json");
                if !pkg.exists() { std::fs::write(&pkg, "{\n  \"name\": \"caret-lynkr\",\n  \"private\": true\n}\n")?; }
                run_logged(&npm, &["install", &format!("lynkr@{LYNKR_VERSION}"), "--omit=optional", "--ignore-scripts", "--no-audit", "--no-fund", "--loglevel", "error"], prefix, log, true).await?;
                // `--omit=optional` again: without it npm re-resolves lynkr's optional deps (onnxruntime, tree-sitter ≈ 400 MB).
                run_logged(&npm, &["install", "better-sqlite3@^12", "--omit=optional", "--no-audit", "--no-fund", "--loglevel", "error"], prefix, log, true).await?;
                let has_clt = std::process::Command::new("xcode-select").arg("-p").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
                if has_clt { if let Err(e) = run_logged(&npm, &["rebuild", "hnswlib-node", "--loglevel", "error"], prefix, log, false).await { log(format!("[install] hnswlib-node rebuild skipped: {}", e.message)); } }
                else { log("[install] Xcode Command Line Tools not found; skipping hnswlib-node rebuild (kNN routing hints stay off)".into()); }
            }
        }
        let version = self.installed_version().ok_or_else(|| CaretError::io("Lynkr package.json missing after install"))?;
        if version != LYNKR_VERSION { return Err(CaretError::io(format!("installed Lynkr {version} but Caret pins {LYNKR_VERSION}"))); }
        let native_ok = self.probe_native().await;
        log(format!("[install] done: lynkr {version}, better-sqlite3 {}", if native_ok { "ok" } else { "unavailable (pins/telemetry/de-escalation degrade)" }));
        Ok(InstallReport { version, native_ok })
    }
}

/// Run a tool, piping stdout/stderr line-by-line into `log` (so the Settings page shows progress live).
async fn run_logged(bin: &Path, args: &[&str], cwd: &Path, log: &(dyn Fn(String) + Send + Sync), must_succeed: bool) -> Result<()> {
    log(format!("[install] {} {}", bin.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), args.join(" ")));
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args).current_dir(cwd).env("PATH", crate::acp::process::augmented_path()).env("NODE_ENV", "production").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| CaretError::io(format!("failed to start {}: {e}", bin.display())))?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    if let Some(out) = child.stdout.take() { let tx = tx.clone(); tokio::spawn(async move { let mut l = BufReader::new(out).lines(); while let Ok(Some(line)) = l.next_line().await { let _ = tx.send(line); } }); }
    if let Some(err) = child.stderr.take() { let tx = tx.clone(); tokio::spawn(async move { let mut l = BufReader::new(err).lines(); while let Ok(Some(line)) = l.next_line().await { let _ = tx.send(line); } }); }
    drop(tx);
    let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let wait = async {
        while let Some(line) = rx.recv().await { if !line.trim().is_empty() { tail.push_back(line.clone()); if tail.len() > 20 { tail.pop_front(); } log(format!("[install] {line}")); } }
        child.wait().await
    };
    let status = tokio::time::timeout(INSTALL_TIMEOUT, wait).await.map_err(|_| CaretError::io("npm step timed out after 15 minutes"))?.map_err(|e| CaretError::io(format!("npm step failed: {e}")))?;
    if !status.success() && must_succeed { return Err(CaretError::io(format!("`{} {}` failed (exit {}): {}", bin.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), args.join(" "), status.code().unwrap_or(-1), tail.iter().cloned().collect::<Vec<_>>().join(" | ").chars().take(600).collect::<String>()))); }
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        let rel = entry.path().strip_prefix(src).unwrap_or(entry.path());
        let target = dst.join(rel);
        if entry.file_type().is_dir() { std::fs::create_dir_all(&target)?; } else if entry.file_type().is_file() { if let Some(p) = target.parent() { std::fs::create_dir_all(p)?; } std::fs::copy(entry.path(), &target)?; }
    }
    Ok(())
}

/// Lexicographic-by-number semver comparison (ignores pre-release tags).
pub fn version_at_least(v: &str, min: &str) -> bool {
    let parse = |s: &str| s.split(|c: char| c == '-' || c == '+').next().unwrap_or("").split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (a, b) = (parse(v), parse(min));
    for i in 0..a.len().max(b.len()) { let (x, y) = (*a.get(i).unwrap_or(&0), *b.get(i).unwrap_or(&0)); if x != y { return x > y; } }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_version_reads_package_json_and_requires_entry() {
        let dir = tempfile::tempdir().unwrap();
        let rt = LynkrRuntime::managed_at(dir.path().to_path_buf(), None);
        assert_eq!(rt.installed_version(), None); assert!(!rt.is_installed());
        std::fs::create_dir_all(dir.path().join("node_modules/lynkr")).unwrap();
        std::fs::write(dir.path().join("node_modules/lynkr/package.json"), format!("{{\"name\":\"lynkr\",\"version\":\"{LYNKR_VERSION}\"}}")).unwrap();
        assert_eq!(rt.installed_version().as_deref(), Some(LYNKR_VERSION));
        assert!(!rt.is_installed(), "index.js missing");
        std::fs::write(dir.path().join("node_modules/lynkr/index.js"), "").unwrap();
        assert!(rt.is_installed());
        std::fs::write(dir.path().join("node_modules/lynkr/package.json"), "{\"version\":\"9.0.0\"}").unwrap();
        assert!(!rt.is_installed(), "version mismatch → reinstall");
    }
    #[test]
    fn entry_override_skips_install_and_pins_version() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("index.js"); std::fs::write(&entry, "").unwrap();
        let rt = LynkrRuntime::managed_at(dir.path().join("prefix"), Some(entry.clone()));
        assert_eq!(rt.entry_js(), entry); assert!(rt.is_installed()); assert_eq!(rt.installed_version().as_deref(), Some(LYNKR_VERSION));
    }
    #[test]
    fn version_compare() { assert!(version_at_least("9.15.0", "9.15.0")); assert!(version_at_least("10.0.1", "9.15.0")); assert!(!version_at_least("9.14.22", "9.15.0")); assert!(version_at_least("9.15.1-beta.1", "9.15.0")); }
    #[test]
    fn ensure_dirs_creates_private_prefix() {
        let dir = tempfile::tempdir().unwrap(); let prefix = dir.path().join("lynkr");
        LynkrRuntime::managed_at(prefix.clone(), None).ensure_dirs().unwrap();
        assert!(prefix.join("logs").is_dir() && prefix.join("data").is_dir());
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(&prefix).unwrap().permissions().mode() & 0o777, 0o700); }
    }
}

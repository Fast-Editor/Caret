//! macOS Seatbelt backend: wraps a command in `sandbox-exec -p <profile>`.
//!
//! Profile shape (deny-by-default, then allow): full process/mach/sysctl/ipc so ordinary dev tooling works, read of the
//! whole filesystem minus credential directories, writes only to an explicit allowlist (workspace, temp, caches,
//! `~/.caret/sandbox`, user extras), and network only when the policy allows it.
use super::SandboxPolicy;
use std::path::{Path, PathBuf};

pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

pub fn available() -> bool { cfg!(target_os = "macos") && Path::new(SANDBOX_EXEC).exists() }

fn esc(p: &Path) -> String { p.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"") }

fn home() -> PathBuf { crate::paths::home_dir() }

/// Directories that must never be readable by sandboxed commands (credentials / tokens), relative to home.
pub const SECRET_DIRS: &[&str] = &[
    ".ssh", ".aws", ".gnupg", ".azure", ".kube", ".config/gcloud", ".config/gh", ".config/op", ".config/hub", ".password-store",
    ".1password", "Library/Keychains", "Library/Cookies", "Library/Application Support/Google/Chrome", "Library/Application Support/Firefox",
    ".docker", ".netrc", ".npmrc", ".pypirc", ".gem/credentials", ".config/git/credentials", ".git-credentials", ".cargo/credentials", ".cargo/credentials.toml",
    ".terraform.d/credentials.tfrc.json", ".vault-token", ".claude/.credentials.json", ".codex/auth.json", ".config/configstore/update-notifier-npm.json",
];
/// Credential-like file names that are denied anywhere under home when reading.
const SECRET_FILE_REGEX: &[&str] = &[r"/id_rsa$", r"/id_ed25519$", r"/id_ecdsa$", r"/id_dsa$", r"\.pem$", r"\.p12$", r"\.pfx$", r"/credentials\.json$", r"/service-account[^/]*\.json$"];

/// Developer directories under home that stay readable when `allow_read_home` is false.
const DEV_READ_DIRS: &[&str] = &[
    ".cargo", ".rustup", ".nvm", ".volta", ".fnm", ".bun", ".deno", ".pyenv", ".rbenv", ".asdf", ".sdkman", ".local", ".config", ".cache",
    ".npm", ".yarn", ".pnpm-store", "Library/pnpm", "Library/Caches", "Library/Developer", "go", ".gradle", ".m2", ".nix-profile", ".oh-my-zsh", ".zsh", ".antigen", ".tmux",
];
const DEV_READ_FILES: &[&str] = &[".zshrc", ".zshenv", ".zprofile", ".zlogin", ".bashrc", ".bash_profile", ".profile", ".bash_login", ".inputrc", ".gitconfig", ".gitignore_global", ".gitignore", ".gitattributes", ".tool-versions", ".node-version", ".nvmrc", ".python-version", ".ruby-version", ".vimrc", ".editorconfig", ".p10k.zsh", ".fzf.zsh", ".hushlogin", ".zcompdump", ".CFUserTextEncoding"];
/// Cache / package-manager directories that are always writable so builds and installs work.
const CACHE_WRITE_DIRS: &[&str] = &[
    ".cache", "Library/Caches", ".npm", ".yarn", ".pnpm-store", "Library/pnpm", ".local/share/pnpm", ".local/state", ".bun/install/cache",
    ".cargo/registry", ".cargo/git", ".cargo/.package-cache", ".cargo/.crates.toml", ".cargo/.crates2.json", ".rustup", ".gradle", ".m2/repository", "go/pkg",
    ".config/configstore", ".config/yarn", ".local/share/uv", ".cache/uv", ".pyenv/versions", ".deno", ".zcompdump", ".zsh_sessions", ".bash_sessions",
    ".caret/sandbox", ".caret/logs",
];

/// Writable roots for a policy (deduplicated, canonicalized when they exist).
pub fn writable_roots(policy: &SandboxPolicy) -> Vec<PathBuf> {
    let h = home();
    let mut out: Vec<PathBuf> = vec![policy.workspace_root.clone()];
    out.extend(policy.writable.iter().cloned());
    out.push(PathBuf::from("/tmp")); out.push(PathBuf::from("/private/tmp")); out.push(PathBuf::from("/var/tmp")); out.push(PathBuf::from("/private/var/tmp"));
    let tmp = std::env::temp_dir();
    out.push(tmp.clone());
    if let Some(parent) = tmp.parent() { out.push(parent.to_path_buf()); } // …/xx/yy/ (T, C, X siblings)
    for c in CACHE_WRITE_DIRS { out.push(h.join(c)); }
    for p in &policy.writable { out.push(p.clone()); }
    let mut seen = std::collections::HashSet::new();
    let mut res = vec![];
    for p in out {
        let p = p.canonicalize().unwrap_or(p);
        if p.as_os_str().is_empty() || p == Path::new("/") { continue; }
        if seen.insert(p.clone()) { res.push(p); }
    }
    res
}

/// Build the SBPL profile for a policy.
pub fn profile(policy: &SandboxPolicy, allow_read_home: bool) -> String {
    let h = home();
    let mut s = String::with_capacity(8192);
    s.push_str("(version 1)\n(deny default)\n");
    s.push_str("(allow process*)\n(allow signal)\n(allow sysctl*)\n(allow mach*)\n(allow ipc*)\n(allow iokit*)\n(allow system*)\n(allow pseudo-tty)\n(allow file-ioctl)\n(allow user-preference-read)\n(allow distributed-notification-post)\n(allow lsopen)\n");
    // reads
    s.push_str("(allow file-read*)\n");
    if !allow_read_home {
        s.push_str(&format!("(deny file-read-data (subpath \"{}\"))\n", esc(&h)));
        s.push_str("(allow file-read-data");
        s.push_str(&format!(" (subpath \"{}\")", esc(&policy.workspace_root)));
        for p in &policy.writable { s.push_str(&format!(" (subpath \"{}\")", esc(p))); }
        for d in DEV_READ_DIRS { s.push_str(&format!(" (subpath \"{}\")", esc(&h.join(d)))); }
        for f in DEV_READ_FILES { s.push_str(&format!(" (literal \"{}\")", esc(&h.join(f)))); }
        s.push_str(")\n");
    }
    // credentials are never readable, whatever came before
    s.push_str("(deny file-read*");
    for d in SECRET_DIRS { s.push_str(&format!(" (subpath \"{}\") (literal \"{}\")", esc(&h.join(d)), esc(&h.join(d)))); }
    for r in SECRET_FILE_REGEX { s.push_str(&format!(" (regex #\"^{}.*{}\")", regex_escape(&h.to_string_lossy()), r)); }
    s.push_str(&format!(" (subpath \"{}\")", esc(&h.join(".caret"))));
    s.push_str(")\n");
    s.push_str(&format!("(allow file-read* (subpath \"{}\") (subpath \"{}\"))\n", esc(&h.join(".caret/sandbox")), esc(&h.join(".caret/logs"))));
    // writes
    s.push_str("(allow file-write*");
    for p in writable_roots(policy) { s.push_str(&format!(" (subpath \"{}\")", esc(&p))); }
    s.push_str(")\n");
    s.push_str("(allow file-write* (literal \"/dev/null\") (literal \"/dev/zero\") (literal \"/dev/tty\") (literal \"/dev/stdout\") (literal \"/dev/stderr\") (literal \"/dev/random\") (literal \"/dev/urandom\") (literal \"/dev/dtracehelper\") (literal \"/dev/ptmx\") (literal \"/dev/autofs_nowait\") (regex #\"^/dev/ttys[0-9]+$\") (regex #\"^/dev/fd/[0-9]+$\") (regex #\"^/dev/pty[a-z0-9]+$\"))\n");
    // writes into credential dirs are never allowed even if a writable root contains them (e.g. workspace == home)
    s.push_str("(deny file-write*");
    for d in SECRET_DIRS { s.push_str(&format!(" (subpath \"{}\") (literal \"{}\")", esc(&h.join(d)), esc(&h.join(d)))); }
    for f in [".zshrc", ".zshenv", ".zprofile", ".bashrc", ".bash_profile", ".profile", ".gitconfig"] { s.push_str(&format!(" (literal \"{}\")", esc(&h.join(f)))); }
    s.push_str(")\n");
    // network
    if policy.allow_network { s.push_str("(allow network*)\n"); } else { s.push_str("(deny network*)\n(allow network-outbound (remote unix-socket))\n(allow network-inbound (local unix-socket))\n(allow network-bind (local unix-socket))\n"); }
    s
}

fn regex_escape(s: &str) -> String { let mut o = String::new(); for c in s.chars() { if "\\.+*?()|[]{}^$#".contains(c) { o.push('\\'); } o.push(c); } o }

/// Rebuild `cmd` as `sandbox-exec -p <profile> -- <program> <args…>`, preserving cwd and env edits.
pub fn wrap(cmd: tokio::process::Command, policy: &SandboxPolicy, allow_read_home: bool) -> tokio::process::Command {
    let std_cmd = cmd.as_std();
    let program = std_cmd.get_program().to_os_string();
    let args: Vec<std::ffi::OsString> = std_cmd.get_args().map(|a| a.to_os_string()).collect();
    let envs: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)> = std_cmd.get_envs().map(|(k, v)| (k.to_os_string(), v.map(|v| v.to_os_string()))).collect();
    let cwd = std_cmd.get_current_dir().map(|p| p.to_path_buf());
    let mut out = tokio::process::Command::new(SANDBOX_EXEC);
    out.arg("-p").arg(profile(policy, allow_read_home)).arg(program).args(args);
    for (k, v) in envs { match v { Some(v) => { out.env(k, v); } None => { out.env_remove(k); } } }
    if let Some(c) = cwd { out.current_dir(c); }
    out.env("CARET_SANDBOXED", "1");
    out
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::process::Stdio;

    fn policy(ws: &Path, net: bool) -> SandboxPolicy { SandboxPolicy { workspace_root: ws.to_path_buf(), writable: vec![], allow_network: net, mode: "auto".into() } }

    async fn run(ws: &Path, net: bool, read_home: bool, script: &str) -> (Option<i32>, String) {
        let mut c = tokio::process::Command::new("/bin/sh");
        c.arg("-c").arg(script).current_dir(ws);
        let mut c = wrap(c, &policy(ws, net), read_home);
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let out = tokio::time::timeout(std::time::Duration::from_secs(20), c.output()).await.expect("timeout").expect("spawn");
        (out.status.code(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
    }

    fn tmp_ws(name: &str) -> PathBuf { let p = std::env::temp_dir().join(format!("caret-sb-{name}-{}", std::process::id())); let _ = std::fs::remove_dir_all(&p); std::fs::create_dir_all(&p).unwrap(); p.canonicalize().unwrap() }

    #[tokio::test]
    async fn runs_ordinary_commands_and_allows_workspace_writes() {
        if !available() { return; }
        let ws = tmp_ws("basic");
        let (code, out) = run(&ws, true, true, "echo hi && ls / >/dev/null && echo data > inside.txt && cat inside.txt && mkdir -p sub/dir && rm -r sub && echo $CARET_SANDBOXED").await;
        assert_eq!(code, Some(0), "{out}");
        assert!(out.contains("hi") && out.contains("data") && out.contains("1"), "{out}");
        assert!(ws.join("inside.txt").exists());
        // git + an interpreter work inside the sandbox
        let (code, out) = run(&ws, true, true, "git init -q . && git status --short && /usr/bin/python3 -c 'print(6*7)' 2>/dev/null || echo pyskip").await;
        assert_eq!(code, Some(0), "{out}");
    }

    #[tokio::test]
    async fn blocks_writes_outside_workspace() {
        if !available() { return; }
        let ws = tmp_ws("writes");
        let outside = std::env::temp_dir().join(format!("caret-sb-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&outside); std::fs::create_dir_all(&outside).unwrap();
        // temp dir itself is a writable root, so pick a non-temp, non-workspace path: the home dir
        let target = crate::paths::home_dir().join(format!(".caret-sandbox-test-{}", std::process::id()));
        let (code, _out) = run(&ws, true, true, &format!("echo pwned > '{}'", target.display())).await;
        assert_ne!(code, Some(0));
        assert!(!target.exists(), "write outside workspace must be denied");
        let _ = std::fs::remove_file(&target);
        // /usr/local and /etc are never writable
        let (code, _) = run(&ws, true, true, "touch /etc/caret-sandbox-test 2>/dev/null").await;
        assert_ne!(code, Some(0));
        assert!(!Path::new("/etc/caret-sandbox-test").exists());
    }

    #[tokio::test]
    async fn blocks_reading_credentials() {
        if !available() { return; }
        let ws = tmp_ws("secrets");
        // Create a throwaway key file under a *real* secret dir name inside a fake HOME is not possible (profile uses the
        // actual home); instead assert against ~/.ssh when it exists, otherwise ~/.caret (always denied except sandbox/).
        let h = crate::paths::home_dir();
        let ssh = h.join(".ssh");
        let probe = if ssh.is_dir() { ssh } else { h.join(".caret") };
        let (code, out) = run(&ws, true, true, &format!("ls '{}'", probe.display())).await;
        assert_ne!(code, Some(0), "listing {} must fail: {out}", probe.display());
        // but the sandbox scratch dir under ~/.caret is fine
        let scratch = h.join(".caret/sandbox"); let _ = std::fs::create_dir_all(&scratch);
        let (code, out) = run(&ws, true, true, &format!("echo ok > '{}/probe.txt' && cat '{}/probe.txt'", scratch.display(), scratch.display())).await;
        assert_eq!(code, Some(0), "{out}");
        assert!(out.contains("ok"));
    }

    #[tokio::test]
    async fn network_toggle() {
        if !available() { return; }
        let ws = tmp_ws("net");
        // Without network: a TCP connect must fail fast (connection to localhost on a closed port errors either way, so use
        // a DNS + socket attempt via python if present, else nc).
        let script = "if command -v /usr/bin/python3 >/dev/null; then /usr/bin/python3 -c 'import socket,sys\ns=socket.socket()\ns.settimeout(3)\ntry:\n  s.connect((\"127.0.0.1\", 9))\n  print(\"connected\")\nexcept PermissionError:\n  print(\"denied\")\nexcept OSError as e:\n  print(\"oserror\", e.errno)\n'; else echo skip; fi";
        let (_, out) = run(&ws, false, true, script).await;
        assert!(out.contains("denied") || out.contains("skip") || out.contains("oserror 1"), "expected sandbox to deny the socket: {out}");
        let (_, out_net) = run(&ws, true, true, script).await;
        assert!(!out_net.contains("denied"), "network allowed should not be denied by sandbox: {out_net}");
    }

    #[tokio::test]
    async fn read_home_restriction() {
        if !available() { return; }
        let ws = tmp_ws("home");
        let h = crate::paths::home_dir();
        let secret = h.join(format!(".caret-sb-home-probe-{}.txt", std::process::id()));
        std::fs::write(&secret, "top secret").unwrap();
        let (code, out) = run(&ws, true, false, &format!("cat '{}'", secret.display())).await;
        assert!(code != Some(0) && !out.contains("top secret"), "home read should be denied: {out}");
        let (code, out) = run(&ws, true, true, &format!("cat '{}'", secret.display())).await;
        assert!(code == Some(0) && out.contains("top secret"), "home read allowed by default: {out}");
        let _ = std::fs::remove_file(&secret);
        // workspace stays readable even under home restriction, and shell startup still works (login shell reads rc files)
        std::fs::write(ws.join("f.txt"), "ws-ok").unwrap();
        let (code, out) = run(&ws, true, false, "cat f.txt && /bin/zsh -lc 'echo zsh-ok' 2>/dev/null").await;
        assert_eq!(code, Some(0), "{out}");
        assert!(out.contains("ws-ok"));
    }

    #[test]
    fn profile_is_well_formed() {
        let ws = PathBuf::from("/tmp/w\"ith quote");
        let p = profile(&policy(&ws, false), false);
        assert!(p.starts_with("(version 1)\n(deny default)"));
        assert!(p.contains("(deny network*)"));
        assert!(p.contains("\\\"ith quote"));
        let opens = p.matches('(').count(); let closes = p.matches(')').count();
        assert_eq!(opens, closes, "unbalanced parens in profile");
    }
}

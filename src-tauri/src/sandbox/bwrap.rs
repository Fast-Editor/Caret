//! Linux backend: bubblewrap (`bwrap`). Read-only root, writable workspace/temp/caches, masked credential
//! directories, optional network namespace isolation.
use super::SandboxPolicy;
use std::path::{Path, PathBuf};

pub fn binary() -> Option<PathBuf> { if cfg!(target_os = "linux") { which::which("bwrap").ok() } else { None } }
pub fn available() -> bool { binary().is_some() }

/// Argument vector for bwrap (without the trailing `-- program args`).
pub fn args(policy: &SandboxPolicy, allow_read_home: bool) -> Vec<String> {
    let h = crate::paths::home_dir();
    let mut a: Vec<String> = vec!["--die-with-parent".into(), "--ro-bind".into(), "/".into(), "/".into(), "--dev".into(), "/dev".into(), "--proc".into(), "/proc".into()];
    if !allow_read_home && h.is_dir() {
        a.extend(["--tmpfs".to_string(), h.to_string_lossy().to_string()]);
        for d in super::seatbelt::SECRET_DIRS.iter().map(|d| h.join(d)).chain([".cargo", ".rustup", ".nvm", ".local", ".config", ".cache", ".npm", "go", ".gradle", ".m2"].iter().map(|d| h.join(d))) {
            if d.is_dir() { a.extend(["--ro-bind".to_string(), d.to_string_lossy().to_string(), d.to_string_lossy().to_string()]); }
        }
        for f in [".bashrc", ".bash_profile", ".profile", ".zshrc", ".zshenv", ".zprofile", ".gitconfig"] { let p = h.join(f); if p.is_file() { a.extend(["--ro-bind".to_string(), p.to_string_lossy().to_string(), p.to_string_lossy().to_string()]); } }
    }
    for p in super::seatbelt::writable_roots(policy) { if p.exists() { a.extend(["--bind".to_string(), p.to_string_lossy().to_string(), p.to_string_lossy().to_string()]); } }
    // mask credentials (after binds so a workspace under home cannot expose them)
    for d in super::seatbelt::SECRET_DIRS { let p = h.join(d); if p.is_dir() { a.extend(["--tmpfs".to_string(), p.to_string_lossy().to_string()]); } else if p.is_file() { a.extend(["--ro-bind".to_string(), "/dev/null".to_string(), p.to_string_lossy().to_string()]); } }
    if !policy.allow_network { a.push("--unshare-net".into()); }
    a.extend(["--setenv".to_string(), "CARET_SANDBOXED".to_string(), "1".to_string()]);
    a
}

pub fn wrap(cmd: tokio::process::Command, policy: &SandboxPolicy, allow_read_home: bool) -> tokio::process::Command {
    let Some(bin) = binary() else { return cmd };
    let std_cmd = cmd.as_std();
    let program = std_cmd.get_program().to_os_string();
    let cargs: Vec<std::ffi::OsString> = std_cmd.get_args().map(|a| a.to_os_string()).collect();
    let envs: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)> = std_cmd.get_envs().map(|(k, v)| (k.to_os_string(), v.map(|v| v.to_os_string()))).collect();
    let cwd = std_cmd.get_current_dir().map(Path::to_path_buf);
    let mut out = tokio::process::Command::new(bin);
    out.args(args(policy, allow_read_home));
    if let Some(c) = &cwd { out.arg("--chdir").arg(c); }
    out.arg("--").arg(program).args(cargs);
    for (k, v) in envs { match v { Some(v) => { out.env(k, v); } None => { out.env_remove(k); } } }
    if let Some(c) = cwd { out.current_dir(c); }
    out.env("CARET_SANDBOXED", "1");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn args_shape() {
        let p = SandboxPolicy { workspace_root: std::env::temp_dir(), writable: vec![], allow_network: false, mode: "auto".into() };
        let a = args(&p, true);
        assert!(a.iter().any(|x| x == "--unshare-net"));
        assert!(a.windows(3).any(|w| w[0] == "--bind" && w[1] == w[2]));
        assert_eq!(a[0], "--die-with-parent");
    }
}

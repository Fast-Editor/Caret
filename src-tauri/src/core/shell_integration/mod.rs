//! Shell integration: ships rc shims that make zsh/bash/fish emit OSC 133 prompt markers, OSC 7 cwd and
//! Caret's private `OSC 7770;cmd;<base64>` command marker, so the terminal can build semantic blocks.
//! Assets are embedded and written (idempotently) under `~/.caret/shell-integration/`.
//! Disable with `settings.terminal.shellIntegration = false`.
use crate::error::Result;
use std::path::{Path, PathBuf};

const ASSETS: &[(&str, &str)] = &[
    ("zsh/.zshenv", include_str!("assets/zsh/.zshenv")),
    ("zsh/.zprofile", include_str!("assets/zsh/.zprofile")),
    ("zsh/.zshrc", include_str!("assets/zsh/.zshrc")),
    ("zsh/.zlogin", include_str!("assets/zsh/.zlogin")),
    ("zsh/caret-integration.zsh", include_str!("assets/zsh/caret-integration.zsh")),
    ("bash/bashrc", include_str!("assets/bash/bashrc")),
    ("bash/caret-integration.bash", include_str!("assets/bash/caret-integration.bash")),
    ("fish/vendor_conf.d/caret.fish", include_str!("assets/fish/vendor_conf.d/caret.fish")),
];

pub fn dir() -> PathBuf { crate::paths::config_dir().join("shell-integration") }

/// Write all assets (only when content differs). Returns the integration root directory.
pub fn ensure_installed() -> Result<PathBuf> { ensure_installed_at(&dir()) }

pub fn ensure_installed_at(root: &Path) -> Result<PathBuf> {
    for (rel, content) in ASSETS {
        let p = root.join(rel);
        if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
        let same = std::fs::read_to_string(&p).map(|c| c == *content).unwrap_or(false);
        if !same { std::fs::write(&p, content)?; }
    }
    Ok(root.to_path_buf())
}

/// How to launch a shell so it loads the integration.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Integration {
    pub env: Vec<(String, String)>,
    /// When set, replaces the default shell args (e.g. bash needs `--init-file`).
    pub args: Option<Vec<String>>,
}

/// Compute env/args for `shell_name` ("zsh" | "bash" | "fish" | …). `login` = we would have passed `-l`.
pub fn for_shell(shell_name: &str, login: bool, root: &Path, home: &Path) -> Option<Integration> {
    let mut env = vec![("CARET_TERMINAL".to_string(), "1".to_string())];
    match shell_name {
        "zsh" => {
            let user_zdotdir = std::env::var("ZDOTDIR").ok().filter(|z| !z.is_empty()).unwrap_or_else(|| home.to_string_lossy().to_string());
            env.push(("ZDOTDIR".into(), root.join("zsh").to_string_lossy().to_string()));
            env.push(("CARET_USER_ZDOTDIR".into(), user_zdotdir));
            Some(Integration { env, args: None })
        }
        "bash" => {
            if login { env.push(("CARET_BASH_LOGIN".into(), "1".into())); }
            Some(Integration { env, args: Some(vec!["--init-file".into(), root.join("bash").join("bashrc").to_string_lossy().to_string()]) })
        }
        "fish" => {
            let existing = std::env::var("XDG_DATA_DIRS").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
            env.push(("XDG_DATA_DIRS".into(), format!("{}:{}", root.join("fish").parent().unwrap_or(root).to_string_lossy(), existing)));
            Some(Integration { env, args: None })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installs_idempotently() {
        let tmp = tempfile::tempdir().unwrap();
        let root = ensure_installed_at(tmp.path()).unwrap();
        let p = root.join("zsh/caret-integration.zsh");
        let m1 = std::fs::metadata(&p).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        ensure_installed_at(tmp.path()).unwrap();
        let m2 = std::fs::metadata(&p).unwrap().modified().unwrap();
        assert_eq!(m1, m2, "unchanged assets are not rewritten");
        assert!(std::fs::read_to_string(root.join("bash/bashrc")).unwrap().contains("caret-integration.bash"));
        assert!(root.join("fish/vendor_conf.d/caret.fish").exists());
        // tamper → rewritten
        std::fs::write(&p, "broken").unwrap();
        ensure_installed_at(tmp.path()).unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().contains("133;A"));
    }
    #[test]
    fn integration_for_shells() {
        let root = Path::new("/tmp/caret-si"); let home = Path::new("/Users/x");
        let z = for_shell("zsh", true, root, home).unwrap();
        assert!(z.env.iter().any(|(k, v)| k == "ZDOTDIR" && v.ends_with("/zsh")));
        assert!(z.env.iter().any(|(k, _)| k == "CARET_USER_ZDOTDIR"));
        let b = for_shell("bash", true, root, home).unwrap();
        assert_eq!(b.args.as_deref().map(|a| a[0].as_str()), Some("--init-file"));
        assert!(b.env.iter().any(|(k, _)| k == "CARET_BASH_LOGIN"));
        let f = for_shell("fish", false, root, home).unwrap();
        assert!(f.env.iter().any(|(k, v)| k == "XDG_DATA_DIRS" && v.starts_with("/tmp/caret-si")));
        assert!(for_shell("nu", false, root, home).is_none());
    }
}

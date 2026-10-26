//! Tool execution policy: auto-run modes, command allow/deny lists, dotfile / delete / outside-workspace protection.
use crate::ai::agent::session::{ApprovalKind, Session};
use crate::core::settings::Settings;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum ToolEffect {
    /// No side effects (read/search).
    ReadOnly,
    /// Edits or creates files at these paths.
    FileEdit(Vec<String>),
    /// Deletes/renames files.
    FileDelete(Vec<String>),
    /// Runs a shell command.
    Shell(String),
    /// Network access (web search/fetch).
    Web,
    /// Calls an MCP tool.
    Mcp { server_id: String, tool: String },
    /// Browser automation (eval needs approval, navigation doesn't).
    Browser { dangerous: bool },
    /// Spawns a subagent.
    Subagent,
    /// Asks the user / writes todos / memory / plans — always allowed.
    Benign,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Allow,
    NeedsApproval { kind: ApprovalKind, title: String, detail: String, allow_always_pattern: Option<String>, classification: Option<crate::sandbox::CommandClassification> },
    Deny(String),
}

/// Split a shell command into top-level segments on `&&`, `||`, `;`, `|`, newlines (quote-aware, best effort).
pub fn segments(cmd: &str) -> Vec<String> {
    let mut out = vec![]; let mut cur = String::new();
    let (mut sq, mut dq) = (false, false);
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\'' if !dq => { sq = !sq; cur.push(c); }
            '"' if !sq => { dq = !dq; cur.push(c); }
            '&' | '|' if !sq && !dq => { if i + 1 < chars.len() && chars[i + 1] == c { i += 1; } out.push(std::mem::take(&mut cur)); }
            ';' | '\n' if !sq && !dq => { out.push(std::mem::take(&mut cur)); }
            _ => cur.push(c),
        }
        i += 1;
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

fn strip_env_prefix(seg: &str) -> &str {
    // `FOO=bar cmd` → `cmd`
    let mut s = seg;
    loop {
        let word = s.split_whitespace().next().unwrap_or("");
        if word.contains('=') && !word.starts_with('-') && !word.contains('/') && word.chars().next().map(|c| c.is_ascii_alphabetic() || c == '_').unwrap_or(false) {
            s = s[word.len()..].trim_start();
            if s.is_empty() { return seg; }
        } else { return s; }
    }
}

fn prefix_matches(segment: &str, prefix: &str) -> bool {
    let seg = strip_env_prefix(segment);
    let p = prefix.trim();
    if p.is_empty() { return false; }
    seg == p || (seg.starts_with(p) && seg[p.len()..].starts_with(|c: char| c.is_whitespace()))
}

pub fn matches_denylist(cmd: &str, denylist: &[String]) -> Option<String> {
    let lower = cmd.to_lowercase();
    for d in denylist {
        let d = d.trim(); if d.is_empty() { continue; }
        let dl = d.to_lowercase();
        if dl.contains(' ') || dl.contains('/') || dl.contains('(') {
            if lower.contains(&dl) { return Some(d.to_string()); }
        } else {
            for seg in segments(cmd) { if prefix_matches(&seg.to_lowercase(), &dl) { return Some(d.to_string()); } }
        }
    }
    None
}

/// Every segment must start with an allowlisted prefix; command substitution forces approval.
pub fn allowlisted(cmd: &str, allowlist: &[String]) -> bool {
    if cmd.contains("$(") || cmd.contains('`') || cmd.contains("<(") || cmd.contains(">(") { return false; }
    let segs = segments(cmd);
    if segs.is_empty() { return false; }
    segs.iter().all(|seg| allowlist.iter().any(|a| prefix_matches(seg, a)))
}

/// First word of the first segment (used as the "allow always" pattern).
pub fn command_prefix(cmd: &str) -> String {
    let seg = segments(cmd).into_iter().next().unwrap_or_default();
    let seg = strip_env_prefix(&seg);
    let words: Vec<&str> = seg.split_whitespace().collect();
    match words.first() {
        Some(&w) if matches!(w, "git" | "npm" | "cargo" | "go" | "docker" | "kubectl" | "pnpm" | "yarn" | "bun" | "pip" | "python" | "python3" | "make" | "gh" | "brew") && words.len() > 1 && !words[1].starts_with('-') => format!("{} {}", w, words[1]),
        Some(&w) => w.to_string(),
        None => seg.to_string(),
    }
}

const PROTECTED_DOTFILES: &[&str] = &[".git", ".env", ".ssh", ".aws", ".gnupg", ".npmrc", ".pypirc", ".netrc", ".docker"];
const ALLOWED_DOT_DIRS: &[&str] = &[".caret", ".cursor", ".github", ".vscode", ".gitlab", ".circleci", ".husky", ".storybook", ".changeset"];

pub fn is_protected_dotfile(rel: &Path) -> bool {
    let comps: Vec<String> = rel.components().filter_map(|c| if let Component::Normal(s) = c { Some(s.to_string_lossy().to_string()) } else { None }).collect();
    for (i, c) in comps.iter().enumerate() {
        if !c.starts_with('.') || c == "." || c == ".." { continue; }
        if PROTECTED_DOTFILES.iter().any(|p| c == p || c.starts_with(&format!("{p}."))) { return true; }
        if c.starts_with(".env") { return true; }
        if ALLOWED_DOT_DIRS.contains(&c.as_str()) {
            // hooks/settings inside .caret/.cursor are sensitive
            if let Some(next) = comps.get(i + 1) { if next.starts_with("hooks") || next == "settings.json" || next == "mcp.json" { return true; } }
            continue;
        }
        return true; // other dotfiles (e.g. .zshrc, .bashrc)
    }
    false
}

fn inside(root: &Path, p: &Path) -> bool { crate::paths::normalize(p).starts_with(crate::paths::normalize(root)) }

pub struct PolicyInput<'a> { pub settings: &'a Settings, pub session: &'a Session, pub tool_name: &'a str, pub mode_auto_run: Option<bool> }

pub fn resolve_path(session: &Session, p: &str) -> PathBuf { crate::paths::resolve(Some(&session.cwd_path()), p) }

pub fn decide(input: &PolicyInput, effect: &ToolEffect) -> Decision {
    let s = input.settings;
    let auto_run = match input.mode_auto_run { Some(false) => "off", Some(true) => s.agent.auto_run.as_str(), None => s.agent.auto_run.as_str() };
    let session_allows = |pat: &str| input.session.session_allow.iter().any(|a| a == pat);
    match effect {
        ToolEffect::ReadOnly | ToolEffect::Benign => Decision::Allow,
        ToolEffect::Web => {
            if !s.privacy.allow_web_tools { return Decision::Deny("Web tools are disabled in Privacy settings".into()); }
            if !s.agent.web_search_enabled { return Decision::Deny("Web search is disabled in Agent settings".into()); }
            Decision::Allow
        }
        ToolEffect::Shell(cmd) => {
            if let Some(d) = matches_denylist(cmd, &s.agent.command_denylist) { return Decision::Deny(format!("Command blocked by denylist entry `{d}`")); }
            // Deterministic classification (shell AST → risk labels) runs below the model and below auto-run.
            let root = input.session.root();
            let cwd = input.session.cwd_path();
            let class = crate::sandbox::classify_with(cmd, Some(&cwd), root.as_deref(), &s.agent.command_allowlist, &s.agent.command_denylist);
            if class.blocked {
                let why = if class.reasons.is_empty() { class.labels.join(", ") } else { class.reasons.join("; ") };
                return Decision::Deny(format!("Command blocked by safety policy [{}]: {why}", class.labels.join(", ")));
            }
            let destructive = class.is_destructive();
            // Destructive commands are never auto-run; "allow for session/always" scopes to the exact command, not the prefix.
            let pattern = if destructive { cmd.trim().to_string() } else { command_prefix(cmd) };
            if session_allows(&format!("cmd:{pattern}")) { return Decision::Allow; }
            let ok = !destructive && match auto_run { "all" => true, "allowlist" => class.allowlisted, _ => false };
            if ok { Decision::Allow } else {
                let title = match class.overall_risk.as_str() { "destructive" => "Run destructive command", "caution" => "Run command (review)", _ => "Run command" };
                Decision::NeedsApproval { kind: ApprovalKind::Command, title: title.into(), detail: cmd.clone(), allow_always_pattern: Some(pattern), classification: Some(class) }
            }
        }
        ToolEffect::FileEdit(paths) | ToolEffect::FileDelete(paths) => {
            let is_delete = matches!(effect, ToolEffect::FileDelete(_));
            let root = input.session.root();
            let cwd = input.session.cwd_path();
            for p in paths {
                let abs = resolve_path(input.session, p);
                if s.agent.protect_outside_workspace {
                    let inside_ws = root.as_ref().map(|r| inside(r, &abs)).unwrap_or(true) || inside(&cwd, &abs);
                    if !inside_ws && !session_allows("outside-workspace") {
                        return Decision::NeedsApproval { kind: ApprovalKind::OutsideWorkspace, title: "Modify file outside workspace".into(), detail: abs.to_string_lossy().to_string(), allow_always_pattern: None, classification: None };
                    }
                }
                if s.agent.protect_dotfiles {
                    let rel = root.as_ref().and_then(|r| abs.strip_prefix(r).ok().map(Path::to_path_buf)).unwrap_or_else(|| abs.clone());
                    if is_protected_dotfile(&rel) && !session_allows("dotfiles") {
                        return Decision::NeedsApproval { kind: ApprovalKind::FileEdit, title: if is_delete { "Delete protected file" } else { "Edit protected file" }.into(), detail: p.clone(), allow_always_pattern: None, classification: None };
                    }
                }
            }
            if is_delete && s.agent.protect_delete && !session_allows("delete") {
                return Decision::NeedsApproval { kind: ApprovalKind::FileDelete, title: "Delete files".into(), detail: paths.join(", "), allow_always_pattern: None, classification: None };
            }
            if auto_run == "off" && !session_allows("edits") {
                return Decision::NeedsApproval { kind: ApprovalKind::FileEdit, title: if is_delete { "Delete files" } else { "Edit files" }.into(), detail: paths.join(", "), allow_always_pattern: None, classification: None };
            }
            Decision::Allow
        }
        ToolEffect::Mcp { server_id, tool } => {
            let q = format!("{server_id}:{tool}");
            // A pinned tool whose description/schema changed since it was first trusted always needs a fresh look.
            if s.sandbox.pin_mcp_tool_descriptions {
                if let Some(ch) = crate::sandbox::mcp_pins::pending_change(server_id, tool) {
                    let before = ch.previous_description.unwrap_or_default();
                    return Decision::NeedsApproval { kind: ApprovalKind::Mcp, title: format!("MCP tool changed: {tool}"), detail: format!("The description or schema of `{tool}` on server `{server_id}` changed since you last approved it (possible prompt injection).\n\nBefore:\n{}\n\nNow:\n{}", before.chars().take(600).collect::<String>(), ch.current_description.chars().take(600).collect::<String>()), allow_always_pattern: Some(q), classification: None };
                }
            }
            if s.agent.mcp_auto_run || s.agent.auto_run_mcp_tools.iter().any(|t| t == &q || t == tool) || session_allows(&format!("mcp:{q}")) { Decision::Allow } else {
                Decision::NeedsApproval { kind: ApprovalKind::Mcp, title: format!("Run MCP tool {tool}"), detail: format!("Server: {server_id}"), allow_always_pattern: Some(q), classification: None }
            }
        }
        ToolEffect::Browser { dangerous } => {
            if !s.beta.browser_tools { return Decision::Deny("Browser tools are disabled (Settings → Beta)".into()); }
            if *dangerous && auto_run != "all" && !session_allows("browser-eval") { Decision::NeedsApproval { kind: ApprovalKind::Browser, title: "Run JavaScript in browser".into(), detail: "The agent wants to execute a script in the browser page".into(), allow_always_pattern: None, classification: None } } else { Decision::Allow }
        }
        ToolEffect::Subagent => Decision::Allow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splits_segments() {
        assert_eq!(segments("ls -la && cat x | grep y; echo 'a;b'"), vec!["ls -la", "cat x", "grep y", "echo 'a;b'"]);
    }
    #[test]
    fn allowlist_requires_every_segment() {
        let allow = vec!["ls".to_string(), "git status".to_string(), "cat".to_string()];
        assert!(allowlisted("ls -la && cat foo", &allow));
        assert!(!allowlisted("ls && rm -rf x", &allow));
        assert!(!allowlisted("git push", &allow));
        assert!(allowlisted("git status --short", &allow));
        assert!(!allowlisted("ls $(rm x)", &allow));
        assert!(allowlisted("FOO=1 ls", &allow));
        assert!(!allowlisted("lsof", &allow));
    }
    #[test]
    fn denylist_detects() {
        let deny = vec!["sudo".into(), "rm -rf /".into(), "git push --force".into()];
        assert!(matches_denylist("sudo rm x", &deny).is_some());
        assert!(matches_denylist("echo hi && sudo ls", &deny).is_some());
        assert!(matches_denylist("rm -rf / --no-preserve-root", &deny).is_some());
        assert!(matches_denylist("git push --force origin main", &deny).is_some());
        assert!(matches_denylist("ls", &deny).is_none());
        assert!(matches_denylist("sudoku", &deny).is_none());
    }
    #[test]
    fn command_prefixes() {
        assert_eq!(command_prefix("git commit -m x"), "git commit");
        assert_eq!(command_prefix("npm test"), "npm test");
        assert_eq!(command_prefix("ls -la"), "ls");
        assert_eq!(command_prefix("FOO=1 cargo build"), "cargo build");
    }
    #[test]
    fn dotfile_protection() {
        assert!(is_protected_dotfile(Path::new(".env")));
        assert!(is_protected_dotfile(Path::new(".env.local")));
        assert!(is_protected_dotfile(Path::new(".git/config")));
        assert!(is_protected_dotfile(Path::new(".zshrc")));
        assert!(is_protected_dotfile(Path::new(".caret/hooks.json")));
        assert!(!is_protected_dotfile(Path::new(".caret/rules/foo.mdc")));
        assert!(!is_protected_dotfile(Path::new(".github/workflows/ci.yml")));
        assert!(!is_protected_dotfile(Path::new("src/main.rs")));
    }

    #[test]
    fn shell_policy_uses_classification() {
        let mut st = Settings::default();
        st.agent.auto_run = "all".into();
        let mut sess = Session::default();
        sess.workspace_root = "/work/repo".into(); sess.cwd = "/work/repo".into();
        let input = PolicyInput { settings: &st, session: &sess, tool_name: "run_terminal_cmd", mode_auto_run: None };
        // safe and caution commands follow auto-run "all"
        assert_eq!(decide(&input, &ToolEffect::Shell("npm test".into())), Decision::Allow);
        assert_eq!(decide(&input, &ToolEffect::Shell("npm install".into())), Decision::Allow);
        // destructive never auto-runs; the allow pattern is the exact command
        match decide(&input, &ToolEffect::Shell("rm -rf dist".into())) {
            Decision::NeedsApproval { allow_always_pattern, classification, title, kind, .. } => {
                assert_eq!(kind, ApprovalKind::Command);
                assert_eq!(allow_always_pattern.as_deref(), Some("rm -rf dist"));
                assert_eq!(classification.expect("classification").overall_risk, "destructive");
                assert!(title.contains("destructive"), "{title}");
            }
            d => panic!("unexpected {d:?}"),
        }
        // blocked commands are denied outright, including evasions
        for cmd in ["rm -rf /", "sh -c 'rm -rf ~'", "echo $(mkfs.ext4 /dev/sda1)", "sudo shutdown -h now"] {
            assert!(matches!(decide(&input, &ToolEffect::Shell(cmd.into())), Decision::Deny(_)), "{cmd}");
        }
        // session allow: a bare prefix does not bless a destructive command, the exact command does
        let mut sess2 = Session::default(); sess2.workspace_root = "/work/repo".into(); sess2.cwd = "/work/repo".into(); sess2.session_allow.push("cmd:rm".into());
        let input2 = PolicyInput { settings: &st, session: &sess2, tool_name: "run_terminal_cmd", mode_auto_run: None };
        assert!(matches!(decide(&input2, &ToolEffect::Shell("rm -rf dist".into())), Decision::NeedsApproval { .. }));
        assert_eq!(decide(&input2, &ToolEffect::Shell("rm file.txt".into())), Decision::Allow);
        let mut sess3 = Session::default(); sess3.workspace_root = "/work/repo".into(); sess3.cwd = "/work/repo".into(); sess3.session_allow.push("cmd:rm -rf dist".into());
        let input3 = PolicyInput { settings: &st, session: &sess3, tool_name: "run_terminal_cmd", mode_auto_run: None };
        assert_eq!(decide(&input3, &ToolEffect::Shell("rm -rf dist".into())), Decision::Allow);
        // allowlist mode uses the classifier's allowlist verdict (every segment must match)
        let mut st2 = Settings::default(); st2.agent.auto_run = "allowlist".into(); st2.agent.command_allowlist = vec!["npm".into(), "ls".into()];
        let input4 = PolicyInput { settings: &st2, session: &sess, tool_name: "run_terminal_cmd", mode_auto_run: None };
        assert_eq!(decide(&input4, &ToolEffect::Shell("npm test && ls".into())), Decision::Allow);
        assert!(matches!(decide(&input4, &ToolEffect::Shell("cat x".into())), Decision::NeedsApproval { .. }));
        assert!(matches!(decide(&input4, &ToolEffect::Shell("npm test && rm -rf dist".into())), Decision::NeedsApproval { .. }));
        // auto-run off: even safe commands ask, and the card carries the classification
        let input5 = PolicyInput { settings: &st, session: &sess, tool_name: "run_terminal_cmd", mode_auto_run: Some(false) };
        match decide(&input5, &ToolEffect::Shell("ls".into())) { Decision::NeedsApproval { classification, allow_always_pattern, .. } => { assert_eq!(classification.unwrap().overall_risk, "safe"); assert_eq!(allow_always_pattern.as_deref(), Some("ls")); } d => panic!("{d:?}") }
    }
}

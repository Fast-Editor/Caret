//! Built-in agent catalog, user-defined agents, and `npm install` into `~/.caret/acp`.
use crate::acp::{process, AcpAgentConfig, AcpInner, AcpRegistryEntry};
use crate::core::settings::Settings;
use crate::error::{CaretError, Result};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct Builtin { pub id: &'static str, pub name: &'static str, pub command: &'static str, pub args: &'static [&'static str], pub description: &'static str, pub npm: Option<&'static str>, pub install_hint: &'static str, pub website: &'static str, pub icon: &'static str,
    /// Vendor CLI that signs the user in (interactive; Caret runs it in a terminal). Empty = no CLI login.
    pub login: &'static [&'static str],
    /// Vendor CLI that prints login state without side effects. Empty = fall back to `auth_file`.
    pub status: &'static [&'static str],
    /// Credentials file whose presence means "signed in" when there is no status command (`~` allowed).
    pub auth_file: Option<&'static str>,
    /// Env var the CLI reads for an alternate config home → lets users add a second account.
    pub account_env: Option<&'static str> }

/// Confirmed launch commands (2026-10): adapters publish a bin of the same name; Gemini CLI has a native flag.
pub const BUILTINS: &[Builtin] = &[
    Builtin { id: "claude-code", name: "Claude Code", command: "claude-agent-acp", args: &[], description: "Anthropic's Claude Code via the official ACP adapter. Uses your Claude Code login or ANTHROPIC_API_KEY.", npm: Some("@agentclientprotocol/claude-agent-acp"), install_hint: "npm install -g @agentclientprotocol/claude-agent-acp", website: "https://github.com/agentclientprotocol/claude-agent-acp", icon: "anthropic", login: &["claude", "auth", "login"], status: &["claude", "auth", "status"], auth_file: None, account_env: Some("CLAUDE_CONFIG_DIR") },
    Builtin { id: "codex", name: "Codex", command: "codex-acp", args: &[], description: "OpenAI Codex CLI via the ACP adapter (bundles the codex binary). Uses your ChatGPT login or OPENAI_API_KEY.", npm: Some("@agentclientprotocol/codex-acp"), install_hint: "npm install -g @agentclientprotocol/codex-acp", website: "https://github.com/agentclientprotocol/codex-acp", icon: "openai", login: &["codex", "login"], status: &["codex", "login", "status"], auth_file: Some("~/.codex/auth.json"), account_env: Some("CODEX_HOME") },
    Builtin { id: "gemini", name: "Gemini CLI", command: "gemini", args: &["--experimental-acp"], description: "Google Gemini CLI in ACP mode (native support).", npm: Some("@google/gemini-cli"), install_hint: "npm install -g @google/gemini-cli", website: "https://github.com/google-gemini/gemini-cli", icon: "google", login: &["gemini"], status: &[], auth_file: Some("~/.gemini/oauth_creds.json"), account_env: None },
    Builtin { id: "opencode", name: "OpenCode", command: "opencode", args: &["acp"], description: "OpenCode (open-source coding agent) in native ACP mode (`opencode acp`). Uses the providers and logins you configured in OpenCode (`opencode auth login`).", npm: Some("opencode-ai"), install_hint: "brew install anomalyco/tap/opencode  (or: npm install -g opencode-ai, or: curl -fsSL https://opencode.ai/install | bash)", website: "https://opencode.ai/docs/acp", icon: "opencode", login: &["opencode", "auth", "login"], status: &["opencode", "auth", "list"], auth_file: None, account_env: None },
    Builtin { id: "pi", name: "Pi", command: "pi-acp", args: &[], description: "Pi coding agent via its ACP adapter.", npm: Some("pi-acp"), install_hint: "npm install -g pi-acp", website: "https://www.npmjs.com/package/pi-acp", icon: "bot", login: &[], status: &[], auth_file: None, account_env: None },
    Builtin { id: "glm", name: "GLM (Z.AI)", command: "glm-acp-agent", args: &[], description: "Zhipu GLM Coding Plan models as an ACP agent. Needs ZAI_API_KEY.", npm: Some("glm-acp-agent"), install_hint: "npm install -g glm-acp-agent", website: "https://www.npmjs.com/package/glm-acp-agent", icon: "bot", login: &[], status: &[], auth_file: None, account_env: None },
    Builtin { id: "cursor", name: "Cursor", command: "cursor-agent", args: &["acp"], description: "Cursor's agent through the official Cursor CLI in ACP mode (`cursor-agent acp`). Uses your Cursor login (paid plan required by Cursor).", npm: None, install_hint: "curl https://cursor.com/install -fsS | bash", website: "https://cursor.com/cli", icon: "bot", login: &["cursor-agent", "login"], status: &["cursor-agent", "status"], auth_file: None, account_env: None },
    Builtin { id: "grok", name: "Grok Build", command: "grok", args: &["agent", "stdio"], description: "xAI's Grok Build CLI in native ACP mode (`grok agent stdio`). Uses your Grok login (Sign in with Grok, or `grok login`) or XAI_API_KEY.", npm: None, install_hint: "curl -fsSL https://x.ai/cli/install.sh | sh", website: "https://x.ai/cli", icon: "bot", login: &["grok", "login"], status: &[], auth_file: Some("~/.grok/auth.json"), account_env: Some("GROK_HOME") },
];

pub fn builtin_config(b: &Builtin) -> AcpAgentConfig {
    AcpAgentConfig { id: b.id.into(), name: b.name.into(), command: b.command.into(), args: b.args.iter().map(|s| s.to_string()).collect(), env: None, based_on: None, enabled: true, source: "builtin".into(), description: Some(b.description.into()), icon: Some(b.icon.into()), install_hint: Some(b.install_hint.into()), website: Some(b.website.into()) }
}

/// Builtins merged with user agents from settings. A user entry with a builtin id overrides it (enables/disables,
/// changes command/env); other user entries are appended.
pub fn all_configs(settings: &Settings) -> Vec<AcpAgentConfig> {
    let mut out: Vec<AcpAgentConfig> = BUILTINS.iter().map(builtin_config).collect();
    for u in &settings.acp.agents {
        if let Some(b) = out.iter_mut().find(|c| c.id == u.id) {
            b.enabled = u.enabled;
            if !u.command.is_empty() { b.command = u.command.clone(); b.args = u.args.clone(); }
            if u.env.is_some() { b.env = u.env.clone(); }
            if u.name != b.name && !u.name.is_empty() { b.name = u.name.clone(); }
        } else {
            let mut c = u.clone(); if c.source.is_empty() { c.source = "user".into(); } out.push(c);
        }
    }
    out
}

pub fn find_config(settings: &Settings, agent_id: &str) -> Option<AcpAgentConfig> { all_configs(settings).into_iter().find(|c| c.id == agent_id) }

pub fn registry(settings: &Settings) -> Vec<AcpRegistryEntry> {
    let cfgs = all_configs(settings);
    BUILTINS.iter().map(|b| {
        let installed = cfgs.iter().find(|c| c.id == b.id).map(process::is_installed).unwrap_or(false);
        AcpRegistryEntry { id: b.id.into(), name: b.name.into(), description: Some(b.description.into()), install: b.npm.map(|p| json!({ "type": "npm", "package": p, "prefix": process::acp_home().to_string_lossy() })), website: Some(b.website.into()), installed, login_command: vec_opt(b.login), status_command: vec_opt(b.status), account_env: b.account_env.map(String::from) }
    }).collect()
}

fn vec_opt(v: &[&str]) -> Option<Vec<String>> { if v.is_empty() { None } else { Some(v.iter().map(|s| s.to_string()).collect()) } }

pub fn builtin(id: &str) -> Option<&'static Builtin> { BUILTINS.iter().find(|b| b.id == id) }

/// The builtin an agent config derives from: itself, or `based_on` for extra accounts.
pub fn base_builtin(cfg: &AcpAgentConfig) -> Option<&'static Builtin> { cfg.based_on.as_deref().and_then(builtin).or_else(|| builtin(&cfg.id)) }

fn shell_quote(s: &str) -> String { if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,~".contains(c)) { s.to_string() } else { format!("'{}'", s.replace('\'', "'\\''")) } }

/// Shell line that signs the user in for `agent_id`, with the account's env (e.g. `CODEX_HOME=~/.codex-work codex login`).
/// Interactive CLIs need a TTY, so the UI runs this in a Caret terminal.
pub fn login_shell_command(settings: &Settings, agent_id: &str) -> Option<String> {
    let cfg = find_config(settings, agent_id)?;
    let b = base_builtin(&cfg)?;
    if b.login.is_empty() { return None; }
    let mut parts: Vec<String> = cfg.env.as_ref().map(|e| e.iter().map(|(k, v)| format!("{k}={}", shell_quote(v))).collect()).unwrap_or_default();
    parts.extend(b.login.iter().map(|s| shell_quote(s)));
    Some(parts.join(" "))
}

/// Interpret a vendor CLI's status output. Pure, so it is unit-tested against captured outputs.
pub fn parse_account_status(builtin_id: &str, stdout: &str, stderr: &str, success: bool) -> (Option<bool>, String) {
    let out = if stdout.trim().is_empty() { stderr } else { stdout };
    let first = out.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
    match builtin_id {
        "codex" => {
            let l = out.to_ascii_lowercase();
            if l.contains("not logged in") { (Some(false), "Not signed in".into()) }
            else if l.contains("logged in") { (Some(true), if first.is_empty() { "Signed in".into() } else { first }) }
            else { (None, first) }
        }
        "claude-code" => match serde_json::from_str::<serde_json::Value>(out.trim()) {
            Ok(v) => {
                let logged = v.get("loggedIn").and_then(|x| x.as_bool());
                let method = v.get("authMethod").and_then(|x| x.as_str()).unwrap_or("");
                let provider = v.get("apiProvider").and_then(|x| x.as_str()).unwrap_or("");
                let email = v.get("email").and_then(|x| x.as_str()).unwrap_or("");
                let mut d = if logged == Some(true) { "Signed in".to_string() } else { "Not signed in".to_string() };
                let mut bits = vec![]; if !email.is_empty() { bits.push(email.to_string()); } if !method.is_empty() { bits.push(method.replace('_', " ")); } if !provider.is_empty() { bits.push(format!("via {provider}")); }
                if !bits.is_empty() { d.push_str(&format!(" ({})", bits.join(", "))); }
                (logged, d)
            }
            Err(_) => { let l = out.to_ascii_lowercase(); if l.contains("not logged in") || l.contains("not authenticated") { (Some(false), "Not signed in".into()) } else { (None, first) } }
        },
        "cursor" => {
            let l = out.to_ascii_lowercase();
            if l.contains("not logged in") || l.contains("not authenticated") || l.contains("login required") { (Some(false), "Not signed in".into()) }
            else if let Some(i) = l.find("logged in as") { let who = out[i + "logged in as".len()..].lines().next().unwrap_or("").trim(); (Some(true), format!("Signed in as {who}")) }
            else if l.contains("logged in") { (Some(true), "Signed in".into()) }
            else { (None, first) }
        }
        "opencode" => {
            let n = out.lines().filter(|l| !l.trim().is_empty() && !l.trim().to_ascii_lowercase().starts_with("no ")).count();
            if n == 0 { (Some(false), "No providers configured (opencode auth login)".into()) } else { (Some(true), format!("{n} provider{} configured", if n == 1 { "" } else { "s" })) }
        }
        _ => (if success { None } else { Some(false) }, first),
    }
}

fn expand_home(p: &str) -> std::path::PathBuf { if let Some(rest) = p.strip_prefix("~/") { crate::paths::home_dir().join(rest) } else { std::path::PathBuf::from(p) } }

/// Ask the vendor CLI (or its credentials file) whether `agent_id` is signed in. Never fails for CLI errors.
pub async fn account_status(settings: &Settings, agent_id: &str) -> crate::acp::AcpAccountStatus {
    use crate::acp::AcpAccountStatus;
    let st = |logged_in, detail: String, checked_with: String| AcpAccountStatus { agent_id: agent_id.into(), logged_in, detail, checked_with };
    let Some(cfg) = find_config(settings, agent_id) else { return st(None, "unknown agent".into(), String::new()) };
    let Some(b) = base_builtin(&cfg) else { return st(None, "Custom agent: sign in with its own tooling".into(), String::new()) };
    if !b.status.is_empty() {
        let Some(exe) = process::resolve_command(b.status[0]) else { return st(None, format!("`{}` is not installed", b.status[0]), b.status.join(" ")) };
        let mut cmd = tokio::process::Command::new(exe);
        cmd.args(&b.status[1..]).env("PATH", process::augmented_path()).env("NO_COLOR", "1").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
        if let Some(env) = &cfg.env { for (k, v) in env { cmd.env(k, v); } }
        return match tokio::time::timeout(std::time::Duration::from_secs(20), cmd.output()).await {
            Ok(Ok(out)) => { let (l, d) = parse_account_status(b.id, &String::from_utf8_lossy(&out.stdout), &String::from_utf8_lossy(&out.stderr), out.status.success()); st(l, d, b.status.join(" ")) }
            Ok(Err(e)) => st(None, format!("could not run `{}`: {e}", b.status.join(" ")), b.status.join(" ")),
            Err(_) => st(None, format!("`{}` timed out", b.status.join(" ")), b.status.join(" ")),
        };
    }
    if let Some(f) = b.auth_file {
        // An extra account points its CLI at another config home; look for the same file there.
        let alt_home = b.account_env.and_then(|k| cfg.env.as_ref().and_then(|e| e.get(k)).cloned());
        let path = match alt_home { Some(home) => expand_home(&home).join(std::path::Path::new(f).file_name().map(|x| x.to_os_string()).unwrap_or_default()), None => expand_home(f) };
        let exists = path.is_file();
        return st(Some(exists), if exists { format!("Credentials found ({})", path.display()) } else { "Not signed in".into() }, format!("file {}", path.display()));
    }
    st(None, "This agent manages its own login".into(), String::new())
}


/// `Codex (Work)` → `work`, `Work` → `work`, `Codex` → `account`: the label without the builtin's own name.
pub fn account_slug(builtin_name: &str, display_name: &str) -> String {
    let mut label = display_name.trim().to_string();
    if let (Some(i), Some(j)) = (label.rfind('('), label.rfind(')')) { if j > i { label = label[i + 1..j].to_string(); } }
    let lower_base = builtin_name.to_lowercase();
    let mut l = label.to_lowercase(); if l.starts_with(&lower_base) { l = l[lower_base.len()..].to_string(); }
    let mut out = String::new(); let mut dash = false;
    for c in l.chars() { if c.is_ascii_alphanumeric() { out.push(c); dash = false; } else if !dash && !out.is_empty() { out.push('-'); dash = true; } }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "account".into() } else { out }
}

/// Add another account of a builtin agent: same command, its own config home through the builtin's `account_env`.
pub fn add_account(st: &crate::core::settings::SettingsState, base_id: &str, name: &str, env: BTreeMap<String, String>) -> Result<Vec<AcpAgentConfig>> {
    let b = builtin(base_id).ok_or_else(|| CaretError::not_found(format!("builtin agent {base_id}")))?;
    let settings = st.effective();
    let id = format!("{}-{}", base_id, account_slug(b.name, name));
    if settings.acp.agents.iter().any(|a| a.id == id) || BUILTINS.iter().any(|x| x.id == id) { return Err(CaretError::invalid(format!("an agent with id `{id}` already exists"))); }
    let mut cfg = builtin_config(b);
    cfg.id = id; cfg.name = name.trim().to_string(); cfg.source = "user".into(); cfg.based_on = Some(base_id.into());
    cfg.env = if env.is_empty() { None } else { Some(env) };
    let mut agents = settings.acp.agents.clone();
    agents.push(cfg);
    save_user_agents(st, agents.clone())?;
    Ok(agents)
}

/// Persist the user agent list: `settings.acp.agents` (user scope).
pub fn save_user_agents(st: &crate::core::settings::SettingsState, agents: Vec<AcpAgentConfig>) -> Result<()> {
    st.patch(&json!({ "acp": { "agents": agents } }), "user")?;
    Ok(())
}

/// Remember a session option (model, effort, …) the user picked for `agent_id`; applied to its future sessions.
pub fn remember_session_default(st: &crate::core::settings::SettingsState, agent_id: &str, config_id: &str, value: serde_json::Value) -> Result<()> {
    let mut defaults = st.effective().acp.session_defaults.clone();
    defaults.entry(agent_id.to_string()).or_default().insert(config_id.to_string(), value);
    st.patch(&json!({ "acp": { "sessionDefaults": defaults } }), "user")?;
    Ok(())
}

pub fn upsert_user_agent(st: &crate::core::settings::SettingsState, mut cfg: AcpAgentConfig) -> Result<Vec<AcpAgentConfig>> {
    if cfg.id.trim().is_empty() { cfg.id = slug(&cfg.name); }
    if cfg.id.is_empty() || cfg.id == "caret" { return Err(CaretError::invalid("agent id is required and cannot be 'caret'")); }
    if cfg.command.trim().is_empty() && !BUILTINS.iter().any(|b| b.id == cfg.id) { return Err(CaretError::invalid("command is required")); }
    if cfg.source.is_empty() || cfg.source == "builtin" { cfg.source = if BUILTINS.iter().any(|b| b.id == cfg.id) { "builtin".into() } else { "user".into() }; }
    let mut agents = st.effective().acp.agents.clone();
    if let Some(e) = agents.iter_mut().find(|a| a.id == cfg.id) { *e = cfg; } else { agents.push(cfg); }
    save_user_agents(st, agents.clone())?;
    Ok(agents)
}

pub fn remove_user_agent(st: &crate::core::settings::SettingsState, agent_id: &str) -> Result<()> {
    let mut agents = st.effective().acp.agents.clone();
    let before = agents.len();
    agents.retain(|a| a.id != agent_id);
    if agents.len() == before && !BUILTINS.iter().any(|b| b.id == agent_id) { return Err(CaretError::not_found(format!("agent {agent_id}"))); }
    save_user_agents(st, agents)
}

pub fn set_enabled(st: &crate::core::settings::SettingsState, agent_id: &str, enabled: bool) -> Result<()> {
    let settings = st.effective();
    let cur = find_config(&settings, agent_id).ok_or_else(|| CaretError::not_found(format!("agent {agent_id}")))?;
    let mut agents = settings.acp.agents.clone();
    if let Some(e) = agents.iter_mut().find(|a| a.id == agent_id) { e.enabled = enabled; }
    else { let mut c = cur; c.enabled = enabled; if c.source == "builtin" { /* store a thin override */ c.command = String::new(); c.args = vec![]; c.env = None; } agents.push(c); }
    save_user_agents(st, agents)
}

fn slug(s: &str) -> String { s.trim().to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>().trim_matches('-').to_string() }

fn find_npm() -> Option<std::path::PathBuf> { process::resolve_command("npm") }

/// `npm install <pkg>@latest --prefix ~/.caret/acp` — streams npm output to the agent log. Returns the resolved bin.
pub async fn install(inner: Arc<AcpInner>, settings: &Settings, entry_id: &str) -> Result<AcpRegistryEntry> {
    let b = BUILTINS.iter().find(|b| b.id == entry_id).ok_or_else(|| CaretError::not_found(format!("registry entry {entry_id}")))?;
    let pkg = b.npm.ok_or_else(|| CaretError::invalid(format!("{} has no npm installer; {}", b.name, b.install_hint)))?;
    let npm = find_npm().ok_or_else(|| CaretError::not_found("npm (install Node.js ≥ 18 from https://nodejs.org and retry)"))?;
    let home = process::acp_home();
    if !home.join("package.json").exists() { let _ = std::fs::write(home.join("package.json"), "{\n  \"name\": \"caret-acp-agents\",\n  \"private\": true\n}\n"); }
    inner.log(entry_id, format!("[install] {} install {pkg}@latest --prefix {}", npm.display(), home.display()));
    let mut cmd = tokio::process::Command::new(&npm);
    cmd.args(["install", "--no-fund", "--no-audit", "--loglevel", "error", &format!("{pkg}@latest")]).current_dir(&home).env("PATH", process::augmented_path()).env("npm_config_prefix", &home).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    let out = tokio::time::timeout(std::time::Duration::from_secs(600), cmd.output()).await.map_err(|_| CaretError::io("npm install timed out after 10 minutes"))?.map_err(|e| CaretError::io(format!("npm install failed to start: {e}")))?;
    for l in String::from_utf8_lossy(&out.stdout).lines().chain(String::from_utf8_lossy(&out.stderr).lines()) { if !l.trim().is_empty() { inner.log(entry_id, format!("[install] {l}")); } }
    if !out.status.success() { return Err(CaretError::io(format!("npm install {pkg} failed (exit {}): {}", out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stderr).chars().take(800).collect::<String>()))); }
    let entry = registry(settings).into_iter().find(|e| e.id == entry_id).unwrap();
    inner.log(entry_id, format!("[install] done; installed={}", entry.installed));
    Ok(entry)
}

#[allow(dead_code)]
pub fn env_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> { pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn user_override_disables_builtin_and_adds_custom() {
        let mut s = Settings::default();
        s.acp.agents.push(AcpAgentConfig { id: "codex".into(), name: "".into(), command: "".into(), args: vec![], env: None, enabled: false, source: "builtin".into(), description: None, icon: None, install_hint: None, website: None, based_on: None });
        s.acp.agents.push(AcpAgentConfig { id: "mine".into(), name: "Mine".into(), command: "/usr/bin/true".into(), args: vec!["acp".into()], env: None, enabled: true, source: "".into(), description: None, icon: None, install_hint: None, website: None, based_on: None });
        let all = all_configs(&s);
        let codex = all.iter().find(|c| c.id == "codex").unwrap();
        assert!(!codex.enabled); assert_eq!(codex.command, "codex-acp"); assert_eq!(codex.name, "Codex");
        let mine = all.iter().find(|c| c.id == "mine").unwrap();
        assert_eq!(mine.source, "user"); assert_eq!(mine.args, vec!["acp"]);
        assert_eq!(all.len(), BUILTINS.len() + 1);
    }
    #[test]
    fn registry_marks_install_state() { let r = registry(&Settings::default()); assert_eq!(r.len(), BUILTINS.len()); assert!(r.iter().zip(BUILTINS.iter()).all(|(e, b)| e.install.is_some() == b.npm.is_some())); assert!(r.iter().find(|e| e.id == "cursor").unwrap().login_command.is_some()); }
    #[test]
    fn slug_is_safe() { assert_eq!(slug("My Agent!"), "my-agent"); }
}

#[cfg(test)]
mod account_status_tests {
    use super::parse_account_status as p;
    #[test]
    fn codex_status_lines() {
        assert_eq!(p("codex", "Not logged in\n", "", true), (Some(false), "Not signed in".into()));
        assert_eq!(p("codex", "Logged in using ChatGPT\n", "", true).0, Some(true));
    }
    #[test]
    fn claude_status_json() {
        let (l, d) = p("claude-code", r#"{"loggedIn": true, "authMethod": "third_party", "apiProvider": "foundry"}"#, "", true);
        assert_eq!(l, Some(true)); assert!(d.contains("third party") && d.contains("via foundry"), "{d}");
        assert_eq!(p("claude-code", r#"{"loggedIn": false}"#, "", true).0, Some(false));
    }
    #[test]
    fn cursor_and_opencode() {
        let (l, d) = p("cursor", "✓ Logged in as someone@example.com\n", "", true);
        assert_eq!(l, Some(true)); assert_eq!(d, "Signed in as someone@example.com");
        assert_eq!(p("cursor", "Not logged in. Run `agent login`.", "", false).0, Some(false));
        assert_eq!(p("opencode", "Google  GEMINI_API_KEY  environment\nLynkr  LYNKR_API_KEY  environment\n", "", true), (Some(true), "2 providers configured".into()));
        assert_eq!(p("opencode", "", "", true).0, Some(false));
    }
    #[test]
    fn account_slugs() {
        assert_eq!(super::account_slug("Codex", "Codex (Work)"), "work");
        assert_eq!(super::account_slug("Codex", "Work"), "work");
        assert_eq!(super::account_slug("Codex", "Codex (My Team!)"), "my-team");
        assert_eq!(super::account_slug("Codex", "Codex"), "account");
    }
    #[test]
    fn builtins_have_auth_metadata_and_new_agents() {
        let ids: Vec<&str> = super::BUILTINS.iter().map(|b| b.id).collect();
        assert!(ids.contains(&"cursor") && ids.contains(&"grok"));
        assert_eq!(super::builtin("codex").unwrap().account_env, Some("CODEX_HOME"));
        assert_eq!(super::builtin("cursor").unwrap().login, &["cursor-agent", "login"]);
        assert_eq!(super::shell_quote("~/.codex-work"), "~/.codex-work");
        assert_eq!(super::shell_quote("a b"), "'a b'");
    }
}

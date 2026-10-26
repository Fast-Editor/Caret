//! Built-in agent catalog, user-defined agents, and `npm install` into `~/.caret/acp`.
use crate::acp::{process, AcpAgentConfig, AcpInner, AcpRegistryEntry};
use crate::core::settings::Settings;
use crate::error::{CaretError, Result};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct Builtin { pub id: &'static str, pub name: &'static str, pub command: &'static str, pub args: &'static [&'static str], pub description: &'static str, pub npm: Option<&'static str>, pub install_hint: &'static str, pub website: &'static str, pub icon: &'static str }

/// Confirmed launch commands (2026-10): adapters publish a bin of the same name; Gemini CLI has a native flag.
pub const BUILTINS: &[Builtin] = &[
    Builtin { id: "claude-code", name: "Claude Code", command: "claude-agent-acp", args: &[], description: "Anthropic's Claude Code via the official ACP adapter. Uses your Claude Code login or ANTHROPIC_API_KEY.", npm: Some("@agentclientprotocol/claude-agent-acp"), install_hint: "npm install -g @agentclientprotocol/claude-agent-acp", website: "https://github.com/agentclientprotocol/claude-agent-acp", icon: "anthropic" },
    Builtin { id: "codex", name: "Codex", command: "codex-acp", args: &[], description: "OpenAI Codex CLI via the ACP adapter (bundles the codex binary). Uses your ChatGPT login or OPENAI_API_KEY.", npm: Some("@agentclientprotocol/codex-acp"), install_hint: "npm install -g @agentclientprotocol/codex-acp", website: "https://github.com/agentclientprotocol/codex-acp", icon: "openai" },
    Builtin { id: "gemini", name: "Gemini CLI", command: "gemini", args: &["--experimental-acp"], description: "Google Gemini CLI in ACP mode (native support).", npm: Some("@google/gemini-cli"), install_hint: "npm install -g @google/gemini-cli", website: "https://github.com/google-gemini/gemini-cli", icon: "google" },
    Builtin { id: "opencode", name: "OpenCode", command: "opencode", args: &["acp"], description: "OpenCode (open-source coding agent) in native ACP mode (`opencode acp`). Uses the providers and logins you configured in OpenCode (`opencode auth login`).", npm: Some("opencode-ai"), install_hint: "brew install anomalyco/tap/opencode  (or: npm install -g opencode-ai, or: curl -fsSL https://opencode.ai/install | bash)", website: "https://opencode.ai/docs/acp", icon: "opencode" },
    Builtin { id: "pi", name: "Pi", command: "pi-acp", args: &[], description: "Pi coding agent via its ACP adapter.", npm: Some("pi-acp"), install_hint: "npm install -g pi-acp", website: "https://www.npmjs.com/package/pi-acp", icon: "bot" },
    Builtin { id: "glm", name: "GLM (Z.AI)", command: "glm-acp-agent", args: &[], description: "Zhipu GLM Coding Plan models as an ACP agent. Needs ZAI_API_KEY.", npm: Some("glm-acp-agent"), install_hint: "npm install -g glm-acp-agent", website: "https://www.npmjs.com/package/glm-acp-agent", icon: "bot" },
];

pub fn builtin_config(b: &Builtin) -> AcpAgentConfig {
    AcpAgentConfig { id: b.id.into(), name: b.name.into(), command: b.command.into(), args: b.args.iter().map(|s| s.to_string()).collect(), env: None, enabled: true, source: "builtin".into(), description: Some(b.description.into()), icon: Some(b.icon.into()), install_hint: Some(b.install_hint.into()), website: Some(b.website.into()) }
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
        AcpRegistryEntry { id: b.id.into(), name: b.name.into(), description: Some(b.description.into()), install: b.npm.map(|p| json!({ "type": "npm", "package": p, "prefix": process::acp_home().to_string_lossy() })), website: Some(b.website.into()), installed }
    }).collect()
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
        s.acp.agents.push(AcpAgentConfig { id: "codex".into(), name: "".into(), command: "".into(), args: vec![], env: None, enabled: false, source: "builtin".into(), description: None, icon: None, install_hint: None, website: None });
        s.acp.agents.push(AcpAgentConfig { id: "mine".into(), name: "Mine".into(), command: "/usr/bin/true".into(), args: vec!["acp".into()], env: None, enabled: true, source: "".into(), description: None, icon: None, install_hint: None, website: None });
        let all = all_configs(&s);
        let codex = all.iter().find(|c| c.id == "codex").unwrap();
        assert!(!codex.enabled); assert_eq!(codex.command, "codex-acp"); assert_eq!(codex.name, "Codex");
        let mine = all.iter().find(|c| c.id == "mine").unwrap();
        assert_eq!(mine.source, "user"); assert_eq!(mine.args, vec!["acp"]);
        assert_eq!(all.len(), BUILTINS.len() + 1);
    }
    #[test]
    fn registry_marks_install_state() { let r = registry(&Settings::default()); assert_eq!(r.len(), BUILTINS.len()); assert!(r.iter().all(|e| e.install.is_some())); }
    #[test]
    fn slug_is_safe() { assert_eq!(slug("My Agent!"), "my-agent"); }
}

//! Lifecycle hooks from `<root>/.caret/hooks.json`, `<root>/.cursor/hooks.json` (compat) and `~/.caret/hooks.json`.
//! Each hook is a shell command receiving the JSON payload on stdin and answering with JSON on stdout:
//! `{ "permission": "allow"|"deny"|"ask", "user_message"?, "agent_message"?, "continue"?, "followup_prompt"?, ... }`.
use crate::paths;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HookEvent { BeforeSubmitPrompt, BeforeShellExecution, BeforeMcpExecution, BeforeReadFile, AfterFileEdit, AfterAgentResponse, AfterAgentThought, Stop, SessionStart, SessionEnd }

impl HookEvent {
    pub fn name(&self) -> &'static str {
        match self { Self::BeforeSubmitPrompt => "beforeSubmitPrompt", Self::BeforeShellExecution => "beforeShellExecution", Self::BeforeMcpExecution => "beforeMCPExecution", Self::BeforeReadFile => "beforeReadFile", Self::AfterFileEdit => "afterFileEdit", Self::AfterAgentResponse => "afterAgentResponse", Self::AfterAgentThought => "afterAgentThought", Self::Stop => "stop", Self::SessionStart => "sessionStart", Self::SessionEnd => "sessionEnd" }
    }
    /// Events whose hooks can block the action.
    pub fn is_gate(&self) -> bool { matches!(self, Self::BeforeSubmitPrompt | Self::BeforeShellExecution | Self::BeforeMcpExecution | Self::BeforeReadFile) }
    fn aliases(&self) -> &'static [&'static str] {
        match self { Self::BeforeMcpExecution => &["beforeMCPExecution", "beforeMcpExecution"], Self::Stop => &["stop", "onStop"], e => { let _ = e; &[] } }
    }
    pub fn matches_key(&self, key: &str) -> bool { key == self.name() || self.aliases().contains(&key) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookPermission { Allow, Deny, Ask }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookOutcome {
    pub permission: HookPermission,
    #[serde(default)] pub user_message: Option<String>,
    #[serde(default)] pub agent_message: Option<String>,
    #[serde(default)] pub followup_prompt: Option<String>,
    #[serde(default)] pub replaced_input: Option<Value>,
    pub ran: usize,
}
impl Default for HookOutcome { fn default() -> Self { Self { permission: HookPermission::Allow, user_message: None, agent_message: None, followup_prompt: None, replaced_input: None, ran: 0 } } }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookInfo { pub event: String, pub command: String, pub source: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub timeout_ms: Option<u64>, #[serde(default, skip_serializing_if = "Option::is_none")] pub matcher: Option<String> }

#[derive(Debug, Clone)]
pub struct HookDef { pub event_key: String, pub command: String, pub timeout_ms: u64, pub matcher: Option<String>, pub source: PathBuf, pub cwd: PathBuf }

const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const MAX_TIMEOUT_MS: u64 = 120_000;

pub fn config_files(root: Option<&Path>) -> Vec<PathBuf> { config_files_with(root, &paths::config_dir()) }
pub fn config_files_with(root: Option<&Path>, user_config_dir: &Path) -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(r) = root { v.push(r.join(".caret").join("hooks.json")); v.push(r.join(".cursor").join("hooks.json")); }
    v.push(user_config_dir.join("hooks.json"));
    v
}

pub fn load_defs(root: Option<&Path>) -> Vec<HookDef> { load_defs_from(&config_files(root), root) }
pub fn load_defs_from(files: &[PathBuf], root: Option<&Path>) -> Vec<HookDef> {
    let mut out = vec![];
    for f in files {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { tracing::warn!(file=%f.display(), "hooks.json is not valid JSON"); continue };
        let Some(hooks) = v.get("hooks").and_then(|h| h.as_object()) else { continue };
        let cwd = root.map(Path::to_path_buf).or_else(|| f.parent().and_then(|p| p.parent()).map(Path::to_path_buf)).unwrap_or_else(paths::home_dir);
        for (event_key, arr) in hooks {
            let Some(list) = arr.as_array() else { continue };
            for h in list {
                let Some(command) = h.get("command").and_then(|c| c.as_str()).filter(|c| !c.trim().is_empty()) else { continue };
                let timeout_ms = h.get("timeout").and_then(|t| t.as_f64()).map(|t| if t > 1000.0 { t as u64 } else { (t * 1000.0) as u64 }).or_else(|| h.get("timeoutMs").and_then(|t| t.as_u64())).unwrap_or(DEFAULT_TIMEOUT_MS).clamp(100, MAX_TIMEOUT_MS);
                let matcher = h.get("matcher").and_then(|m| m.as_str()).map(String::from).filter(|m| !m.is_empty());
                out.push(HookDef { event_key: event_key.clone(), command: command.to_string(), timeout_ms, matcher, source: f.clone(), cwd: cwd.clone() });
            }
        }
    }
    out
}

pub fn list(root: Option<&Path>) -> Vec<HookInfo> {
    load_defs(root).into_iter().map(|d| HookInfo { event: d.event_key, command: d.command, source: d.source.to_string_lossy().to_string(), timeout_ms: Some(d.timeout_ms), matcher: d.matcher }).collect()
}

fn matcher_subject(payload: &Value) -> String {
    for k in ["command", "tool_name", "toolName", "tool", "server_name", "serverName", "file_path", "filePath", "path"] {
        if let Some(s) = payload.get(k).and_then(|v| v.as_str()) { return s.to_string(); }
    }
    payload.to_string()
}

fn shell() -> String { std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into()) }

fn extract_json(stdout: &str) -> Option<Value> {
    let t = stdout.trim();
    if t.is_empty() { return None; }
    if let Ok(v) = serde_json::from_str::<Value>(t) { if v.is_object() { return Some(v); } }
    for line in t.lines().rev() { let l = line.trim(); if l.starts_with('{') { if let Ok(v) = serde_json::from_str::<Value>(l) { return Some(v); } } }
    if let (Some(s), Some(e)) = (t.find('{'), t.rfind('}')) { if e > s { if let Ok(v) = serde_json::from_str::<Value>(&t[s..=e]) { return Some(v); } } }
    None
}

async fn run_one(def: &HookDef, event: HookEvent, payload: &Value) -> (HookPermission, Option<String>, Option<String>, Option<String>, Option<Value>) {
    let mut child = match tokio::process::Command::new(shell()).arg("-lc").arg(&def.command).current_dir(&def.cwd)
        .env("CARET_HOOK_EVENT", event.name()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).spawn() {
        Ok(c) => c,
        Err(e) => { tracing::warn!(cmd=%def.command, error=%e, "hook failed to spawn"); return (HookPermission::Allow, Some(format!("Hook `{}` failed to start: {e}", def.command)), None, None, None); }
    };
    if let Some(mut stdin) = child.stdin.take() { let data = serde_json::to_vec(payload).unwrap_or_default(); let _ = stdin.write_all(&data).await; let _ = stdin.shutdown().await; }
    let out = match tokio::time::timeout(std::time::Duration::from_millis(def.timeout_ms), child.wait_with_output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => return (HookPermission::Allow, Some(format!("Hook `{}` error: {e}", def.command)), None, None, None),
        Err(_) => { tracing::warn!(cmd=%def.command, "hook timed out"); return (HookPermission::Allow, Some(format!("Hook `{}` timed out after {} ms (ignored)", def.command, def.timeout_ms)), None, None, None); }
    };
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        if event.is_gate() { return (HookPermission::Deny, Some(if stderr.trim().is_empty() { format!("Blocked by hook `{}` (exit {})", def.command, out.status.code().unwrap_or(-1)) } else { stderr.trim().to_string() }), None, None, None); }
        tracing::debug!(cmd=%def.command, code=?out.status.code(), "non-gating hook exited non-zero");
        return (HookPermission::Allow, None, None, None, None);
    }
    let Some(json) = extract_json(&stdout) else { return (HookPermission::Allow, None, None, None, None) };
    let perm = match json.get("permission").and_then(|p| p.as_str()).map(|s| s.to_ascii_lowercase()).as_deref() {
        Some("deny") | Some("block") => HookPermission::Deny,
        Some("ask") => HookPermission::Ask,
        _ => if json.get("continue").and_then(|c| c.as_bool()) == Some(false) && event.is_gate() { HookPermission::Deny } else { HookPermission::Allow },
    };
    let user_message = json.get("user_message").or_else(|| json.get("userMessage")).or_else(|| json.get("reason")).and_then(|v| v.as_str()).map(String::from);
    let agent_message = json.get("agent_message").or_else(|| json.get("agentMessage")).and_then(|v| v.as_str()).map(String::from);
    let followup = json.get("followup_prompt").or_else(|| json.get("followup_message")).or_else(|| json.get("followupPrompt")).and_then(|v| v.as_str()).map(String::from);
    let known = ["permission", "user_message", "userMessage", "reason", "agent_message", "agentMessage", "continue", "followup_prompt", "followup_message", "followupPrompt"];
    let replaced = if event == HookEvent::BeforeSubmitPrompt { json.as_object().map(|o| o.iter().filter(|(k, _)| !known.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect::<serde_json::Map<_, _>>()).filter(|m| !m.is_empty()).map(Value::Object) } else { None };
    (perm, user_message, agent_message, followup, replaced)
}

/// Run every matching hook for `event`. Payload gets `hook_event_name`, `workspace_root` and (if present) `session_id` added.
pub async fn run(root: Option<&Path>, event: HookEvent, payload: Value) -> HookOutcome { run_defs(&load_defs(root), root, event, payload).await }
pub async fn run_defs(defs: &[HookDef], root: Option<&Path>, event: HookEvent, payload: Value) -> HookOutcome {
    let defs: Vec<&HookDef> = defs.iter().filter(|d| event.matches_key(&d.event_key)).collect();
    let mut outcome = HookOutcome::default();
    if defs.is_empty() { return outcome; }
    let mut payload = payload;
    if let Value::Object(m) = &mut payload {
        m.insert("hook_event_name".into(), json!(event.name()));
        m.insert("workspace_root".into(), json!(root.map(|r| r.to_string_lossy().to_string())));
    } else { payload = json!({ "hook_event_name": event.name(), "workspace_root": root.map(|r| r.to_string_lossy().to_string()), "input": payload }); }
    let subject = matcher_subject(&payload);
    let mut user_msgs = vec![]; let mut agent_msgs = vec![];
    for d in defs {
        if let Some(m) = &d.matcher { match regex::Regex::new(m) { Ok(re) => { if !re.is_match(&subject) { continue; } } Err(_) => { if !subject.contains(m.as_str()) { continue; } } } }
        let (perm, um, am, fu, rep) = run_one(d, event, &payload).await;
        outcome.ran += 1;
        match (outcome.permission, perm) { (_, HookPermission::Deny) => outcome.permission = HookPermission::Deny, (HookPermission::Allow, HookPermission::Ask) => outcome.permission = HookPermission::Ask, _ => {} }
        if let Some(m) = um { user_msgs.push(m); }
        if let Some(m) = am { agent_msgs.push(m); }
        if fu.is_some() && outcome.followup_prompt.is_none() { outcome.followup_prompt = fu; }
        if let Some(r) = rep { match &mut outcome.replaced_input { Some(Value::Object(cur)) => { if let Value::Object(n) = r { cur.extend(n); } } _ => outcome.replaced_input = Some(r) } }
    }
    if !user_msgs.is_empty() { outcome.user_message = Some(user_msgs.join("\n")); }
    if !agent_msgs.is_empty() { outcome.agent_message = Some(agent_msgs.join("\n")); }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write(p: &Path, s: &str) { std::fs::create_dir_all(p.parent().unwrap()).unwrap(); std::fs::write(p, s).unwrap(); }

    #[tokio::test]
    async fn deny_ask_allow_merge_and_matchers() {
        let tmp = tempfile::tempdir().unwrap(); let root = tmp.path().join("ws"); std::fs::create_dir_all(&root).unwrap();
        write(&root.join(".caret/hooks.json"), r#"{"version":1,"hooks":{
            "beforeShellExecution":[
              {"command":"cat > /dev/null; echo '{\"permission\":\"deny\",\"user_message\":\"no rm\"}'","matcher":"rm -rf"},
              {"command":"echo '{\"permission\":\"ask\",\"agent_message\":\"careful\"}'"}
            ],
            "afterFileEdit":[{"command":"exit 3"}],
            "beforeSubmitPrompt":[{"command":"cat | python3 -c 'import sys,json; d=json.load(sys.stdin); print(json.dumps({\"permission\":\"allow\",\"prompt\": d[\"prompt\"] + \" (audited)\", \"seen\": d[\"hook_event_name\"]}))'"}]
        }}"#);
        write(&root.join(".cursor/hooks.json"), r#"{"version":1,"hooks":{"beforeShellExecution":[{"command":"echo not json"}]}}"#);
        let defs = load_defs_from(&config_files_with(Some(&root), &tmp.path().join("nouser")), Some(&root));
        assert_eq!(defs.len(), 5);
        let o = run_defs(&defs, Some(&root), HookEvent::BeforeShellExecution, json!({"command":"rm -rf build"})).await;
        assert_eq!(o.permission, HookPermission::Deny); assert_eq!(o.ran, 3);
        assert!(o.user_message.as_deref().unwrap().contains("no rm")); assert_eq!(o.agent_message.as_deref(), Some("careful"));
        let o = run_defs(&defs, Some(&root), HookEvent::BeforeShellExecution, json!({"command":"ls"})).await;
        assert_eq!(o.permission, HookPermission::Ask); assert_eq!(o.ran, 2);
        let o = run_defs(&defs, Some(&root), HookEvent::AfterFileEdit, json!({"file_path":"a.rs"})).await;
        assert_eq!(o.permission, HookPermission::Allow, "non-gating hook exit codes are ignored");
        let o = run_defs(&defs, Some(&root), HookEvent::BeforeSubmitPrompt, json!({"prompt":"hi"})).await;
        assert_eq!(o.permission, HookPermission::Allow);
        let rep = o.replaced_input.unwrap();
        assert_eq!(rep["prompt"], "hi (audited)"); assert_eq!(rep["seen"], "beforeSubmitPrompt");
        let o = run_defs(&defs, Some(&root), HookEvent::Stop, json!({})).await;
        assert_eq!(o.ran, 0);
        assert!(list(Some(&root)).len() >= 4);
    }

    #[tokio::test]
    async fn gate_hook_nonzero_exit_denies_and_timeout_allows() {
        let tmp = tempfile::tempdir().unwrap(); let root = tmp.path().join("ws"); std::fs::create_dir_all(&root).unwrap();
        write(&root.join(".caret/hooks.json"), r#"{"version":1,"hooks":{"beforeReadFile":[{"command":"echo secret-file >&2; exit 1"}],"beforeMCPExecution":[{"command":"sleep 5","timeout":0.3}]}}"#);
        let defs = load_defs_from(&config_files_with(Some(&root), &tmp.path().join("nouser")), Some(&root));
        let o = run_defs(&defs, Some(&root), HookEvent::BeforeReadFile, json!({"file_path":".env"})).await;
        assert_eq!(o.permission, HookPermission::Deny); assert_eq!(o.user_message.as_deref(), Some("secret-file"));
        let t0 = std::time::Instant::now();
        let o = run_defs(&defs, Some(&root), HookEvent::BeforeMcpExecution, json!({"tool_name":"x"})).await;
        assert_eq!(o.permission, HookPermission::Allow); assert!(o.user_message.unwrap().contains("timed out")); assert!(t0.elapsed() < std::time::Duration::from_secs(3));
    }
}

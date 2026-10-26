//! Agent tools: trait, context, registry, mode filtering and provider tool definitions.
pub mod edit;
pub mod fs_tools;
pub mod search_tools;
pub mod shell_tools;
pub mod web_tools;
pub mod meta_tools;
pub mod dynamic;

use crate::ai::agent::manager::AgentManager;
use crate::ai::agent::modes::{ModeSpec, ToolFilter};
use crate::ai::agent::policy::ToolEffect;
use crate::ai::agent::session::{FileChangeSummary, ToolCatalogEntry};
use crate::ai::llm::ToolDef;
use crate::core::settings::Settings;
use crate::error::Result;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::AppHandle;
use tokio_util::sync::CancellationToken;

pub struct ToolContext {
    /// None when running headless (tests).
    pub app: Option<AppHandle>,
    pub manager: Arc<AgentManager>,
    pub session_id: String,
    pub message_id: String,
    pub tool_call_id: String,
    pub turn_id: String,
    pub root: Option<PathBuf>,
    pub cwd: PathBuf,
    pub settings: Arc<Settings>,
    pub cancel: CancellationToken,
    pub mode: Arc<ModeSpec>,
}
impl ToolContext {
    /// Resolve a model-supplied path against the session cwd (absolute paths pass through).
    pub fn resolve(&self, p: &str) -> PathBuf { crate::paths::resolve(Some(&self.cwd), p.trim()) }
    pub fn display(&self, p: &std::path::Path) -> String {
        if let Some(r) = &self.root { if let Ok(rel) = p.strip_prefix(r) { return rel.to_string_lossy().to_string(); } }
        if let Ok(rel) = p.strip_prefix(&self.cwd) { return rel.to_string_lossy().to_string(); }
        p.to_string_lossy().to_string()
    }
    pub fn max_output(&self) -> usize { (self.settings.agent.max_tool_output_chars as usize).max(2000) }
}

#[derive(Debug, Default, Clone)]
pub struct ToolOutput {
    pub text: String,
    /// (media_type, base64)
    pub images: Vec<(String, String)>,
    pub file_changes: Vec<FileChangeSummary>,
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub is_error: bool,
    pub summary: Option<String>,
    pub subagent_session_id: Option<String>,
    pub diff: Option<String>,
    pub is_background: bool,
    /// The command ran under the OS sandbox.
    pub sandboxed: bool,
}
impl ToolOutput {
    pub fn text(t: impl Into<String>) -> Self { Self { text: t.into(), ..Default::default() } }
    pub fn error(t: impl Into<String>) -> Self { Self { text: t.into(), is_error: true, ..Default::default() } }
    pub fn with_summary(mut self, s: impl Into<String>) -> Self { self.summary = Some(s.into()); self }
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn display_name(&self) -> &str;
    fn description(&self) -> String;
    fn input_schema(&self) -> Value;
    fn readonly(&self) -> bool;
    fn group(&self) -> &str;
    fn effect(&self, input: &Value) -> ToolEffect;
    /// Short human label for the UI card (e.g. the path or command).
    fn label(&self, input: &Value) -> Option<String> { input.get("path").and_then(|v| v.as_str()).map(String::from) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput>;
}

pub fn schema(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}
pub fn s_str(desc: &str) -> Value { json!({ "type": "string", "description": desc }) }
pub fn s_int(desc: &str) -> Value { json!({ "type": "integer", "description": desc }) }
pub fn s_bool(desc: &str) -> Value { json!({ "type": "boolean", "description": desc }) }
pub fn s_arr(desc: &str, items: Value) -> Value { json!({ "type": "array", "description": desc, "items": items }) }
pub fn arg_str<'a>(input: &'a Value, keys: &[&str]) -> Option<&'a str> { keys.iter().find_map(|k| input.get(*k).and_then(|v| v.as_str())) }
pub fn arg_u64(input: &Value, keys: &[&str]) -> Option<u64> { keys.iter().find_map(|k| input.get(*k).and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f.max(0.0) as u64)).or_else(|| v.as_str().and_then(|s| s.parse().ok())))) }
pub fn arg_bool(input: &Value, keys: &[&str]) -> Option<bool> { keys.iter().find_map(|k| input.get(*k).and_then(|v| v.as_bool().or_else(|| v.as_str().map(|s| s == "true")))) }

/// Truncate tool output keeping head + tail.
pub fn cap_output(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars { return (text.to_string(), false); }
    let chars: Vec<char> = text.chars().collect();
    let head = max_chars * 7 / 10; let tail = max_chars - head;
    let mut s: String = chars[..head].iter().collect();
    s.push_str(&format!("\n\n... [{} characters truncated — read the file or narrow the query for more] ...\n\n", chars.len() - max_chars));
    s.extend(chars[chars.len() - tail..].iter());
    (s, true)
}

pub struct ToolRegistry { tools: Vec<Arc<dyn Tool>> }
impl ToolRegistry {
    pub fn builtin() -> Self {
        let mut tools: Vec<Arc<dyn Tool>> = vec![];
        tools.extend(fs_tools::tools());
        tools.extend(search_tools::tools());
        tools.extend(shell_tools::tools());
        tools.extend(web_tools::tools());
        tools.extend(meta_tools::tools());
        Self { tools }
    }
    pub fn all(&self) -> &[Arc<dyn Tool>] { &self.tools }
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> { self.tools.iter().find(|t| t.name() == name).cloned() }
    pub fn catalog(&self) -> Vec<ToolCatalogEntry> {
        self.tools.iter().map(|t| ToolCatalogEntry { name: t.name().into(), display_name: t.display_name().into(), description: t.description(), group: t.group().into(), readonly: t.readonly() }).collect()
    }
}

/// Build the active tool set for a turn: builtin (filtered by mode & settings) + MCP + browser tools.
pub async fn active_tools(registry: &ToolRegistry, mode: &ModeSpec, settings: &Settings, is_subagent: bool) -> Vec<Arc<dyn Tool>> {
    let mut out: Vec<Arc<dyn Tool>> = vec![];
    let allowed = |name: &str, readonly: bool| match &mode.tools { ToolFilter::All => true, ToolFilter::Readonly => readonly, ToolFilter::Only { names } => names.iter().any(|n| n == name) };
    for t in registry.all() {
        if !allowed(t.name(), t.readonly()) { continue; }
        if is_subagent && t.name() == "task" { continue; }
        if matches!(t.name(), "web_search" | "fetch_url") && (!settings.privacy.allow_web_tools || !settings.agent.web_search_enabled && t.name() == "web_search") { continue; }
        out.push(t.clone());
    }
    if settings.mcp.enabled && !matches!(mode.tools, ToolFilter::Readonly) {
        for t in dynamic::mcp_tools().await { if allowed(t.name(), false) || matches!(mode.tools, ToolFilter::All) { out.push(t); } }
    }
    if settings.beta.browser_tools && matches!(mode.tools, ToolFilter::All) { out.extend(dynamic::browser_tools()); }
    out
}

pub fn to_defs(tools: &[Arc<dyn Tool>]) -> Vec<ToolDef> {
    tools.iter().map(|t| ToolDef { name: t.name().to_string(), description: t.description(), input_schema: t.input_schema(), strict: false }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtin_schemas_valid_and_unique() {
        let reg = ToolRegistry::builtin();
        let mut names = std::collections::HashSet::new();
        for t in reg.all() {
            assert!(names.insert(t.name().to_string()), "duplicate tool {}", t.name());
            assert!(t.name().chars().all(|c| c.is_ascii_alphanumeric() || c == '_'), "bad name {}", t.name());
            let s = t.input_schema();
            assert_eq!(s["type"], "object", "{} schema must be object", t.name());
            assert!(s.get("properties").is_some());
            assert!(!t.description().is_empty());
        }
        for ro in ["read_file", "list_dir", "grep_search", "file_search", "codebase_search", "read_lints", "fetch_rules", "skill", "search_docs"] { assert!(reg.get(ro).unwrap().readonly(), "{ro} should be readonly"); }
        for rw in ["edit_file", "write_file", "delete_file", "run_terminal_cmd"] { assert!(!reg.get(rw).unwrap().readonly(), "{rw} should not be readonly"); }
    }
    #[test]
    fn cap_keeps_head_and_tail() {
        let (s, t) = cap_output(&"a".repeat(100), 50);
        assert!(t); assert!(s.starts_with("aaaa")); assert!(s.ends_with("aaaa")); assert!(s.contains("truncated"));
    }
}

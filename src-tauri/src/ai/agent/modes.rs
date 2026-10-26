//! Agent modes (Agent / Ask / Plan / Debug + custom). Shared by the runner and the prompt builder.
use crate::core::settings::{CustomMode, Settings};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolFilter { All, Readonly, Only { names: Vec<String> } }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeSpec {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)] pub icon: Option<String>,
    /// Extra system-prompt instructions for this mode.
    pub instructions: String,
    pub tools: ToolFilter,
    pub builtin: bool,
    #[serde(default)] pub auto_run: Option<bool>,
    #[serde(default)] pub model_id: Option<String>,
}

pub const AGENT_INSTRUCTIONS: &str = "You are in Agent mode: you may read, search, edit files and run commands to complete the task end to end. Prefer making the change over describing it. Verify your work (run tests/linters when available) and keep edits minimal and idiomatic.";
pub const ASK_INSTRUCTIONS: &str = "You are in Ask mode: answer questions and explain code. You may read and search the codebase but must NOT modify files or run commands that change state. When proposing code, show it in fenced blocks with the target path so the user can apply it.";
pub const PLAN_INSTRUCTIONS: &str = "You are in Plan mode: investigate the codebase (read/search only), ask clarifying questions when requirements are ambiguous, then write a concrete implementation plan with the `create_plan` tool: goals, steps with target files, risks, and verification. Do not edit project files; the user will approve the plan and build it.";
pub const DEBUG_INSTRUCTIONS: &str = "You are in Debug mode: form hypotheses about the bug, add targeted logging/instrumentation, run the code or tests to gather evidence, analyze outputs, iterate until the root cause is confirmed, then fix it and remove temporary instrumentation.";

pub fn builtin_modes() -> Vec<ModeSpec> {
    vec![
        ModeSpec { id: "agent".into(), name: "Agent".into(), description: "Autonomous coding with tools".into(), icon: Some("bot".into()), instructions: AGENT_INSTRUCTIONS.into(), tools: ToolFilter::All, builtin: true, auto_run: None, model_id: None },
        ModeSpec { id: "ask".into(), name: "Ask".into(), description: "Read-only Q&A and explanations".into(), icon: Some("message-circle".into()), instructions: ASK_INSTRUCTIONS.into(), tools: ToolFilter::Readonly, builtin: true, auto_run: None, model_id: None },
        ModeSpec { id: "plan".into(), name: "Plan".into(), description: "Research then write a plan to approve".into(), icon: Some("list-checks".into()), instructions: PLAN_INSTRUCTIONS.into(), tools: ToolFilter::Only { names: vec!["read_file".into(), "list_dir".into(), "grep_search".into(), "file_search".into(), "codebase_search".into(), "fetch_rules".into(), "web_search".into(), "fetch_url".into(), "read_lints".into(), "ask_question".into(), "create_plan".into(), "skill".into(), "search_docs".into()] }, builtin: true, auto_run: None, model_id: None },
        ModeSpec { id: "debug".into(), name: "Debug".into(), description: "Hypothesis-driven debugging with instrumentation".into(), icon: Some("bug".into()), instructions: DEBUG_INSTRUCTIONS.into(), tools: ToolFilter::All, builtin: true, auto_run: None, model_id: None },
    ]
}

pub fn all_modes(settings: &Settings) -> Vec<ModeSpec> {
    let mut v = builtin_modes();
    for c in &settings.custom_modes { v.push(from_custom(c)); }
    v
}

pub fn from_custom(c: &CustomMode) -> ModeSpec {
    let tools = match &c.tools {
        serde_json::Value::String(s) if s == "readonly" => ToolFilter::Readonly,
        serde_json::Value::Array(a) => ToolFilter::Only { names: a.iter().filter_map(|v| v.as_str().map(String::from)).collect() },
        _ => ToolFilter::All,
    };
    ModeSpec { id: c.id.clone(), name: c.name.clone(), description: c.description.clone().unwrap_or_default(), icon: c.icon.clone(), instructions: c.instructions.clone(), tools, builtin: false, auto_run: c.auto_run, model_id: c.model_id.clone() }
}

pub fn resolve(settings: &Settings, id: &str) -> ModeSpec {
    all_modes(settings).into_iter().find(|m| m.id == id).unwrap_or_else(|| builtin_modes().remove(0))
}

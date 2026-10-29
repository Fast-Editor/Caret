//! Tauri commands for rules / memories / skills / slash commands / subagents / hooks / plans.
use crate::ai::agent::{commands as slash, hooks, memories, plans, rules, skills, subagents};
use crate::core::settings::SettingsState;
use crate::core::workspace::WorkspaceState;
use crate::error::{CaretError, Result};
use crate::events;
use std::path::PathBuf;
use tauri::{AppHandle, State};

fn root(ws: &WorkspaceState) -> Option<PathBuf> { ws.root() }

// ───────── rules ─────────
#[tauri::command(async)]
pub fn rules_list(ws: State<'_, WorkspaceState>, st: State<'_, SettingsState>) -> Result<Vec<rules::RuleInfo>> { Ok(rules::list(root(&ws).as_deref(), &st.effective())) }

#[tauri::command(async)]
pub fn rules_read(ws: State<'_, WorkspaceState>, st: State<'_, SettingsState>, rule_id: String) -> Result<String> {
    let r = root(&ws);
    let file = rules::resolve_path(r.as_deref(), &st.effective(), &rule_id).ok_or_else(|| CaretError::not_found(format!("Rule {rule_id}")))?;
    Ok(std::fs::read_to_string(file)?)
}

#[tauri::command]
pub fn rules_create(app: AppHandle, ws: State<'_, WorkspaceState>, st: State<'_, SettingsState>, name: String, kind: String, globs: Option<Vec<String>>, description: Option<String>, content: String, scope: String) -> Result<rules::RuleInfo> {
    let r = root(&ws);
    let file = rules::create(r.as_deref(), &name, &kind, &globs.unwrap_or_default(), description.as_deref(), &content, &scope)?;
    let settings = st.effective();
    let created = rules::discover(r.as_deref(), &settings).into_iter().find(|d| d.file == file).map(|d| d.info).ok_or_else(|| CaretError::internal("rule created but not discovered"))?;
    events::emit(&app, events::RULES_CHANGED, ());
    Ok(created)
}

#[tauri::command]
pub fn rules_toggle(app: AppHandle, rule_id: String, enabled: bool) -> Result<()> { rules::set_enabled(&rule_id, enabled)?; events::emit(&app, events::RULES_CHANGED, ()); Ok(()) }

#[tauri::command]
pub fn rules_delete(app: AppHandle, ws: State<'_, WorkspaceState>, st: State<'_, SettingsState>, rule_id: String) -> Result<()> {
    rules::delete(root(&ws).as_deref(), &st.effective(), &rule_id)?;
    events::emit(&app, events::RULES_CHANGED, ());
    Ok(())
}

// ───────── memories ─────────
#[tauri::command(async)]
pub fn memories_list(ws: State<'_, WorkspaceState>) -> Result<Vec<memories::MemoryItem>> { Ok(memories::list(root(&ws).as_deref())) }
#[tauri::command]
pub fn memories_add(ws: State<'_, WorkspaceState>, text: String, title: Option<String>) -> Result<memories::MemoryItem> { memories::add(root(&ws).as_deref(), &text, title.as_deref(), "user", None) }
#[tauri::command]
pub fn memories_update(ws: State<'_, WorkspaceState>, memory_id: String, text: String) -> Result<()> { memories::update(root(&ws).as_deref(), &memory_id, &text) }
#[tauri::command]
pub fn memories_delete(ws: State<'_, WorkspaceState>, memory_id: String) -> Result<()> { memories::delete(root(&ws).as_deref(), &memory_id) }
#[tauri::command]
pub fn memories_toggle(ws: State<'_, WorkspaceState>, memory_id: String, enabled: bool) -> Result<()> { memories::toggle(root(&ws).as_deref(), &memory_id, enabled) }

// ───────── skills / commands / subagents / hooks ─────────
#[tauri::command(async)]
pub fn skills_list(ws: State<'_, WorkspaceState>) -> Result<Vec<skills::SkillInfo>> { Ok(skills::list(root(&ws).as_deref())) }
#[tauri::command(async)]
pub fn skills_read(ws: State<'_, WorkspaceState>, skill_id: String) -> Result<String> { skills::read(root(&ws).as_deref(), &skill_id) }
#[tauri::command(async)]
pub fn commands_list(ws: State<'_, WorkspaceState>) -> Result<Vec<slash::SlashCommandInfo>> { Ok(slash::list(root(&ws).as_deref())) }
#[tauri::command(async)]
pub fn subagents_list(ws: State<'_, WorkspaceState>) -> Result<Vec<subagents::SubagentInfo>> { Ok(subagents::list(root(&ws).as_deref())) }
#[tauri::command(async)]
pub fn hooks_list(ws: State<'_, WorkspaceState>) -> Result<Vec<hooks::HookInfo>> { Ok(hooks::list(root(&ws).as_deref())) }

// ───────── plans ─────────
#[tauri::command(async)]
pub fn plans_list(ws: State<'_, WorkspaceState>) -> Result<Vec<plans::PlanInfo>> { Ok(plans::list(root(&ws).as_deref())) }
#[tauri::command(async)]
pub fn plans_read(path: String) -> Result<String> { plans::read(&path) }
#[tauri::command]
pub fn plans_set_status(path: String, status: String) -> Result<()> { plans::set_status(&path, &status) }
#[tauri::command]
pub fn plans_update(path: String, content: String) -> Result<()> { plans::update(&path, &content) }

/// Start an agent session that executes the plan. Delegates to the agent core (`chat_commands::start_plan_build`).
#[tauri::command]
pub async fn plans_build(app: AppHandle, path: String, session_id: Option<String>, model_id: Option<String>, use_worktree: Option<bool>) -> Result<String> {
    if plans::info(&path).is_none() { return Err(CaretError::not_found(format!("Plan {path}"))); }
    plans::set_status(&path, "building")?;
    start_plan_build(app, path, session_id, model_id, use_worktree.unwrap_or(false)).await
}

async fn start_plan_build(app: AppHandle, plan_path: String, session_id: Option<String>, model_id: Option<String>, use_worktree: bool) -> Result<String> {
    crate::ai::agent::chat_commands::start_plan_build(&app, &plan_path, session_id, model_id, use_worktree).await
}

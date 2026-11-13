//! MCP commands. Every command acts as seen from the calling window: its folder's servers plus the user's (just the
//! user's for a window without a folder). Workspace-scope edits (`project`) need a folder.
use crate::core::ports::McpCallResult;
use crate::core::workspace::WorkspaceState;
use crate::error::{CaretError, Result};
use crate::mcp::config::{self, McpServerConfig};
use crate::mcp::manager::{McpManager, McpPromptInfo, McpPromptMessage, McpResourceInfo, McpServerStatus, McpToolInfo};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::State;

fn root_of(ws: &WorkspaceState, window: &tauri::Window) -> Option<PathBuf> { ws.root_for(window.label()) }
/// The root a config edit of `scope` writes to: the window's folder for workspace scopes (an error without one).
fn root_for_scope(ws: &WorkspaceState, window: &tauri::Window, scope: &str) -> Result<Option<PathBuf>> {
    if config::is_workspace_scope(scope) { ws.require_root_for(window.label()).map(Some) } else { Ok(None) }
}

#[tauri::command(async)]
pub fn mcp_list_servers(m: State<'_, McpManager>, ws: State<'_, WorkspaceState>, window: tauri::Window) -> Result<Vec<McpServerStatus>> { Ok(m.inner.statuses(root_of(&ws, &window).as_deref())) }
#[tauri::command(async)]
pub fn mcp_list_configs(m: State<'_, McpManager>, ws: State<'_, WorkspaceState>, window: tauri::Window) -> Result<Vec<McpServerConfig>> { Ok(m.inner.configs(root_of(&ws, &window).as_deref())) }
#[tauri::command]
pub async fn mcp_add_server(m: State<'_, McpManager>, ws: State<'_, WorkspaceState>, window: tauri::Window, config: McpServerConfig, scope: String) -> Result<()> {
    if config.name.trim().is_empty() { return Err(CaretError::invalid("server name is required")); }
    let root = root_for_scope(&ws, &window, &scope)?;
    config::save_server(&scope, root.as_deref(), &config)?;
    m.inner.reload_scope(&scope).await; Ok(())
}
#[tauri::command]
pub async fn mcp_update_server(m: State<'_, McpManager>, ws: State<'_, WorkspaceState>, window: tauri::Window, config: McpServerConfig) -> Result<()> {
    let scope = config::scope_of(&config.id).to_string();
    let root = root_for_scope(&ws, &window, &scope)?;
    config::save_server(&scope, root.as_deref(), &config)?;
    m.inner.reload_scope(&scope).await; Ok(())
}
#[tauri::command]
pub async fn mcp_remove_server(m: State<'_, McpManager>, ws: State<'_, WorkspaceState>, window: tauri::Window, server_id: String) -> Result<()> {
    let scope = config::scope_of(&server_id).to_string();
    let root = root_for_scope(&ws, &window, &scope)?;
    config::remove_server(&server_id, root.as_deref())?;
    m.inner.reload_scope(&scope).await; Ok(())
}
#[tauri::command]
pub async fn mcp_toggle_server(m: State<'_, McpManager>, server_id: String, enabled: bool) -> Result<()> { config::set_enabled(&server_id, enabled)?; m.inner.reload_scope(config::scope_of(&server_id)).await; Ok(()) }
#[tauri::command]
pub async fn mcp_restart_server(m: State<'_, McpManager>, ws: State<'_, WorkspaceState>, window: tauri::Window, server_id: String) -> Result<()> { m.inner.restart(root_of(&ws, &window).as_deref(), &server_id).await }
#[tauri::command(async)]
pub fn mcp_tools(m: State<'_, McpManager>, ws: State<'_, WorkspaceState>, window: tauri::Window) -> Result<Vec<McpToolInfo>> { Ok(m.inner.tools(root_of(&ws, &window).as_deref())) }
#[tauri::command]
pub async fn mcp_toggle_tool(m: State<'_, McpManager>, server_id: String, tool: String, enabled: bool) -> Result<()> { config::set_tool_enabled(&server_id, &tool, enabled)?; m.inner.reload_scope(config::scope_of(&server_id)).await; Ok(()) }
#[tauri::command(async)]
pub fn mcp_resources(m: State<'_, McpManager>, ws: State<'_, WorkspaceState>, window: tauri::Window, server_id: Option<String>) -> Result<Vec<McpResourceInfo>> { Ok(m.inner.resources(root_of(&ws, &window).as_deref(), server_id.as_deref())) }
#[tauri::command]
pub async fn mcp_read_resource(m: State<'_, McpManager>, ws: State<'_, WorkspaceState>, window: tauri::Window, server_id: String, uri: String) -> Result<McpCallResult> { m.inner.read_resource(root_of(&ws, &window).as_deref(), &server_id, &uri).await }
#[tauri::command(async)]
pub fn mcp_prompts(m: State<'_, McpManager>, ws: State<'_, WorkspaceState>, window: tauri::Window) -> Result<Vec<McpPromptInfo>> { Ok(m.inner.prompts(root_of(&ws, &window).as_deref())) }
#[tauri::command]
pub async fn mcp_get_prompt(m: State<'_, McpManager>, ws: State<'_, WorkspaceState>, window: tauri::Window, server_id: String, name: String, args: Option<HashMap<String, String>>) -> Result<Vec<McpPromptMessage>> { m.inner.get_prompt(root_of(&ws, &window).as_deref(), &server_id, &name, args.unwrap_or_default()).await }
#[tauri::command]
pub async fn mcp_call_tool(m: State<'_, McpManager>, ws: State<'_, WorkspaceState>, window: tauri::Window, server_id: String, name: String, args: Option<serde_json::Value>) -> Result<McpCallResult> { m.inner.call_tool(root_of(&ws, &window).as_deref(), &server_id, &name, args.unwrap_or(serde_json::Value::Object(Default::default()))).await }
#[tauri::command(async)]
pub fn mcp_logs(m: State<'_, McpManager>, ws: State<'_, WorkspaceState>, window: tauri::Window, server_id: String) -> Result<Vec<String>> { Ok(m.inner.logs(root_of(&ws, &window).as_deref(), &server_id)) }

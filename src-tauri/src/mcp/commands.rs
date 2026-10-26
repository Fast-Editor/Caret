use crate::error::{CaretError, Result};
use crate::mcp::config::{self, McpServerConfig};
use crate::mcp::manager::{McpManager, McpPromptInfo, McpPromptMessage, McpResourceInfo, McpServerStatus, McpToolInfo};
use crate::core::ports::McpCallResult;
use std::collections::HashMap;
use tauri::State;

fn root(m: &McpManager) -> Option<std::path::PathBuf> { m.inner.app_root() }

#[tauri::command]
pub fn mcp_list_servers(m: State<'_, McpManager>) -> Result<Vec<McpServerStatus>> { Ok(m.inner.statuses()) }
#[tauri::command]
pub fn mcp_list_configs(m: State<'_, McpManager>) -> Result<Vec<McpServerConfig>> { Ok(m.inner.configs()) }
#[tauri::command]
pub async fn mcp_add_server(m: State<'_, McpManager>, config: McpServerConfig, scope: String) -> Result<()> {
    if config.name.trim().is_empty() { return Err(CaretError::invalid("server name is required")); }
    config::save_server(&scope, root(&m).as_deref(), &config)?;
    m.inner.reload().await; Ok(())
}
#[tauri::command]
pub async fn mcp_update_server(m: State<'_, McpManager>, config: McpServerConfig) -> Result<()> {
    let scope = config.id.split_once(':').map(|(s, _)| s.to_string()).unwrap_or_else(|| "user".into());
    config::save_server(&scope, root(&m).as_deref(), &config)?;
    m.inner.reload().await; Ok(())
}
#[tauri::command]
pub async fn mcp_remove_server(m: State<'_, McpManager>, server_id: String) -> Result<()> { config::remove_server(&server_id, root(&m).as_deref())?; m.inner.reload().await; Ok(()) }
#[tauri::command]
pub async fn mcp_toggle_server(m: State<'_, McpManager>, server_id: String, enabled: bool) -> Result<()> { config::set_enabled(&server_id, enabled)?; m.inner.reload().await; Ok(()) }
#[tauri::command]
pub async fn mcp_restart_server(m: State<'_, McpManager>, server_id: String) -> Result<()> { m.inner.restart(&server_id).await }
#[tauri::command]
pub fn mcp_tools(m: State<'_, McpManager>) -> Result<Vec<McpToolInfo>> { Ok(m.inner.tools()) }
#[tauri::command]
pub async fn mcp_toggle_tool(m: State<'_, McpManager>, server_id: String, tool: String, enabled: bool) -> Result<()> { config::set_tool_enabled(&server_id, &tool, enabled)?; m.inner.reload().await; Ok(()) }
#[tauri::command]
pub fn mcp_resources(m: State<'_, McpManager>, server_id: Option<String>) -> Result<Vec<McpResourceInfo>> { Ok(m.inner.resources(server_id.as_deref())) }
#[tauri::command]
pub async fn mcp_read_resource(m: State<'_, McpManager>, server_id: String, uri: String) -> Result<McpCallResult> { m.inner.read_resource(&server_id, &uri).await }
#[tauri::command]
pub fn mcp_prompts(m: State<'_, McpManager>) -> Result<Vec<McpPromptInfo>> { Ok(m.inner.prompts()) }
#[tauri::command]
pub async fn mcp_get_prompt(m: State<'_, McpManager>, server_id: String, name: String, args: Option<HashMap<String, String>>) -> Result<Vec<McpPromptMessage>> { m.inner.get_prompt(&server_id, &name, args.unwrap_or_default()).await }
#[tauri::command]
pub async fn mcp_call_tool(m: State<'_, McpManager>, server_id: String, name: String, args: Option<serde_json::Value>) -> Result<McpCallResult> { m.inner.call_tool(&server_id, &name, args.unwrap_or(serde_json::Value::Object(Default::default()))).await }
#[tauri::command]
pub fn mcp_logs(m: State<'_, McpManager>, server_id: String) -> Result<Vec<String>> { Ok(m.inner.logs(&server_id)) }

//! Tauri commands for the Agents settings page and the composer's agent picker.
use crate::acp::{bridge, registry, AcpAgentConfig, AcpAgentStatus, AcpRegistryEntry, AcpState};
use crate::ai::agent::manager::AgentManager;
use crate::ai::agent::session::AgentEvent;
use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

fn mgr(app: &AppHandle) -> Arc<AgentManager> { app.state::<Arc<AgentManager>>().inner().clone() }

/// Enabled agents with live status (installed / idle / starting / ready / error / unsupported).
#[tauri::command]
pub async fn acp_list_agents(app: AppHandle, st: State<'_, AcpState>) -> Result<Vec<AcpAgentStatus>> {
    let settings = app.state::<SettingsState>().effective();
    let root = mgr(&app).workspace_root();
    Ok(registry::all_configs(&settings).into_iter().filter(|c| c.enabled).map(|c| bridge::status_for(&st.inner, &c, root.as_deref())).collect())
}

/// All agent configs (builtins merged with user overrides), including disabled ones.
#[tauri::command]
pub fn acp_list_configs(app: AppHandle, _st: State<'_, AcpState>) -> Result<Vec<AcpAgentConfig>> {
    Ok(registry::all_configs(&app.state::<SettingsState>().effective()))
}

#[tauri::command]
pub fn acp_add_agent(app: AppHandle, _st: State<'_, AcpState>, config: AcpAgentConfig) -> Result<Vec<AcpAgentConfig>> {
    let s = app.state::<SettingsState>();
    registry::upsert_user_agent(&s, config)?;
    Ok(registry::all_configs(&s.effective()))
}

#[tauri::command]
pub async fn acp_remove_agent(app: AppHandle, st: State<'_, AcpState>, agent_id: String) -> Result<Vec<AcpAgentConfig>> {
    let s = app.state::<SettingsState>();
    registry::remove_user_agent(&s, &agent_id)?;
    shutdown_agent(&st.inner, &agent_id).await;
    Ok(registry::all_configs(&s.effective()))
}

#[tauri::command]
pub async fn acp_toggle_agent(app: AppHandle, st: State<'_, AcpState>, agent_id: String, enabled: bool) -> Result<Vec<AcpAgentConfig>> {
    let s = app.state::<SettingsState>();
    registry::set_enabled(&s, &agent_id, enabled)?;
    if !enabled { shutdown_agent(&st.inner, &agent_id).await; }
    Ok(registry::all_configs(&s.effective()))
}

async fn shutdown_agent(inner: &Arc<crate::acp::AcpInner>, agent_id: &str) {
    let keys: Vec<String> = inner.conns.iter().filter(|e| e.value().agent_id == agent_id).map(|e| e.key().clone()).collect();
    for k in keys { if let Some((_, c)) = inner.conns.remove(&k) { c.shutdown().await; } }
    inner.probes.remove(agent_id);
}

/// Start the agent (if needed) and run the `initialize` handshake. Never fails: errors come back in `status.error`.
#[tauri::command]
pub async fn acp_probe(app: AppHandle, st: State<'_, AcpState>, agent_id: String) -> Result<AcpAgentStatus> {
    let m = mgr(&app);
    let settings = app.state::<SettingsState>().effective();
    let Some(cfg) = registry::find_config(&settings, &agent_id) else { return Err(CaretError::not_found(format!("agent {agent_id}"))) };
    if !crate::acp::process::is_installed(&cfg) { return Ok(bridge::status_for(&st.inner, &cfg, m.workspace_root().as_deref())); }
    let inner = st.inner.clone();
    match bridge::ensure_conn(&inner, &m, &agent_id).await {
        Ok(c) => Ok(bridge::status_from_conn(&c)),
        Err(e) => { let mut s = bridge::status_for(&inner, &cfg, m.workspace_root().as_deref()); s.state = "error".into(); s.error = Some(e.message); Ok(s) }
    }
}

#[tauri::command]
pub async fn acp_authenticate(app: AppHandle, st: State<'_, AcpState>, agent_id: String, method_id: String) -> Result<AcpAgentStatus> {
    let m = mgr(&app);
    let inner = st.inner.clone();
    let c = bridge::ensure_conn(&inner, &m, &agent_id).await?;
    c.authenticate(&method_id).await?;
    let mut s = bridge::status_from_conn(&c); s.auth_required = Some(false);
    inner.probes.insert(agent_id, s.clone());
    Ok(s)
}

#[tauri::command]
pub async fn acp_set_mode(app: AppHandle, st: State<'_, AcpState>, session_id: String, mode_id: String) -> Result<AcpAgentStatus> {
    let inner = st.inner.clone();
    let link = inner.links.get(&session_id).map(|l| l.clone()).ok_or_else(|| CaretError::not_found("agent session (send a message first)"))?;
    let conn = inner.conns.get(&link.conn_key).map(|c| c.clone()).filter(|c| c.alive()).ok_or_else(|| CaretError::provider("agent is not connected"))?;
    conn.set_mode(&link.acp_id, &mode_id).await?;
    if let Some(ms) = link.modes.lock().as_mut() { ms.current_mode_id = mode_id.clone(); }
    let s = bridge::status_for_link(&conn, &link);
    mgr(&app).emit(AgentEvent::AgentStatus { session_id, agent_id: link.agent_id.clone(), status: s.clone() });
    Ok(s)
}

/// Change a session config option (model, effort, …) via `session/set_config_option` and remember it as the default
/// for this agent's future sessions.
#[tauri::command]
pub async fn acp_set_config_option(app: AppHandle, st: State<'_, AcpState>, session_id: String, config_id: String, value: serde_json::Value) -> Result<AcpAgentStatus> {
    let inner = st.inner.clone();
    let link = inner.links.get(&session_id).map(|l| l.clone()).ok_or_else(|| CaretError::not_found("agent session (pick the agent first)"))?;
    let conn = inner.conns.get(&link.conn_key).map(|c| c.clone()).filter(|c| c.alive()).ok_or_else(|| CaretError::provider("agent is not connected"))?;
    let updated = conn.set_config_option(&link.acp_id, &config_id, &value).await?;
    {
        let mut g = link.config_options.lock();
        if let Some(u) = updated { *g = Some(u); }
        else if let Some(opts) = g.as_mut() { if let Some(o) = opts.iter_mut().find(|o| o.id == config_id) { o.current_value = value.clone(); } }
    }
    if let Err(e) = registry::remember_session_default(&app.state::<SettingsState>(), &link.agent_id, &config_id, value) { inner.log(&link.agent_id, format!("[acp] could not persist option default: {e:?}")); }
    let s = bridge::status_for_link(&conn, &link);
    mgr(&app).emit(AgentEvent::AgentStatus { session_id, agent_id: link.agent_id.clone(), status: s.clone() });
    Ok(s)
}

/// Connect the agent and create its ACP session right away (before the first prompt) so the composer can show the
/// agent's model/effort/mode options up front.
#[tauri::command]
pub async fn acp_prepare_session(app: AppHandle, st: State<'_, AcpState>, session_id: String, agent_id: String) -> Result<AcpAgentStatus> {
    let inner = st.inner.clone();
    let m = mgr(&app);
    let h = m.handle(&session_id)?;
    let conn = bridge::ensure_conn(&inner, &m, &agent_id).await?;
    let (link, _) = bridge::ensure_session(&inner, &m, Some(&app), &h, &conn).await?;
    Ok(bridge::status_for_link(&conn, &link))
}

#[tauri::command]
pub fn acp_registry(app: AppHandle, _st: State<'_, AcpState>) -> Result<Vec<AcpRegistryEntry>> { Ok(registry::registry(&app.state::<SettingsState>().effective())) }

#[tauri::command]
pub async fn acp_install(app: AppHandle, st: State<'_, AcpState>, entry_id: String) -> Result<AcpRegistryEntry> {
    let settings = app.state::<SettingsState>().effective();
    let inner = st.inner.clone();
    let e = registry::install(inner.clone(), &settings, &entry_id).await?;
    inner.probes.remove(&entry_id);
    Ok(e)
}

#[tauri::command]
pub fn acp_logs(st: State<'_, AcpState>, agent_id: String) -> Result<Vec<String>> { Ok(st.inner.logs_for(&agent_id)) }

/// Current ACP session modes/commands for a Caret session (used by the composer when an external agent drives it).
#[tauri::command]
pub fn acp_session_info(st: State<'_, AcpState>, session_id: String) -> Result<Option<serde_json::Value>> {
    Ok(st.inner.links.get(&session_id).map(|l| serde_json::json!({ "agentId": l.agent_id, "acpSessionId": l.acp_id, "modes": l.modes.lock().clone(), "configOptions": l.config_options.lock().clone(), "commands": l.commands.lock().clone() })))
}

//! Tauri commands for Settings → Models → Auto routing.
use super::{assign, manager::RouterManager, Assignment, RouterStatus, Tier};
use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use serde_json::json;
use std::sync::Arc;
use tauri::{AppHandle, Manager};

fn mgr(app: &AppHandle) -> Result<Arc<RouterManager>> { super::manager(app).ok_or_else(|| CaretError::internal("router not initialised")) }

#[tauri::command(async)]
pub fn router_status(app: AppHandle) -> Result<RouterStatus> { Ok(mgr(&app)?.status()) }

#[tauri::command]
pub async fn router_install(app: AppHandle) -> Result<RouterStatus> { mgr(&app)?.install().await }

#[tauri::command]
pub async fn router_restart(app: AppHandle) -> Result<RouterStatus> { mgr(&app)?.restart().await }

#[tauri::command]
pub fn router_stop(app: AppHandle) -> Result<RouterStatus> { let m = mgr(&app)?; m.stop(); Ok(m.status()) }

#[tauri::command(async)]
pub fn router_logs(app: AppHandle) -> Result<Vec<String>> { Ok(mgr(&app)?.logs()) }

/// Tier assignment computed from the current settings (what the sidecar would be configured with).
#[tauri::command(async)]
pub fn router_preview_assignment(app: AppHandle) -> Result<Assignment> {
    let st = app.state::<SettingsState>();
    let s = st.effective();
    Ok(assign::assign_tiers(&s, &|p| crate::ai::util::usable(p, &st)))
}

/// Pin a tier to a model (`model_id = None` clears the override). Persists in user settings and reloads the sidecar.
#[tauri::command]
pub fn router_set_override(app: AppHandle, tier: String, model_id: Option<String>) -> Result<RouterStatus> {
    let t = Tier::parse(&tier).ok_or_else(|| CaretError::invalid(format!("unknown tier {tier}")))?;
    let st = app.state::<SettingsState>();
    if let Some(id) = &model_id { if st.effective().model(id).is_none() { return Err(CaretError::not_found(format!("model {id}"))); } }
    st.patch(&json!({ "router": { "overrides": { t.key(): model_id } } }), "user")?;
    crate::events::emit(&app, crate::events::SETTINGS_CHANGED, st.effective());
    let m = mgr(&app)?;
    m.on_settings_changed();
    Ok(m.status())
}

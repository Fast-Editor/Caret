//! `warren_*` Tauri commands for the Sandboxes settings page (status, key, instances, exec, terminal, logs) and the
//! helpers the thread executor shares with them. Thread binding and `warren_sync` live with the session code.
use super::client::TERMINAL_PORT;
use super::types::{wire, WarrenExecResult, WarrenInstance, WarrenInstanceStatus, WarrenLogs, WarrenSignedUrl, WarrenStatus, WarrenWorkspace};
use super::{cached_client, client_for, resolve, WarrenClient};
use crate::core::settings::{SettingsState, WarrenSettings, WarrenShape};
use crate::error::{CaretError, Result};
use crate::events;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// Options of `warren_create_instance`; unset fields come from `warren.*` settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateInstanceOpts {
    pub name: Option<String>,
    pub template: Option<String>,
    pub shape: Option<WarrenShape>,
    pub auto_sleep: Option<bool>,
    pub idle_timeout_seconds: Option<u32>,
}

/// Warren's create request for `opts` over the settings defaults; only set fields are serialised.
pub fn create_request(w: &WarrenSettings, opts: &CreateInstanceOpts) -> wire::CreateInstanceRequest {
    let shape = opts.shape.unwrap_or(w.shape);
    wire::CreateInstanceRequest {
        template: Some(opts.template.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| w.template.clone())),
        shape: Some(wire::ShapeRequest { cpu: Some(shape.cpu), memory: Some(shape.memory), disk: Some(shape.disk) }),
        name: opts.name.clone().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()),
        auto_sleep: Some(opts.auto_sleep.unwrap_or(w.auto_sleep)),
        idle_timeout_seconds: Some(opts.idle_timeout_seconds.unwrap_or(w.idle_timeout_seconds)),
        ..Default::default()
    }
}

/// Create an instance named `name` with the `warren.*` defaults (template, shape, auto-sleep, idle timeout). Returns
/// it in `provisioning`; wait with [`WarrenClient::wait_running`]. For the executor's "new instance for this thread".
pub async fn create_instance_for(settings: &SettingsState, name: &str) -> Result<WarrenInstance> {
    let (w, key) = resolve(settings);
    if key.is_none() { return Err(no_key(&w)); }
    let client = client_for(settings)?;
    let req = create_request(&w, &CreateInstanceOpts { name: Some(name.to_string()), ..Default::default() });
    client.create_instance(&req).await.map(WarrenInstance::from)
}

fn no_key(w: &WarrenSettings) -> CaretError { CaretError::config(format!("No Warren API key is set (secret `{}`); add one in Settings → Sandboxes", w.api_key_ref)) }

/// The cached client, or a `config` error when no key is stored.
fn client(app: &AppHandle) -> Result<WarrenClient> {
    let settings = app.state::<SettingsState>();
    let (w, key) = resolve(&settings);
    if key.is_none() { return Err(no_key(&w)); }
    cached_client(app)
}

/// Probe the daemon and the key: `health` (anonymous), then `workspace` when a key is set.
pub async fn status(app: &AppHandle) -> WarrenStatus {
    let settings = app.state::<SettingsState>();
    let (_, key) = resolve(&settings);
    let mut st = WarrenStatus { configured: key.is_some(), ..Default::default() };
    let client = match cached_client(app) { Ok(c) => c, Err(e) => { st.error = Some(e.message); return st; } };
    match client.health().await {
        Ok(h) => { st.reachable = true; st.version = Some(h.version).filter(|v| !v.is_empty()); st.dev_mode = Some(h.dev_mode); }
        Err(e) => { st.error = Some(e.message); return st; }
    }
    if !st.configured { return st; }
    match client.workspace().await {
        Ok(ws) => st.workspace = Some(WarrenWorkspace { id: ws.id, name: ws.name, wallet_micros: ws.wallet_micros, awake_limit: ws.awake_limit, awake_count: ws.awake_count }),
        Err(e) => st.error = Some(e.message),
    }
    st
}

fn changed(app: &AppHandle, id: Option<&str>) { events::emit(app, events::WARREN_CHANGED, serde_json::json!({ "id": id })); }

#[tauri::command]
pub async fn warren_status(app: AppHandle) -> Result<WarrenStatus> { Ok(status(&app).await) }

/// Store `key` under `warren.apiKeyRef` (an empty key removes it) and probe again.
#[tauri::command]
pub async fn warren_set_key(app: AppHandle, key: String) -> Result<WarrenStatus> {
    let settings = app.state::<SettingsState>();
    let id = settings.effective().warren.api_key_ref;
    settings.set_secret(&id, key.trim())?;
    if let Some(st) = app.try_state::<super::WarrenState>() { st.invalidate(); }
    Ok(status(&app).await)
}

#[tauri::command]
pub async fn warren_list_instances(app: AppHandle) -> Result<Vec<WarrenInstance>> {
    Ok(client(&app)?.instances().await?.into_iter().map(WarrenInstance::from).collect())
}

#[tauri::command]
pub async fn warren_get_instance(app: AppHandle, id: String) -> Result<WarrenInstance> { client(&app)?.instance(&id).await.map(WarrenInstance::from) }

/// Create an instance; it comes back in `provisioning` and the UI polls `warren_get_instance`.
#[tauri::command]
pub async fn warren_create_instance(app: AppHandle, opts: Option<CreateInstanceOpts>) -> Result<WarrenInstance> {
    let w = app.state::<SettingsState>().effective().warren;
    let req = create_request(&w, &opts.unwrap_or_default());
    let inst = client(&app)?.create_instance(&req).await.map(WarrenInstance::from)?;
    changed(&app, Some(&inst.id));
    Ok(inst)
}

/// `start | stop | sleep | restart | delete` → `{ id, status }`.
#[tauri::command]
pub async fn warren_instance_action(app: AppHandle, id: String, action: String) -> Result<WarrenInstanceStatus> {
    let c = client(&app)?;
    let r = match action.as_str() {
        "start" => c.start(&id).await?,
        "stop" => c.stop(&id).await?,
        "sleep" => c.sleep(&id).await?,
        "restart" => c.restart(&id).await?,
        "delete" => c.delete_instance(&id).await?,
        other => return Err(CaretError::invalid(format!("unknown instance action `{other}` (start, stop, sleep, restart, delete)"))),
    };
    changed(&app, Some(&id));
    Ok(r.into())
}

/// Run a shell command in the instance (wakes it when asleep).
#[tauri::command]
pub async fn warren_exec(app: AppHandle, id: String, command: String, timeout_secs: Option<u32>) -> Result<WarrenExecResult> {
    let req = wire::ExecRequest { timeout_seconds: timeout_secs.unwrap_or(0), ..wire::ExecRequest::new(command) };
    client(&app)?.exec(&id, &req).await.map(WarrenExecResult::from)
}

/// Signed URL of the instance's browser terminal (ttyd on port 7681); `ttl_secs` defaults to Warren's (60..86400).
#[tauri::command]
pub async fn warren_terminal_url(app: AppHandle, id: String, ttl_secs: Option<u64>) -> Result<WarrenSignedUrl> {
    client(&app)?.signed_url(&id, Some(TERMINAL_PORT), ttl_secs).await.map(WarrenSignedUrl::from)
}

#[tauri::command]
pub async fn warren_logs(app: AppHandle, id: String, tail: Option<u32>) -> Result<WarrenLogs> {
    client(&app)?.logs(&id, tail).await.map(|logs| WarrenLogs { logs })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_request_layers_options_over_settings() {
        let w = WarrenSettings::default();
        let req = create_request(&w, &CreateInstanceOpts::default());
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v, serde_json::json!({ "template": "warren-base", "resources": { "cpu": 2, "memory": 4, "disk": 4 }, "auto_sleep": true, "idle_timeout_seconds": 900 }));
        let opts: CreateInstanceOpts = serde_json::from_value(serde_json::json!({ "name": " caret ", "template": "warren-agent", "shape": { "cpu": 4, "memory": 8, "disk": 20 }, "autoSleep": false, "idleTimeoutSeconds": 3600 })).unwrap();
        let v = serde_json::to_value(create_request(&w, &opts)).unwrap();
        assert_eq!(v, serde_json::json!({ "template": "warren-agent", "resources": { "cpu": 4, "memory": 8, "disk": 20 }, "name": "caret", "auto_sleep": false, "idle_timeout_seconds": 3600 }));
        let blank = create_request(&w, &CreateInstanceOpts { name: Some("  ".into()), template: Some("".into()), ..Default::default() });
        assert!(blank.name.is_none());
        assert_eq!(blank.template.as_deref(), Some("warren-base"));
    }
}

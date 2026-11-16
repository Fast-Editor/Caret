//! Part B's seam onto part A's Warren client and sync layer (docs/WARREN.md, "Rust layout").
//!
//! Everything the executor, the binding commands and the session model need from `types`, `client`, `sync`,
//! `client_for` and `commands::create_instance_for` goes through this file, so a signature difference between the
//! two parts is fixed here alone. `WarrenRemote` is the production `RemoteOps` (push → exec → pull) behind
//! `WarrenExecutor`; tests use a fake `RemoteOps` and never touch this file.
pub use super::client::WarrenClient;
pub use super::types::{SandboxBinding, SandboxStatus, WarrenInstance};

use super::executor::{PushStats, RemoteExec, RemoteOps, RemotePull};
use super::sync::{self, Manifest};
use super::types::wire::ExecRequest;
use crate::ai::agent::manager::AgentManager;
use crate::ai::agent::session::now_ms;
use crate::core::settings::SettingsState;
use tauri::Manager;
use crate::error::Result;
use async_trait::async_trait;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

// ───────────────────────── client / instances ─────────────────────────

/// A client for `settings.warren` (part A caches one per settings fingerprint; the `Arc` is ours).
pub fn connect(mgr: &AgentManager) -> Result<Arc<WarrenClient>> {
    let app = mgr.app_opt().ok_or_else(|| crate::error::CaretError::config("Warren needs the running app (no app handle in this context)"))?;
    Ok(Arc::new(super::cached_client(&app)?))
}

pub async fn create_instance(mgr: &AgentManager, name: &str) -> Result<WarrenInstance> {
    let app = mgr.app_opt().ok_or_else(|| crate::error::CaretError::config("Warren needs the running app (no app handle in this context)"))?;
    let st = app.state::<SettingsState>();
    super::commands::create_instance_for(&st, name).await
}
pub async fn instance(client: &WarrenClient, id: &str) -> Result<WarrenInstance> { Ok(client.instance(id).await?.into()) }
pub async fn wait_running(client: &WarrenClient, id: &str, timeout: Duration) -> Result<WarrenInstance> { Ok(client.wait_running(id, timeout).await?.into()) }
pub async fn delete_instance(client: &WarrenClient, id: &str) -> Result<()> { client.delete_instance(id).await.map(|_| ()) }

/// Instance id and display name of a Warren instance object.
pub fn instance_ident(i: &WarrenInstance) -> (String, String) { (i.id.clone(), i.name.clone()) }

// ───────────────────────── binding ─────────────────────────

/// A fresh `ready` binding for an instance whose synced copy lives at `remote_root`.
pub fn new_binding(instance_id: &str, instance_name: &str, remote_root: String) -> SandboxBinding { SandboxBinding::new(instance_id, instance_name, remote_root) }
/// `status` is one of `ready | syncing | running | error`.
pub fn set_status(b: &mut SandboxBinding, status: &str, error: Option<String>) {
    b.status = match status { "syncing" => SandboxStatus::Syncing, "running" => SandboxStatus::Running, "error" => SandboxStatus::Error, _ => SandboxStatus::Ready };
    b.error = error;
}
pub fn record_push(b: &mut SandboxBinding, files: u64) { b.last_push_at = Some(now_ms()); if files > 0 { b.pushed_files = b.pushed_files.saturating_add(files as usize); } }
pub fn record_pull(b: &mut SandboxBinding) { b.last_pull_at = Some(now_ms()); }
pub fn binding_status(b: &SandboxBinding) -> &'static str { match b.status { SandboxStatus::Ready => "ready", SandboxStatus::Syncing => "syncing", SandboxStatus::Running => "running", SandboxStatus::Error => "error" } }

// ───────────────────────── sync state ─────────────────────────

/// Manifest of the last successful push per instance id. Executors live one turn; the manifest outlives them so
/// the next turn only pushes what changed since. Cleared when a thread (re)binds the instance.
static MANIFESTS: Lazy<Mutex<HashMap<String, Manifest>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub fn forget_manifest(instance_id: &str) { MANIFESTS.lock().remove(instance_id); }

/// Production `RemoteOps`: part A's `sync::{scan, plan_push, push, pull}` and `WarrenClient::exec` for one binding.
pub struct WarrenRemote { client: Arc<WarrenClient>, binding: SandboxBinding }

impl WarrenRemote {
    pub fn new(client: Arc<WarrenClient>, binding: SandboxBinding) -> Self { Self { client, binding } }
}

#[async_trait]
impl RemoteOps for WarrenRemote {
    async fn push_changes(&self, local_root: &Path, excludes: &[String]) -> Result<PushStats> {
        let now = sync::scan(local_root, excludes)?;
        let prev = MANIFESTS.lock().get(&self.binding.instance_id).cloned();
        let plan = sync::plan_push(prev.as_ref().unwrap_or(&Manifest::default()), &now);
        if plan.upload.is_empty() && plan.delete.is_empty() { return Ok(PushStats::default()); }
        let rep = sync::push(&*self.client, &self.binding, local_root, &plan).await?;
        MANIFESTS.lock().insert(self.binding.instance_id.clone(), now);
        Ok(PushStats { files: rep.files as u64, bytes: rep.bytes, deleted: rep.deleted as u64 })
    }
    async fn exec(&self, command: &str, timeout_secs: u64) -> Result<RemoteExec> {
        let req = ExecRequest { command: command.to_string(), timeout_seconds: timeout_secs.min(u32::MAX as u64) as u32, ..Default::default() };
        let r = self.client.exec(&self.binding.instance_id, &req).await?;
        Ok(RemoteExec { exit_code: r.exit_code, stdout: r.stdout, stderr: r.stderr, truncated: r.truncated })
    }
    async fn pull_changes(&self, local_root: &Path, excludes: &[String]) -> Result<RemotePull> {
        let pushed = MANIFESTS.lock().get(&self.binding.instance_id).cloned().unwrap_or_default();
        let rep = sync::pull(&*self.client, &self.binding, local_root, &pushed, excludes).await?;
        Ok(RemotePull { changed: rep.changed, deleted: rep.deleted })
    }
}

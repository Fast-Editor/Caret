//! Part B's seam onto part A's Warren client and sync layer (docs/WARREN.md, "Rust layout").
//!
//! Everything the executor, the binding commands and the session model need from `types`, `client`, `sync`,
//! `client_for` and `commands::create_instance_for` goes through this file, so a signature difference between the
//! two parts is fixed here alone. `WarrenRemote` is the production `RemoteOps` (push → exec → pull) behind
//! `WarrenExecutor`; tests use a fake `RemoteOps` and never touch this file.
pub use super::client::WarrenClient;
pub use super::types::{SandboxBinding, WarrenInstance};

use super::executor::{PushStats, RemoteExec, RemoteOps, RemotePull};
use super::sync::{self, Manifest};
use super::types::ExecRequest;
use crate::ai::agent::session::now_ms;
use crate::core::settings::Settings;
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
pub fn connect(settings: &Settings) -> Result<Arc<WarrenClient>> { Ok(Arc::new(super::client_for(settings)?)) }

pub async fn create_instance(settings: &Settings, name: &str) -> Result<WarrenInstance> { super::commands::create_instance_for(settings, name).await }
pub async fn instance(client: &WarrenClient, id: &str) -> Result<WarrenInstance> { client.instance(id).await }
pub async fn wait_running(client: &WarrenClient, id: &str, timeout: Duration) -> Result<WarrenInstance> { client.wait_running(id, timeout).await }
pub async fn delete_instance(client: &WarrenClient, id: &str) -> Result<()> { client.delete_instance(id).await }

/// Instance id and display name of a Warren instance object.
pub fn instance_ident(i: &WarrenInstance) -> (String, String) { (i.id.clone(), i.name.clone()) }

// ───────────────────────── binding ─────────────────────────

/// A fresh `ready` binding for an instance whose synced copy lives at `remote_root`.
pub fn new_binding(instance_id: &str, instance_name: &str, remote_root: String) -> SandboxBinding {
    SandboxBinding { provider: "warren".into(), instance_id: instance_id.into(), instance_name: instance_name.into(), remote_root, last_push_at: None, last_pull_at: None, pushed_files: 0, status: "ready".into(), error: None }
}
/// `status` is one of `ready | syncing | running | error`.
pub fn set_status(b: &mut SandboxBinding, status: &str, error: Option<String>) { b.status = status.into(); b.error = error; }
pub fn record_push(b: &mut SandboxBinding, files: u64) { b.last_push_at = Some(now_ms()); if files > 0 { b.pushed_files = b.pushed_files.saturating_add(files); } }
pub fn record_pull(b: &mut SandboxBinding) { b.last_pull_at = Some(now_ms()); }
pub fn binding_status(b: &SandboxBinding) -> &str { &b.status }

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
        let plan = sync::plan_push(prev.as_ref(), &now);
        if plan.upload.is_empty() && plan.delete.is_empty() { return Ok(PushStats::default()); }
        let rep = sync::push(&self.client, &self.binding, local_root, &plan).await?;
        MANIFESTS.lock().insert(self.binding.instance_id.clone(), now);
        Ok(PushStats { files: rep.files as u64, bytes: rep.bytes, deleted: rep.deleted as u64 })
    }
    async fn exec(&self, command: &str, timeout_secs: u64) -> Result<RemoteExec> {
        let req = ExecRequest { command: command.to_string(), timeout_seconds: Some(timeout_secs), ..Default::default() };
        let r = self.client.exec(&self.binding.instance_id, &req).await?;
        Ok(RemoteExec { exit_code: r.exit_code, stdout: r.stdout, stderr: r.stderr, truncated: r.truncated })
    }
    async fn pull_changes(&self, local_root: &Path, _excludes: &[String]) -> Result<RemotePull> {
        let pushed = MANIFESTS.lock().get(&self.binding.instance_id).cloned().unwrap_or_default();
        let rep = sync::pull(&self.client, &self.binding, local_root, &pushed).await?;
        Ok(RemotePull { changed: rep.changed, deleted: rep.deleted })
    }
}

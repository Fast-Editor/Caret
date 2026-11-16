//! Warren types. Caret-facing shapes (camelCase, mirrored in src/types.ts) live at the top level; Warren's own wire
//! shapes (snake_case, exactly as the daemon sends and accepts them) live in [`wire`]. Reading ignores unknown fields;
//! requests serialise only the fields that are set, because Warren rejects unknown JSON fields with `invalid_request`.
pub use crate::core::settings::WarrenShape;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ───────────────────────── Caret-facing ─────────────────────────

/// The workspace behind the configured key, as shown on the Sandboxes page.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WarrenWorkspace {
    pub id: String,
    pub name: String,
    /// Wallet balance in micro-dollars (USD × 1,000,000).
    pub wallet_micros: i64,
    /// How many instances may be awake at once.
    pub awake_limit: u32,
    /// How many are awake now.
    pub awake_count: u32,
}

/// Result of `warren_status`: is a key configured, does the daemon answer, and whose workspace is it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WarrenStatus {
    /// A key is stored under `warren.apiKeyRef`.
    pub configured: bool,
    /// `GET /v1/health` answered.
    pub reachable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub dev_mode: Option<bool>,
    /// Present when the key was accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub workspace: Option<WarrenWorkspace>,
    /// Why `reachable` is false or `workspace` is missing (unreachable daemon, invalid key …).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub error: Option<String>,
}

/// A Warren instance, the camelCase subset of Warren's instance object the UI and the executor need.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WarrenInstance {
    pub id: String,
    pub name: String,
    pub template: String,
    pub template_revision: u32,
    /// `provisioning | running | stopping | stopped | starting | restarting | updating | sleeping | waking | failed | deleting | deleted`.
    pub status: String,
    pub status_reason: String,
    pub resources: WarrenShape,
    /// Instance URL at the default port, derived by Warren from its base domain.
    pub url: String,
    pub default_port: u16,
    /// Where the persistent disk mounts (the home of the exec user), e.g. `/home/agent`.
    pub mount_path: String,
    pub auto_sleep: bool,
    pub idle_timeout_seconds: u32,
    /// Unix seconds.
    pub created_at: i64,
    pub updated_at: i64,
    pub last_activity_at: i64,
    pub disk_used_bytes: i64,
    /// `docker` or `firecracker`.
    pub runtime: String,
}

impl From<wire::Instance> for WarrenInstance {
    fn from(i: wire::Instance) -> Self {
        Self {
            id: i.id, name: i.name, template: i.template, template_revision: i.template_revision, status: i.status, status_reason: i.status_reason,
            resources: WarrenShape { cpu: i.shape.cpu, memory: i.shape.memory, disk: i.shape.disk },
            url: i.url, default_port: i.default_port, mount_path: i.mount_path, auto_sleep: i.auto_sleep, idle_timeout_seconds: i.idle_timeout_seconds,
            created_at: i.created_at, updated_at: i.updated_at, last_activity_at: i.last_activity_at, disk_used_bytes: i.disk_used_bytes, runtime: i.runtime,
        }
    }
}

/// What a command run inside an instance produced.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WarrenExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: i64,
    /// A stream was cut at Warren's 1 MiB cap.
    pub truncated: bool,
}

impl From<wire::ExecResult> for WarrenExecResult {
    fn from(r: wire::ExecResult) -> Self {
        Self { exit_code: r.exit_code, stdout: r.stdout, stderr: r.stderr, duration_ms: r.duration_ms, truncated: r.truncated }
    }
}

/// `{ id, status }` of a lifecycle action (`start`, `stop`, `sleep`, `restart`, `delete`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WarrenInstanceStatus { pub id: String, pub status: String }

impl From<wire::StatusResponse> for WarrenInstanceStatus {
    fn from(s: wire::StatusResponse) -> Self { Self { id: s.id, status: s.status } }
}

/// A signed URL for an instance port (the browser terminal on 7681), valid until `expires_at` (unix seconds).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WarrenSignedUrl { pub url: String, pub expires_at: i64 }

impl From<wire::SignedUrl> for WarrenSignedUrl {
    fn from(s: wire::SignedUrl) -> Self { Self { url: s.url, expires_at: s.expires_at } }
}

/// `warren_logs` result.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WarrenLogs { pub logs: String }

/// Sync status of a bound thread.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SandboxStatus {
    /// Bound and idle.
    #[default]
    Ready,
    /// A push or pull is in flight.
    Syncing,
    /// A command is running in the instance.
    Running,
    /// The last operation failed; see `SandboxBinding::error`.
    Error,
}

/// A thread's sandbox: which Warren instance runs its commands and where the workspace copy lives there. Stored on
/// the session and shown to the UI as `sandbox` on `ChatSessionView` / `SessionSummary`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SandboxBinding {
    /// Always `"warren"`; room for other sandbox providers later.
    pub provider: String,
    pub instance_id: String,
    pub instance_name: String,
    /// Absolute path of the synced workspace copy inside the instance: `<warren.remoteRoot>/<folder name>`. Commands
    /// run with this as their working directory; push extracts into it and pull lists it.
    pub remote_root: String,
    /// Unix milliseconds of the last successful push / pull.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub last_push_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub last_pull_at: Option<u64>,
    /// How many workspace files the instance currently holds from Caret (the size of the pushed manifest, which the
    /// executor keeps separately; see `sync::Manifest`).
    #[serde(default)] pub pushed_files: usize,
    #[serde(default)] pub status: SandboxStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub error: Option<String>,
}

impl SandboxBinding {
    /// A fresh binding to `instance` with the workspace copy at `remote_root`.
    pub fn new(instance_id: impl Into<String>, instance_name: impl Into<String>, remote_root: impl Into<String>) -> Self {
        Self { provider: "warren".into(), instance_id: instance_id.into(), instance_name: instance_name.into(), remote_root: remote_root.into(), last_push_at: None, last_pull_at: None, pushed_files: 0, status: SandboxStatus::Ready, error: None }
    }
}

/// Result of `warren_sync` (and of the executor's implicit syncs).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    /// `"push"` or `"pull"`.
    pub direction: String,
    /// Files uploaded (push) or changed remotely and brought back (pull).
    pub files: usize,
    /// Bytes that crossed the wire (the gzip tarball).
    pub bytes: u64,
    /// Files removed remotely (push) or found missing remotely (pull).
    pub deleted: usize,
    pub ms: u64,
}

// ───────────────────────── Warren wire shapes ─────────────────────────

/// Warren's JSON exactly as the daemon sends and accepts it (see /tmp/warren-docs/API.md and `warren_core`).
pub mod wire {
    use super::*;

    /// `resources` on an instance: vCPU, GB RAM, GB disk.
    #[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
    pub struct Shape { #[serde(default)] pub cpu: u32, #[serde(default)] pub memory: u32, #[serde(default)] pub disk: u32 }

    /// Partial shape in requests; only set fields are sent.
    #[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
    pub struct ShapeRequest {
        #[serde(default, skip_serializing_if = "Option::is_none")] pub cpu: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")] pub memory: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")] pub disk: Option<u32>,
    }

    /// Managed-service spend caps.
    #[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
    pub struct Budget { #[serde(default)] pub monthly_cap_micros: i64, #[serde(default)] pub credit_micros: i64 }

    /// The instance object. `type` and `resources` on the wire are `kind` and `shape` here, as in Warren's own SDK.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct Instance {
        #[serde(default)] pub id: String,
        #[serde(default)] pub workspace_id: String,
        #[serde(default)] pub name: String,
        #[serde(default)] pub template: String,
        #[serde(default)] pub template_name: String,
        #[serde(default)] pub template_revision: u32,
        #[serde(default)] pub image_ref: String,
        /// `default | vm | performance`.
        #[serde(default, rename = "type")] pub kind: String,
        #[serde(default)] pub status: String,
        #[serde(default)] pub status_reason: String,
        #[serde(default, rename = "resources")] pub shape: Shape,
        #[serde(default)] pub region: String,
        #[serde(default)] pub metadata: BTreeMap<String, String>,
        #[serde(default)] pub env: BTreeMap<String, String>,
        /// `allow-all | deny-all`.
        #[serde(default)] pub network: String,
        #[serde(default)] pub auto_sleep: bool,
        #[serde(default)] pub idle_timeout_seconds: u32,
        #[serde(default)] pub budget: Budget,
        #[serde(default)] pub url: String,
        #[serde(default)] pub default_port: u16,
        #[serde(default)] pub mount_path: String,
        #[serde(default)] pub agent_api: bool,
        #[serde(default)] pub parent_id: String,
        #[serde(default)] pub created_at: i64,
        #[serde(default)] pub updated_at: i64,
        #[serde(default)] pub started_at: i64,
        #[serde(default)] pub stopped_at: i64,
        #[serde(default)] pub slept_at: i64,
        #[serde(default)] pub last_activity_at: i64,
        #[serde(default)] pub meter_started_at: i64,
        #[serde(default)] pub runtime: String,
        #[serde(default)] pub disk_used_bytes: i64,
    }

    /// Body of `POST /v1/instances`; every field optional, only set ones are sent.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct CreateInstanceRequest {
        #[serde(default, skip_serializing_if = "Option::is_none")] pub template: Option<String>,
        #[serde(default, rename = "resources", skip_serializing_if = "Option::is_none")] pub shape: Option<ShapeRequest>,
        #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")] pub kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")] pub region: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")] pub name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")] pub metadata: Option<BTreeMap<String, String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")] pub env: Option<BTreeMap<String, String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")] pub budget: Option<Budget>,
        #[serde(default, skip_serializing_if = "Option::is_none")] pub auto_sleep: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")] pub idle_timeout_seconds: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")] pub network: Option<String>,
    }

    /// Body of `POST /v1/instances/{id}/exec`. Zero / empty fields are not sent.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct ExecRequest {
        pub command: String,
        /// Empty means the image's default user (the owner of the persistent home); `root` is allowed.
        #[serde(default, skip_serializing_if = "String::is_empty")] pub user: String,
        /// Zero means Warren's default (15 minutes).
        #[serde(default, skip_serializing_if = "is_zero_u32")] pub timeout_seconds: u32,
        #[serde(default, skip_serializing_if = "String::is_empty")] pub workdir: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")] pub env: BTreeMap<String, String>,
    }

    impl ExecRequest {
        /// A request for `command` with everything else at Warren's defaults.
        pub fn new(command: impl Into<String>) -> Self { Self { command: command.into(), ..Default::default() } }
    }

    fn is_zero_u32(v: &u32) -> bool { *v == 0 }

    /// What `exec` returns; each stream is capped at 1 MiB (`truncated`).
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct ExecResult {
        #[serde(default)] pub exit_code: i32,
        #[serde(default)] pub stdout: String,
        #[serde(default)] pub stderr: String,
        #[serde(default)] pub duration_ms: i64,
        #[serde(default)] pub truncated: bool,
    }

    /// `GET /v1/health`.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct Health {
        #[serde(default)] pub healthy: bool,
        #[serde(default)] pub version: String,
        #[serde(default)] pub runtimes: Vec<String>,
        #[serde(default)] pub dev_mode: bool,
        #[serde(default)] pub time: i64,
    }

    /// `GET /v1/workspace`: the workspace object plus the awake-instance counters.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct Workspace {
        #[serde(default)] pub id: String,
        #[serde(default)] pub name: String,
        #[serde(default)] pub created_at: i64,
        #[serde(default)] pub wallet_micros: i64,
        #[serde(default)] pub top_up_total_micros: i64,
        #[serde(default)] pub free_credit_granted: bool,
        #[serde(default)] pub awake_limit: u32,
        #[serde(default)] pub awake_count: u32,
    }

    /// `GET /v1/billing`.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct Billing {
        #[serde(default)] pub wallet_micros: i64,
        #[serde(default)] pub top_up_total_micros: i64,
        #[serde(default)] pub awake_limit: u32,
        #[serde(default)] pub awake_count: u32,
        #[serde(default)] pub rates: serde_json::Value,
        #[serde(default)] pub free_credit_granted: bool,
    }

    /// One entry of `GET /v1/templates`.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct Template {
        #[serde(default)] pub name: String,
        /// `system | workspace`.
        #[serde(default)] pub scope: String,
        #[serde(default)] pub image_ref: String,
        #[serde(default)] pub default_port: u16,
        #[serde(default)] pub mount_path: String,
        #[serde(default)] pub agent: bool,
        #[serde(default)] pub description: String,
        #[serde(default)] pub types: Vec<String>,
        #[serde(default)] pub default_shape: Shape,
        #[serde(default)] pub revision: u32,
        #[serde(default)] pub created_at: i64,
        #[serde(default)] pub updated_at: i64,
    }

    /// `GET /v1/instances/{id}/signed-url`.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct SignedUrl { #[serde(default)] pub url: String, #[serde(default)] pub expires_at: i64 }

    /// `GET /v1/files?path=`: one directory entry on the instance host.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct FileEntry {
        #[serde(default)] pub name: String,
        /// `file | dir`.
        #[serde(default, rename = "type")] pub kind: String,
        #[serde(default)] pub size: u64,
        /// Unix milliseconds (the Agent API uses milliseconds).
        #[serde(default)] pub modified_at: i64,
    }

    /// `GET /v1/files?path=` envelope.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct FileListing { #[serde(default)] pub path: String, #[serde(default)] pub entries: Vec<FileEntry> }

    /// `PUT /v1/files/content?path=` result.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct FileWritten { #[serde(default)] pub path: String, #[serde(default)] pub size: u64 }

    /// `{ id, status }` of lifecycle operations and `DELETE /v1/instances/{id}`.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct StatusResponse { #[serde(default)] pub id: String, #[serde(default)] pub status: String }

    /// `GET /v1/instances/{id}/logs`.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct Logs { #[serde(default)] pub logs: String }

    /// `{ "data": [...] }` list envelope.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct List<T> { #[serde(default = "Vec::new")] pub data: Vec<T> }

    /// `{ "error": { code, message, details? } }`.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct ApiError {
        #[serde(default)] pub code: String,
        #[serde(default)] pub message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")] pub details: Option<serde_json::Value>,
    }

    /// Error envelope.
    #[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
    pub struct ApiErrorEnvelope { pub error: ApiError }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "The instance object" from /tmp/warren-docs/API.md, verbatim.
    const INSTANCE_JSON: &str = r#"{
      "id": "ab12cd34ef", "workspace_id": "ws_...", "name": "Production agent",
      "template": "warren-agent", "template_name": "warren-agent", "template_revision": 1,
      "image_ref": "warren/agent:dev", "type": "default", "status": "running", "status_reason": "",
      "resources": { "cpu": 2, "memory": 4, "disk": 4 }, "region": "local",
      "metadata": { "plan": "pro" }, "env": { "APP_ENV": "production" },
      "network": "allow-all", "auto_sleep": true, "idle_timeout_seconds": 900,
      "budget": { "monthly_cap_micros": 0, "credit_micros": 0 },
      "url": "http://ab12cd34ef.warren.localhost:7337", "default_port": 3737, "mount_path": "/home/agent", "agent_api": true,
      "parent_id": "", "created_at": 1763000000, "updated_at": 1763000100, "started_at": 1763000050,
      "stopped_at": 0, "slept_at": 0, "last_activity_at": 1763000100, "meter_started_at": 1763000050,
      "runtime": "docker", "disk_used_bytes": 123456
    }"#;

    #[test]
    fn instance_object_from_api_md_deserialises_and_maps() {
        let w: wire::Instance = serde_json::from_str(INSTANCE_JSON).unwrap();
        assert_eq!(w.kind, "default");
        assert_eq!(w.shape, wire::Shape { cpu: 2, memory: 4, disk: 4 });
        assert_eq!(w.metadata.get("plan").map(String::as_str), Some("pro"));
        let i = WarrenInstance::from(w);
        assert_eq!(i.id, "ab12cd34ef");
        assert_eq!(i.name, "Production agent");
        assert_eq!(i.template, "warren-agent");
        assert_eq!(i.template_revision, 1);
        assert_eq!(i.status, "running");
        assert_eq!(i.resources, WarrenShape { cpu: 2, memory: 4, disk: 4 });
        assert_eq!(i.url, "http://ab12cd34ef.warren.localhost:7337");
        assert_eq!(i.default_port, 3737);
        assert_eq!(i.mount_path, "/home/agent");
        assert!(i.auto_sleep);
        assert_eq!(i.idle_timeout_seconds, 900);
        assert_eq!((i.created_at, i.updated_at, i.last_activity_at), (1763000000, 1763000100, 1763000100));
        assert_eq!(i.disk_used_bytes, 123456);
        assert_eq!(i.runtime, "docker");
        let json = serde_json::to_value(&i).unwrap();
        assert_eq!(json["templateRevision"], 1);
        assert_eq!(json["resources"]["memory"], 4);
        assert_eq!(json["lastActivityAt"], 1763000100);
        assert!(json.get("template_revision").is_none(), "Caret-facing types are camelCase");
    }

    #[test]
    fn instance_tolerates_unknown_and_missing_fields() {
        let w: wire::Instance = serde_json::from_str(r#"{"id":"x","status":"provisioning","brand_new_field":1}"#).unwrap();
        assert_eq!(w.id, "x");
        assert_eq!(w.shape, wire::Shape::default());
    }

    #[test]
    fn requests_serialise_only_set_fields() {
        let req = wire::CreateInstanceRequest { name: Some("n".into()), shape: Some(wire::ShapeRequest { cpu: Some(2), ..Default::default() }), ..Default::default() };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v, serde_json::json!({ "name": "n", "resources": { "cpu": 2 } }));
        let exec = wire::ExecRequest::new("echo ok");
        assert_eq!(serde_json::to_value(&exec).unwrap(), serde_json::json!({ "command": "echo ok" }));
        let exec = wire::ExecRequest { timeout_seconds: 30, workdir: "/w".into(), ..wire::ExecRequest::new("ls") };
        assert_eq!(serde_json::to_value(&exec).unwrap(), serde_json::json!({ "command": "ls", "timeout_seconds": 30, "workdir": "/w" }));
    }

    #[test]
    fn binding_serialises_camel_case_with_defaults() {
        let b = SandboxBinding::new("ab12cd34ef", "caret", "/home/agent/work/caret");
        let v = serde_json::to_value(&b).unwrap();
        assert_eq!(v, serde_json::json!({ "provider": "warren", "instanceId": "ab12cd34ef", "instanceName": "caret", "remoteRoot": "/home/agent/work/caret", "pushedFiles": 0, "status": "ready" }));
        let back: SandboxBinding = serde_json::from_value(serde_json::json!({ "provider": "warren", "instanceId": "a", "instanceName": "b", "remoteRoot": "/r" })).unwrap();
        assert_eq!(back.status, SandboxStatus::Ready);
        assert_eq!(back.pushed_files, 0);
        let err: SandboxBinding = serde_json::from_value(serde_json::json!({ "provider": "warren", "instanceId": "a", "instanceName": "b", "remoteRoot": "/r", "status": "error", "error": "boom", "lastPushAt": 5 })).unwrap();
        assert_eq!(err.status, SandboxStatus::Error);
        assert_eq!(err.last_push_at, Some(5));
    }

    #[test]
    fn list_and_error_envelopes_parse() {
        let l: wire::List<wire::Template> = serde_json::from_str(r#"{"data":[{"name":"warren-base","scope":"system"}]}"#).unwrap();
        assert_eq!(l.data[0].name, "warren-base");
        let e: wire::ApiErrorEnvelope = serde_json::from_str(r#"{"error":{"code":"invalid_api_key","message":"Invalid API key."}}"#).unwrap();
        assert_eq!(e.error.code, "invalid_api_key");
    }
}

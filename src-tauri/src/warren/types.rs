//! PART B STAND-IN for part A's `warren/types.rs`: only the shapes part B touches, with the field names docs/WARREN.md
//! lists. The integrator replaces this file with part A's.
use serde::{Deserialize, Serialize};

/// Per-thread sandbox binding (`Session.sandbox`, mirrored on `ChatSessionView` / `SessionSummary` and TS `SandboxBinding`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxBinding {
    pub provider: String,
    pub instance_id: String,
    pub instance_name: String,
    pub remote_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub last_push_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub last_pull_at: Option<u64>,
    #[serde(default)] pub pushed_files: u64,
    /// ready | syncing | running | error
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub error: Option<String>,
}

/// Subset of Warren's instance object (camelCase on the wire).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WarrenInstance {
    pub id: String,
    pub name: String,
    pub template: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub status_reason: Option<String>,
}

/// `POST /v1/instances/{id}/exec` body; only set fields are sent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExecRequest {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub timeout_seconds: Option<u64>,
}

/// Exec response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub truncated: bool,
}

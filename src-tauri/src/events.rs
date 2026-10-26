//! Low-frequency broadcast event names (mirror `Events` in src/lib/tauri.ts).
use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const FS_CHANGED: &str = "caret://fs/changed";
pub const GIT_CHANGED: &str = "caret://git/changed";
pub const SETTINGS_CHANGED: &str = "caret://settings/changed";
pub const INDEX_STATUS: &str = "caret://index/status";
pub const MCP_CHANGED: &str = "caret://mcp/changed";
pub const NOTIFY: &str = "caret://notify";
pub const WORKSPACE_CHANGED: &str = "caret://workspace/changed";
pub const LSP_STATUS: &str = "caret://lsp/status";
pub const LSP_DIAGNOSTICS: &str = "caret://lsp/diagnostics";
pub const AGENT_EVENT: &str = "caret://agent/event";
pub const REVIEW_CHANGED: &str = "caret://review/changed";
pub const RULES_CHANGED: &str = "caret://rules/changed";
pub const MENU: &str = "caret://menu";
pub const DOCS_CHANGED: &str = "caret://docs/changed";

/// Emit to all windows, swallowing errors (the UI may not be ready yet).
pub fn emit<P: Serialize + Clone>(app: &AppHandle, event: &str, payload: P) {
    if let Err(e) = app.emit(event, payload) {
        tracing::debug!(event, error = %e, "emit failed");
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: String,
    pub level: &'static str,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

pub fn notify(app: &AppHandle, level: &'static str, title: impl Into<String>, message: Option<String>) {
    emit(app, NOTIFY, Notification { id: uuid::Uuid::new_v4().to_string(), level, title: title.into(), message, source: None, timeout_ms: None });
}

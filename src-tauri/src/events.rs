//! Low-frequency broadcast event names (mirror `Events` in src/lib/tauri.ts) and the emit helpers.
//!
//! * `emit` — app-wide: every window's frontend and every Rust `app.listen` listener.
//! * `emit_to` — one window's frontend (the frontend subscribes with its own label as target, see `on` in
//!   src/lib/tauri.ts; Rust listeners do not receive it).
//! * `emit_for_root` — the frontends of every window showing a folder, plus Rust listeners.
//! * `INTERNAL_WORKSPACE_CHANGED` — Rust-only: which window changed folder, from what, and whether the per-root
//!   resources for the new / previous root should start / stop (`on_workspace_change`).
use crate::core::workspace::WorkspaceInfo;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{AppHandle, Emitter, EventTarget, Listener, Manager};

pub const FS_CHANGED: &str = "caret://fs/changed";
pub const GIT_CHANGED: &str = "caret://git/changed";
pub const SETTINGS_CHANGED: &str = "caret://settings/changed";
pub const ROUTER_STATUS: &str = "caret://router/status";
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
/// Rust-only (never reaches a webview): a window opened, switched or closed its folder. Payload: [`WorkspaceChange`].
pub const INTERNAL_WORKSPACE_CHANGED: &str = "caret-internal://workspace/changed";

/// Emit to all windows, swallowing errors (the UI may not be ready yet).
pub fn emit<P: Serialize + Clone>(app: &AppHandle, event: &str, payload: P) {
    if let Err(e) = app.emit(event, payload) {
        tracing::debug!(event, error = %e, "emit failed");
    }
}

/// Emit to one window's frontend only.
pub fn emit_to<P: Serialize + Clone>(app: &AppHandle, label: &str, event: &str, payload: P) {
    if let Err(e) = app.emit_to(EventTarget::labeled(label), event, payload) {
        tracing::debug!(event, label, error = %e, "emit_to failed");
    }
}

/// Emit to the frontend of every window showing `root` (none when no window shows it) and to Rust `app.listen`
/// listeners, which are not per window.
pub fn emit_for_root<P: Serialize + Clone>(app: &AppHandle, root: &Path, event: &str, payload: P) {
    let labels = app.state::<crate::core::windows::WindowRegistry>().labels_for_root(root);
    if let Err(e) = app.emit_filter(event, payload, move |t| root_target_matches(&labels, t)) {
        tracing::debug!(event, root = %root.display(), error = %e, "emit_for_root failed");
    }
}

/// Pure filter behind `emit_for_root`: webview targets whose label is in `labels`, plus the app target (Rust-side
/// listeners). `Any`-targeted listeners are let through by Tauri itself before this filter runs.
pub fn root_target_matches(labels: &[String], target: &EventTarget) -> bool {
    match target {
        EventTarget::App => true,
        EventTarget::Window { label } | EventTarget::Webview { label } | EventTarget::WebviewWindow { label } | EventTarget::AnyLabel { label } => labels.iter().any(|l| l == label),
        _ => false,
    }
}

/// What changed when a window opened, switched or closed its folder (payload of `INTERNAL_WORKSPACE_CHANGED`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceChange {
    /// The window that changed.
    pub label: String,
    /// The folder it shows now (`None`: closed).
    pub info: Option<WorkspaceInfo>,
    /// The folder it showed before (`None`: it was empty).
    pub prev_root: Option<String>,
    /// No other window showed `info.root` before this change: start that root's resources.
    pub first_for_root: bool,
    /// No window shows `prev_root` any more: stop that root's resources.
    pub last_for_prev_root: bool,
}

/// Rust-side broadcast of a workspace change (`EventTarget::App`, so it never reaches a webview).
pub fn emit_internal_workspace_changed(app: &AppHandle, change: WorkspaceChange) {
    if let Err(e) = app.emit_to(EventTarget::App, INTERNAL_WORKSPACE_CHANGED, change) {
        tracing::debug!(error = %e, "internal workspace change emit failed");
    }
}

/// Subscribe to workspace changes of every window. The handler runs inline inside the emitting command, so hand real
/// work to a background task.
pub fn on_workspace_change(app: &AppHandle, f: impl Fn(&AppHandle, WorkspaceChange) + Send + Sync + 'static) -> tauri::EventId {
    let handle = app.clone();
    app.listen(INTERNAL_WORKSPACE_CHANGED, move |ev| match serde_json::from_str::<WorkspaceChange>(ev.payload()) {
        Ok(change) => f(&handle, change),
        Err(e) => tracing::warn!(error = %e, "workspace change payload did not parse"),
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }

    #[test]
    fn root_target_matches_selects_windows_on_the_root_and_rust_listeners() {
        let on_root = labels(&["main", "w3"]);
        assert!(root_target_matches(&on_root, &EventTarget::labeled("main")));
        assert!(root_target_matches(&on_root, &EventTarget::WebviewWindow { label: "w3".into() }));
        assert!(root_target_matches(&on_root, &EventTarget::Webview { label: "w3".into() }));
        assert!(root_target_matches(&on_root, &EventTarget::Window { label: "main".into() }));
        assert!(!root_target_matches(&on_root, &EventTarget::labeled("w2")), "a window on another folder");
        assert!(!root_target_matches(&on_root, &EventTarget::labeled("browser-1")));
        assert!(root_target_matches(&on_root, &EventTarget::App), "Rust app.listen listeners always receive it");
        assert!(!root_target_matches(&on_root, &EventTarget::Any), "Any is handled by Tauri before the filter");
        assert!(!root_target_matches(&[], &EventTarget::labeled("main")), "no window shows the root");
        assert!(root_target_matches(&[], &EventTarget::App));
    }

    #[test]
    fn workspace_change_round_trips_through_json() {
        let c = WorkspaceChange { label: "w2".into(), info: None, prev_root: Some("/a".into()), first_for_root: false, last_for_prev_root: true };
        let s = serde_json::to_string(&c).unwrap();
        assert!(s.contains("\"prevRoot\":\"/a\"") && s.contains("\"lastForPrevRoot\":true"), "{s}");
        let back: WorkspaceChange = serde_json::from_str(&s).unwrap();
        assert_eq!((back.label.as_str(), back.prev_root.as_deref(), back.first_for_root, back.last_for_prev_root), ("w2", Some("/a"), false, true));
        assert!(back.info.is_none());
    }
}

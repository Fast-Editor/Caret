//! Unified error type. Serializes to `{ code, message }` for the UI.
use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaretError {
    pub code: String,
    pub message: String,
}

impl CaretError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
    pub fn not_found(what: impl fmt::Display) -> Self { Self::new("not_found", format!("{what} not found")) }
    pub fn invalid(message: impl Into<String>) -> Self { Self::new("invalid", message) }
    pub fn io(message: impl Into<String>) -> Self { Self::new("io", message) }
    pub fn no_workspace() -> Self { Self::new("no_workspace", "No workspace is open") }
    pub fn cancelled() -> Self { Self::new("cancelled", "Operation cancelled") }
    pub fn provider(message: impl Into<String>) -> Self { Self::new("provider", message) }
    pub fn config(message: impl Into<String>) -> Self { Self::new("config", message) }
    pub fn denied(message: impl Into<String>) -> Self { Self::new("denied", message) }
    pub fn internal(message: impl Into<String>) -> Self { Self::new("internal", message) }
}

impl fmt::Display for CaretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "[{}] {}", self.code, self.message) }
}
impl std::error::Error for CaretError {}

impl From<std::io::Error> for CaretError {
    fn from(e: std::io::Error) -> Self {
        let code = match e.kind() {
            std::io::ErrorKind::NotFound => "not_found",
            std::io::ErrorKind::PermissionDenied => "permission_denied",
            std::io::ErrorKind::AlreadyExists => "already_exists",
            _ => "io",
        };
        Self::new(code, e.to_string())
    }
}
impl From<serde_json::Error> for CaretError { fn from(e: serde_json::Error) -> Self { Self::new("json", e.to_string()) } }
impl From<anyhow::Error> for CaretError { fn from(e: anyhow::Error) -> Self { Self::new("internal", format!("{e:#}")) } }
impl From<reqwest::Error> for CaretError {
    fn from(e: reqwest::Error) -> Self {
        let code = if e.is_timeout() { "timeout" } else if e.is_connect() { "connect" } else { "http" };
        Self::new(code, e.to_string())
    }
}
impl From<git2::Error> for CaretError { fn from(e: git2::Error) -> Self { Self::new("git", e.message().to_string()) } }
impl From<tauri::Error> for CaretError { fn from(e: tauri::Error) -> Self { Self::new("tauri", e.to_string()) } }
impl From<String> for CaretError { fn from(s: String) -> Self { Self::new("error", s) } }
impl From<&str> for CaretError { fn from(s: &str) -> Self { Self::new("error", s) } }

pub type Result<T> = std::result::Result<T, CaretError>;

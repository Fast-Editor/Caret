//! Run and Debug: a Debug Adapter Protocol client. Adapters (js-debug for Node/TypeScript, debugpy for Python) are
//! installed under `~/.caret/debug` and launched per session; events stream to the UI as `caret://debug/event`.
//! See docs/DEBUGGING.md.
pub mod protocol;
pub mod client;
pub mod configs;
pub mod adapters;
pub mod session;
pub mod commands;
#[cfg(test)] mod tests;
pub use commands::*;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;

pub const DEBUG_EVENT: &str = "caret://debug/event";

/// Events pushed to the frontend. Thread / frame / variable ids are **global** (connection-qualified) ids.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DebugEvent {
    SessionStarted { session_id: String, name: String, adapter: String, kind: String },
    Initialized { session_id: String, capabilities: Value },
    Stopped { session_id: String, thread_id: i64, reason: String, #[serde(skip_serializing_if = "Option::is_none")] description: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] text: Option<String>, all_threads_stopped: bool, hit_breakpoint_ids: Vec<i64> },
    Continued { session_id: String, thread_id: i64, all_threads_continued: bool },
    Output { session_id: String, category: String, output: String, #[serde(skip_serializing_if = "Option::is_none")] source: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] line: Option<i64> },
    Breakpoint { session_id: String, reason: String, breakpoint: Breakpoint },
    Thread { session_id: String, reason: String, thread_id: i64 },
    Terminated { session_id: String },
    Exited { session_id: String, exit_code: i64 },
    Error { session_id: String, message: String },
    State { session_id: String, state: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Breakpoint { #[serde(skip_serializing_if = "Option::is_none")] pub id: Option<i64>, pub verified: bool, #[serde(skip_serializing_if = "Option::is_none")] pub line: Option<i64>, #[serde(skip_serializing_if = "Option::is_none")] pub message: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] pub path: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DapThread { pub id: i64, pub name: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackFrame { pub id: i64, pub name: String, #[serde(skip_serializing_if = "Option::is_none")] pub path: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] pub source_name: Option<String>, pub line: i64, pub column: i64, #[serde(skip_serializing_if = "Option::is_none")] pub presentation_hint: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope { pub name: String, pub variables_reference: i64, pub expensive: bool, #[serde(skip_serializing_if = "Option::is_none")] pub presentation_hint: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variable { pub name: String, pub value: String, #[serde(rename = "type", skip_serializing_if = "Option::is_none")] pub kind: Option<String>, pub variables_reference: i64, #[serde(skip_serializing_if = "Option::is_none")] pub evaluate_name: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] pub indexed_variables: Option<i64>, #[serde(skip_serializing_if = "Option::is_none")] pub named_variables: Option<i64> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateResult { pub result: String, #[serde(rename = "type", skip_serializing_if = "Option::is_none")] pub kind: Option<String>, pub variables_reference: i64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugSessionInfo { pub id: String, pub name: String, pub adapter: String, pub kind: String, pub request: String, pub state: String, pub capabilities: Value }

pub type EventSink = Arc<dyn Fn(DebugEvent) + Send + Sync>;
pub type Logger = Arc<dyn Fn(String) + Send + Sync>;

/// Breakpoints the user set, by absolute file path → sorted 1-based lines. Shared by all sessions so a new session
/// receives them on `initialized` and live sessions get `setBreakpoints` on every change.
#[derive(Default)]
pub struct BreakpointStore { pub files: DashMap<String, Vec<u32>> }
impl BreakpointStore {
    pub fn set(&self, file: &str, mut lines: Vec<u32>) { lines.sort_unstable(); lines.dedup(); if lines.is_empty() { self.files.remove(file); } else { self.files.insert(file.to_string(), lines); } }
    pub fn toggle(&self, file: &str, line: u32) -> Vec<u32> {
        let mut lines = self.files.get(file).map(|v| v.clone()).unwrap_or_default();
        if let Some(i) = lines.iter().position(|l| *l == line) { lines.remove(i); } else { lines.push(line); }
        self.set(file, lines.clone()); lines.sort_unstable(); lines
    }
    pub fn all(&self) -> Vec<(String, Vec<u32>)> { self.files.iter().map(|e| (e.key().clone(), e.value().clone())).collect() }
}

/// Tauri-managed debugger state.
pub struct DebugState {
    pub sessions: DashMap<String, Arc<session::DebugSession>>,
    pub breakpoints: Arc<BreakpointStore>,
    pub sink: once_cell::sync::OnceCell<EventSink>,
    logs: parking_lot::Mutex<VecDeque<String>>,
}
impl Default for DebugState { fn default() -> Self { Self { sessions: DashMap::new(), breakpoints: Arc::new(BreakpointStore::default()), sink: once_cell::sync::OnceCell::new(), logs: parking_lot::Mutex::new(VecDeque::new()) } } }
impl DebugState {
    pub fn log(&self, line: impl Into<String>) {
        let line = line.into();
        tracing::debug!(target: "caret::debug", "{line}");
        let mut g = self.logs.lock(); g.push_back(format!("{} {line}", chrono::Local::now().format("%H:%M:%S"))); while g.len() > 500 { g.pop_front(); }
    }
    pub fn logs(&self) -> Vec<String> { self.logs.lock().iter().cloned().collect() }
    pub fn logger(self: &Arc<Self>) -> Logger { let me = self.clone(); Arc::new(move |l| me.log(l)) }
    pub fn emit(&self, ev: DebugEvent) { if let Some(s) = self.sink.get() { s(ev); } }
}

//! Tauri commands for Run and Debug.
use super::adapters::{self, AdapterStatus};
use super::configs::{self, LaunchConfig};
use super::session::DebugSession;
use super::{Breakpoint, DapThread, DebugEvent, DebugSessionInfo, DebugState, EvaluateResult, Scope, StackFrame, Variable};
use crate::core::workspace::WorkspaceState;
use crate::error::{CaretError, Result};
use std::sync::Arc;
use tauri::{AppHandle, State};

fn ensure_sink(app: &AppHandle, st: &DebugState) {
    let _ = st.sink.get_or_init(|| { let app = app.clone(); Arc::new(move |ev: DebugEvent| crate::events::emit(&app, super::DEBUG_EVENT, ev)) });
}
fn info(s: &DebugSession) -> DebugSessionInfo { DebugSessionInfo { id: s.id.clone(), name: s.name.clone(), adapter: s.adapter.clone(), kind: s.kind.clone(), request: s.request.clone(), state: s.state(), capabilities: s.capabilities() } }
fn session(st: &DebugState, id: &str) -> Result<Arc<DebugSession>> { st.sessions.get(id).map(|s| s.clone()).ok_or_else(|| CaretError::not_found(format!("debug session {id}"))) }

#[tauri::command(async)]
pub fn debug_adapters() -> Result<Vec<AdapterStatus>> { Ok(adapters::status_all()) }

#[tauri::command]
pub async fn debug_install_adapter(app: AppHandle, st: State<'_, Arc<DebugState>>, id: String) -> Result<AdapterStatus> {
    ensure_sink(&app, &st);
    adapters::install(&id, st.logger()).await
}

/// Launch configurations for the Start dropdown (`file` = the active editor file for `${file}` defaults).
#[tauri::command(async)]
pub fn debug_list_configs(ws: State<'_, WorkspaceState>, file: Option<String>) -> Result<Vec<LaunchConfig>> {
    Ok(configs::discover(ws.root().as_deref(), file.as_deref()))
}

#[tauri::command]
pub async fn debug_start(app: AppHandle, st: State<'_, Arc<DebugState>>, ws: State<'_, WorkspaceState>, config: LaunchConfig) -> Result<DebugSessionInfo> {
    ensure_sink(&app, &st);
    let root = ws.root();
    let cwd = config.config.get("cwd").and_then(|c| c.as_str()).map(std::path::PathBuf::from).filter(|p| p.is_dir()).or(root.clone()).unwrap_or_else(crate::paths::home_dir);
    let custom = if config.kind == "custom" { config.config.get("adapterCommand").and_then(|c| c.as_str()).map(|c| { let mut parts = c.split_whitespace().map(String::from); (parts.next().unwrap_or_default(), parts.collect::<Vec<_>>()) }) } else { std::env::var("CARET_DAP_ADAPTER").ok().filter(|_| config.kind == "custom").map(|c| { let mut parts = c.split_whitespace().map(String::from); (parts.next().unwrap_or_default(), parts.collect()) }) };
    let adapter = if custom.is_some() { "custom".to_string() } else { adapters::adapter_for_type(&config.kind).ok_or_else(|| CaretError::invalid(format!("no debug adapter for type `{}` (supported: node, python)", config.kind)))?.to_string() };
    let id = uuid::Uuid::new_v4().simple().to_string();
    let sink = st.sink.get().cloned().ok_or_else(|| CaretError::internal("debug sink missing"))?;
    let inner = st.inner().clone();
    let s = DebugSession::start(id.clone(), &adapter, config.config.clone(), &cwd, inner.breakpoints.clone(), sink, inner.logger(), custom).await?;
    inner.sessions.insert(id, s.clone());
    inner.log(format!("[debug] started session {} ({}) via {adapter}", s.name, s.kind));
    Ok(info(&s))
}

#[tauri::command(async)]
pub fn debug_sessions(st: State<'_, Arc<DebugState>>) -> Result<Vec<DebugSessionInfo>> { Ok(st.sessions.iter().map(|e| info(e.value())).collect()) }

#[tauri::command]
pub async fn debug_stop(st: State<'_, Arc<DebugState>>, session_id: String) -> Result<()> {
    let s = session(&st, &session_id)?; s.stop().await; st.sessions.remove(&session_id); Ok(())
}
#[tauri::command] pub async fn debug_continue(st: State<'_, Arc<DebugState>>, session_id: String, thread_id: i64) -> Result<()> { session(&st, &session_id)?.continue_(thread_id).await }
#[tauri::command] pub async fn debug_next(st: State<'_, Arc<DebugState>>, session_id: String, thread_id: i64) -> Result<()> { session(&st, &session_id)?.next(thread_id).await }
#[tauri::command] pub async fn debug_step_in(st: State<'_, Arc<DebugState>>, session_id: String, thread_id: i64) -> Result<()> { session(&st, &session_id)?.step_in(thread_id).await }
#[tauri::command] pub async fn debug_step_out(st: State<'_, Arc<DebugState>>, session_id: String, thread_id: i64) -> Result<()> { session(&st, &session_id)?.step_out(thread_id).await }
#[tauri::command] pub async fn debug_pause(st: State<'_, Arc<DebugState>>, session_id: String, thread_id: i64) -> Result<()> { session(&st, &session_id)?.pause(thread_id).await }
#[tauri::command] pub async fn debug_restart(st: State<'_, Arc<DebugState>>, session_id: String) -> Result<()> { session(&st, &session_id)?.restart().await }
#[tauri::command] pub async fn debug_threads(st: State<'_, Arc<DebugState>>, session_id: String) -> Result<Vec<DapThread>> { session(&st, &session_id)?.threads().await }
#[tauri::command] pub async fn debug_stack_trace(st: State<'_, Arc<DebugState>>, session_id: String, thread_id: i64) -> Result<Vec<StackFrame>> { session(&st, &session_id)?.stack_trace(thread_id).await }
#[tauri::command] pub async fn debug_scopes(st: State<'_, Arc<DebugState>>, session_id: String, frame_id: i64) -> Result<Vec<Scope>> { session(&st, &session_id)?.scopes(frame_id).await }
#[tauri::command] pub async fn debug_variables(st: State<'_, Arc<DebugState>>, session_id: String, variables_reference: i64) -> Result<Vec<Variable>> { session(&st, &session_id)?.variables(variables_reference).await }
#[tauri::command] pub async fn debug_evaluate(st: State<'_, Arc<DebugState>>, session_id: String, expression: String, frame_id: Option<i64>, context: Option<String>) -> Result<EvaluateResult> { session(&st, &session_id)?.evaluate(&expression, frame_id, context.as_deref().unwrap_or("repl")).await }

/// Replace the breakpoints of one file (absolute path) and push them to live sessions. Returns verification info.
#[tauri::command]
pub async fn debug_set_breakpoints(st: State<'_, Arc<DebugState>>, file: String, lines: Vec<u32>) -> Result<Vec<Breakpoint>> {
    st.breakpoints.set(&file, lines.clone());
    let lines = st.breakpoints.files.get(&file).map(|v| v.clone()).unwrap_or_default();
    let sessions: Vec<Arc<DebugSession>> = st.sessions.iter().filter(|e| !e.value().is_terminated()).map(|e| e.value().clone()).collect();
    let mut best: Vec<Breakpoint> = lines.iter().map(|l| Breakpoint { id: None, verified: false, line: Some(*l as i64), message: None, path: Some(file.clone()) }).collect();
    for s in sessions { let r = s.set_breakpoints(&file, &lines).await; if r.iter().filter(|b| b.verified).count() >= best.iter().filter(|b| b.verified).count() { best = r; } }
    Ok(best)
}

#[tauri::command(async)]
pub fn debug_breakpoints(st: State<'_, Arc<DebugState>>) -> Result<Vec<(String, Vec<u32>)>> { Ok(st.breakpoints.all()) }

#[tauri::command(async)]
pub fn debug_logs(st: State<'_, Arc<DebugState>>) -> Result<Vec<String>> { Ok(st.logs()) }

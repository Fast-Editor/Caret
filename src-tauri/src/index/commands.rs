//! Tauri commands for indexing and @Docs. The `index_*` commands act on the calling window's folder; `docs_*` are
//! user-global (documentation sources are not tied to a workspace).
use crate::core::workspace::WorkspaceState;
use crate::error::Result;
use crate::index::docs::{DocSearchHit, DocSource, DocsManager};
use crate::index::hybrid::CodebaseSearchHit;
use crate::index::manager::{IndexManager, IndexStatus};
use std::path::PathBuf;
use tauri::State;

fn root_of(ws: &WorkspaceState, window: &tauri::Window) -> Result<PathBuf> { ws.require_root_for(window.label()) }

#[tauri::command(async)]
pub fn index_status(m: State<'_, IndexManager>, ws: State<'_, WorkspaceState>, window: tauri::Window) -> Result<IndexStatus> { Ok(m.inner.status_for(ws.root_for(window.label()).as_deref())) }
#[tauri::command]
pub async fn index_start(m: State<'_, IndexManager>, ws: State<'_, WorkspaceState>, window: tauri::Window) -> Result<()> {
    let root = root_of(&ws, &window)?;
    let me = m.inner.clone();
    tauri::async_runtime::spawn(async move { if let Ok(ri) = me.ensure_root(&root).await { ri.run_full(false).await; } });
    Ok(())
}
#[tauri::command]
pub fn index_pause(m: State<'_, IndexManager>, ws: State<'_, WorkspaceState>, window: tauri::Window) -> Result<()> {
    if let Some(ri) = m.inner.get(&root_of(&ws, &window)?) { ri.pause(); }
    Ok(())
}
#[tauri::command]
pub fn index_resume(m: State<'_, IndexManager>, ws: State<'_, WorkspaceState>, window: tauri::Window) -> Result<()> {
    if let Some(ri) = m.inner.get(&root_of(&ws, &window)?) { ri.resume(); }
    Ok(())
}
#[tauri::command]
pub async fn index_rebuild(m: State<'_, IndexManager>, ws: State<'_, WorkspaceState>, window: tauri::Window) -> Result<()> {
    let root = root_of(&ws, &window)?;
    let me = m.inner.clone();
    tauri::async_runtime::spawn(async move { if let Ok(ri) = me.ensure_root(&root).await { ri.run_full(true).await; } });
    Ok(())
}
#[tauri::command]
pub async fn index_clear(m: State<'_, IndexManager>, ws: State<'_, WorkspaceState>, window: tauri::Window) -> Result<()> {
    let root = root_of(&ws, &window)?;
    match m.inner.get(&root) { Some(ri) => ri.clear().await, None => Ok(()) }
}
#[tauri::command]
pub async fn index_search(m: State<'_, IndexManager>, ws: State<'_, WorkspaceState>, window: tauri::Window, query: String, limit: Option<usize>, path_prefix: Option<String>) -> Result<Vec<CodebaseSearchHit>> {
    let root = root_of(&ws, &window)?;
    m.inner.ensure_root(&root).await?.search(&query, limit.unwrap_or(10), path_prefix.as_deref()).await
}

#[tauri::command(async)]
pub fn docs_list(d: State<'_, DocsManager>) -> Result<Vec<DocSource>> { Ok(d.list()) }
#[tauri::command]
pub fn docs_add(d: State<'_, DocsManager>, url: String, name: Option<String>, max_pages: Option<u32>, prefix: Option<String>) -> Result<DocSource> { d.add(&url, name.as_deref(), max_pages, prefix.as_deref()) }
#[tauri::command]
pub fn docs_remove(d: State<'_, DocsManager>, doc_id: String) -> Result<()> { d.remove(&doc_id) }
#[tauri::command]
pub fn docs_refresh(d: State<'_, DocsManager>, doc_id: String) -> Result<()> { d.refresh(&doc_id) }
#[tauri::command]
pub async fn docs_search(d: State<'_, DocsManager>, query: String, doc_ids: Option<Vec<String>>, limit: Option<usize>) -> Result<Vec<DocSearchHit>> { d.search(&query, doc_ids.as_deref(), limit.unwrap_or(10)).await }

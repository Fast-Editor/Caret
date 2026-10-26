//! Workspace (open folder) state, recent list, and `caret://workspace/changed` broadcasts.
use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use crate::events;
use crate::paths;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo { pub root: String, pub name: String, pub is_git_repo: bool, pub has_caret_dir: bool, pub has_cursor_dir: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentWorkspace { pub path: String, pub name: String, pub last_opened: u64, pub exists: bool }

#[derive(Default)]
pub struct WorkspaceState { root: RwLock<Option<PathBuf>> }

impl WorkspaceState {
    pub fn root(&self) -> Option<PathBuf> { self.root.read().clone() }
    pub fn require_root(&self) -> Result<PathBuf> { self.root().ok_or_else(CaretError::no_workspace) }
    pub fn info(&self) -> Option<WorkspaceInfo> { self.root().map(|r| info_for(&r)) }
}

pub fn info_for(root: &Path) -> WorkspaceInfo {
    WorkspaceInfo {
        root: root.to_string_lossy().to_string(),
        name: root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| root.to_string_lossy().to_string()),
        is_git_repo: root.join(".git").exists(),
        has_caret_dir: root.join(".caret").is_dir(),
        has_cursor_dir: root.join(".cursor").is_dir(),
    }
}

fn recent_path() -> PathBuf { paths::config_dir().join("recent.json") }
fn now() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) }

fn load_recent() -> Vec<RecentWorkspace> {
    let mut v: Vec<RecentWorkspace> = std::fs::read_to_string(recent_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    for r in &mut v { r.exists = Path::new(&r.path).is_dir(); }
    v
}
fn save_recent(v: &[RecentWorkspace]) { let _ = std::fs::create_dir_all(paths::config_dir()); let _ = std::fs::write(recent_path(), serde_json::to_string_pretty(v).unwrap_or_default()); }
fn touch_recent(root: &Path) {
    let mut v = load_recent();
    let p = root.to_string_lossy().to_string();
    v.retain(|r| r.path != p);
    v.insert(0, RecentWorkspace { path: p, name: root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), last_opened: now(), exists: true });
    v.truncate(30);
    save_recent(&v);
}

/// Open a workspace: updates settings scope, recent list, and broadcasts. Other domains react to the event.
pub fn open_workspace(app: &AppHandle, path: &Path) -> Result<WorkspaceInfo> {
    let root = std::fs::canonicalize(path).map_err(|_| CaretError::not_found(format!("Folder {}", path.display())))?;
    if !root.is_dir() { return Err(CaretError::invalid(format!("{} is not a directory", root.display()))); }
    let ws = app.state::<WorkspaceState>();
    *ws.root.write() = Some(root.clone());
    app.state::<SettingsState>().set_workspace(Some(&root));
    touch_recent(&root);
    let info = info_for(&root);
    events::emit(app, events::WORKSPACE_CHANGED, Some(info.clone()));
    tracing::info!(root = %root.display(), "workspace opened");
    Ok(info)
}

pub fn close_workspace(app: &AppHandle) {
    let ws = app.state::<WorkspaceState>();
    *ws.root.write() = None;
    app.state::<SettingsState>().set_workspace(None);
    events::emit::<Option<WorkspaceInfo>>(app, events::WORKSPACE_CHANGED, None);
}

#[tauri::command]
pub fn workspace_open(app: AppHandle, path: String) -> Result<WorkspaceInfo> { open_workspace(&app, Path::new(&path)) }
#[tauri::command]
pub fn workspace_current(ws: State<'_, WorkspaceState>) -> Result<Option<WorkspaceInfo>> { Ok(ws.info()) }
#[tauri::command]
pub fn workspace_close(app: AppHandle) -> Result<()> { close_workspace(&app); Ok(()) }
#[tauri::command]
pub fn workspace_recent() -> Result<Vec<RecentWorkspace>> { Ok(load_recent()) }
#[tauri::command]
pub fn workspace_remove_recent(path: String) -> Result<()> { let mut v = load_recent(); v.retain(|r| r.path != path); save_recent(&v); Ok(()) }
#[tauri::command]
pub fn workspace_ensure_project_dir(ws: State<'_, WorkspaceState>) -> Result<String> {
    let root = ws.require_root()?;
    let dir = root.join(".caret");
    std::fs::create_dir_all(dir.join("rules"))?;
    Ok(dir.to_string_lossy().to_string())
}

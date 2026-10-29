//! App-level commands: info, process stats, open external, devtools, logging.
use crate::error::Result;
use crate::paths;
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo { pub version: String, pub os: String, pub arch: String, pub config_dir: String, pub data_dir: String, pub webview: String, pub debug: bool, pub home_dir: String }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessStats { pub rss_bytes: u64, pub cpu_percent: f32, pub threads: usize, pub uptime_secs: u64, pub total_memory_bytes: u64 }

pub struct AppStats { sys: Mutex<sysinfo::System>, started: std::time::Instant }
impl Default for AppStats {
    fn default() -> Self { Self { sys: Mutex::new(sysinfo::System::new()), started: std::time::Instant::now() } }
}

#[tauri::command(async)]
pub fn app_info(app: AppHandle) -> Result<AppInfo> {
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        config_dir: paths::config_dir().to_string_lossy().to_string(),
        data_dir: paths::config_dir().to_string_lossy().to_string(),
        webview: tauri::webview_version().unwrap_or_else(|_| "unknown".into()),
        debug: cfg!(debug_assertions),
        home_dir: paths::home_dir().to_string_lossy().to_string(),
    })
}

#[tauri::command(async)]
pub fn app_process_stats(st: State<'_, AppStats>) -> Result<ProcessStats> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, MemoryRefreshKind};
    let mut sys = st.sys.lock();
    let pid = Pid::from_u32(std::process::id());
    sys.refresh_specifics(RefreshKind::nothing().with_memory(MemoryRefreshKind::nothing().with_ram()));
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing().with_memory().with_cpu());
    let (rss, cpu) = sys.process(pid).map(|p| (p.memory(), p.cpu_usage())).unwrap_or((0, 0.0));
    Ok(ProcessStats { rss_bytes: rss, cpu_percent: cpu, threads: 0, uptime_secs: st.started.elapsed().as_secs(), total_memory_bytes: sys.total_memory() })
}

#[tauri::command]
pub fn app_open_external(app: AppHandle, url: String) -> Result<()> {
    app.opener().open_url(url, None::<&str>).map_err(|e| crate::error::CaretError::io(e.to_string()))
}
#[tauri::command]
pub fn app_show_in_folder(app: AppHandle, path: String) -> Result<()> {
    app.opener().reveal_item_in_dir(path).map_err(|e| crate::error::CaretError::io(e.to_string()))
}
#[tauri::command]
pub fn app_open_devtools(app: AppHandle) -> Result<()> {
    if let Some(w) = app.get_webview_window("main") { w.open_devtools(); }
    Ok(())
}
#[tauri::command]
pub fn app_log(level: String, message: String) -> Result<()> {
    match level.as_str() { "error" => tracing::error!(target: "ui", "{message}"), "warn" => tracing::warn!(target: "ui", "{message}"), "debug" => tracing::debug!(target: "ui", "{message}"), _ => tracing::info!(target: "ui", "{message}") }
    Ok(())
}

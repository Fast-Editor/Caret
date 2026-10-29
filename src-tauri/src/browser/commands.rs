use crate::browser::manager::{BrowserManager, BrowserTab};
use crate::core::ports::BrowserSnapshot;
use crate::error::Result;
use tauri::State;

#[tauri::command]
pub async fn browser_open(b: State<'_, BrowserManager>, url: String, label: Option<String>) -> Result<BrowserTab> { b.inner.open(&url, label.as_deref()).await }
#[tauri::command(async)]
pub fn browser_list(b: State<'_, BrowserManager>) -> Result<Vec<BrowserTab>> { Ok(b.inner.list()) }
#[tauri::command]
pub async fn browser_navigate(b: State<'_, BrowserManager>, tab_id: String, url: String) -> Result<()> { b.inner.navigate(&tab_id, &url).await }
#[tauri::command]
pub async fn browser_snapshot(b: State<'_, BrowserManager>, tab_id: String) -> Result<BrowserSnapshot> { b.inner.snapshot(&tab_id).await }
#[tauri::command]
pub async fn browser_click(b: State<'_, BrowserManager>, tab_id: String, r#ref: String) -> Result<()> { b.inner.click(&tab_id, &r#ref).await }
#[tauri::command]
pub async fn browser_type(b: State<'_, BrowserManager>, tab_id: String, r#ref: String, text: String, submit: Option<bool>) -> Result<()> { b.inner.type_text(&tab_id, &r#ref, &text, submit.unwrap_or(false)).await }
#[tauri::command]
pub async fn browser_eval(b: State<'_, BrowserManager>, tab_id: String, js: String) -> Result<String> { b.inner.eval_in(&tab_id, &js).await }
#[tauri::command]
pub async fn browser_back(b: State<'_, BrowserManager>, tab_id: String) -> Result<()> { b.inner.back(&tab_id).await }
#[tauri::command]
pub async fn browser_forward(b: State<'_, BrowserManager>, tab_id: String) -> Result<()> { b.inner.forward(&tab_id).await }
#[tauri::command]
pub fn browser_close(b: State<'_, BrowserManager>, tab_id: String) -> Result<()> { b.inner.close(&tab_id) }

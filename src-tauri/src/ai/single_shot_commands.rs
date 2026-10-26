//! Tauri commands for the single-shot AI features and model utilities.
use crate::ai::{apply, cancel, commit, inline_edit, llm, review, tab};
use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn ai_complete(app: AppHandle, req: tab::CompletionRequest) -> Result<tab::CompletionResult> { tab::complete(&app, req).await }

#[tauri::command]
pub async fn ai_next_edit(app: AppHandle, req: tab::NextEditRequest) -> Result<tab::NextEditResult> { tab::next_edit(&app, req).await }

#[tauri::command]
pub async fn ai_inline_edit(app: AppHandle, req: inline_edit::InlineEditRequest, on_event: Channel<inline_edit::InlineEditEvent>) -> Result<()> {
    match inline_edit::inline_edit(&app, req, on_event).await { Err(e) if e.code == "cancelled" => Ok(()), other => other }
}

#[tauri::command]
pub fn ai_cancel(request_id: String) -> Result<()> { cancel::cancel(&request_id); Ok(()) }

#[tauri::command]
pub async fn ai_terminal_command(app: AppHandle, req: inline_edit::TerminalCommandRequest) -> Result<inline_edit::TerminalCommandResult> { inline_edit::terminal_command(&app, req).await }

#[tauri::command]
pub async fn ai_apply(app: AppHandle, req: apply::ApplyRequest) -> Result<apply::ApplyResult> { apply::apply(&app, req).await }

#[tauri::command]
pub async fn ai_commit_message(app: AppHandle, req: commit::CommitMessageRequest) -> Result<commit::CommitMessageResult> { commit::commit_message(&app, req).await }

#[tauri::command]
pub async fn ai_review(app: AppHandle, req: review::ReviewRequest, on_event: Channel<review::ReviewStreamEvent>) -> Result<String> { review::start_review(app, req, on_event).await }

#[tauri::command]
pub fn ai_review_list(app: AppHandle) -> Result<Vec<review::ReviewResult>> { let root = crate::ai::util::workspace_root(&app); Ok(app.state::<review::ReviewState>().list(root.as_deref())) }

#[tauri::command]
pub fn ai_review_dismiss(app: AppHandle, review_id: String) -> Result<()> { let root = crate::ai::util::workspace_root(&app); app.state::<review::ReviewState>().dismiss(root.as_deref(), &review_id); crate::events::emit(&app, crate::events::REVIEW_CHANGED, ()); Ok(()) }

#[tauri::command]
pub fn ai_review_dismiss_finding(app: AppHandle, review_id: String, finding_id: String) -> Result<()> { let root = crate::ai::util::workspace_root(&app); app.state::<review::ReviewState>().dismiss_finding(root.as_deref(), &review_id, &finding_id); crate::events::emit(&app, crate::events::REVIEW_CHANGED, ()); Ok(()) }

#[tauri::command]
pub async fn models_list_remote(app: AppHandle, provider_id: String) -> Result<Vec<String>> {
    let st = app.state::<SettingsState>();
    let settings = st.effective();
    let provider = settings.provider(&provider_id).cloned().ok_or_else(|| CaretError::not_found(format!("provider {provider_id}")))?;
    let model = settings.models.iter().find(|m| m.provider_id == provider_id).cloned().unwrap_or_else(|| crate::core::settings::ModelConfig { id: "probe".into(), provider_id: provider_id.clone(), model: "probe".into(), display_name: "probe".into(), ..Default::default() });
    let api_key = provider.api_key_ref.as_deref().and_then(|r| st.secret(r));
    let resolved = llm::ResolvedModel { model, provider, api_key };
    let p = llm::provider_for(&resolved)?;
    p.list_models().await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTestResult { pub ok: bool, pub message: String, pub latency_ms: u64 }

#[tauri::command]
pub async fn models_test(app: AppHandle, model_id: String) -> Result<ModelTestResult> {
    let st = app.state::<SettingsState>();
    let settings = st.effective();
    let started = std::time::Instant::now();
    let resolved = match llm::resolve_model(&settings, &st, Some(&model_id)) { Ok(r) => r, Err(e) => return Ok(ModelTestResult { ok: false, message: e.message, latency_ms: 0 }) };
    let mut req = llm::LlmRequest::new(resolved.model.clone()).no_thinking().max_tokens(16).purpose("model-test");
    req.messages = vec![llm::Message::user_text("Reply with exactly: OK")];
    req.cache = false;
    match llm::complete_text(&resolved, req, tokio_util::sync::CancellationToken::new()).await {
        Ok((text, usage)) => Ok(ModelTestResult { ok: true, message: format!("{} responded: {:?} ({} in / {} out tokens)", resolved.model.display_name, text.trim().chars().take(40).collect::<String>(), usage.input_tokens, usage.output_tokens), latency_ms: started.elapsed().as_millis() as u64 }),
        Err(e) => Ok(ModelTestResult { ok: false, message: e.message, latency_ms: started.elapsed().as_millis() as u64 }),
    }
}

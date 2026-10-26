//! Caret core — Tauri application entry. Domain modules register their state and commands here.
#![allow(clippy::too_many_arguments)]

pub mod error;
pub mod events;
pub mod paths;
pub mod tokens;
pub mod menu;
pub mod core;
pub mod commands;
pub mod ai;
pub mod dev;
pub mod verify;
pub mod sandbox;
pub mod acp;
pub mod index;
pub mod mcp;
pub mod browser;
// REGISTER:modules (index, mcp, browser)

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use tauri::Manager;

fn init_tracing() {
    let dir = paths::sub_dir("logs");
    let file = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("caret.log")).ok();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,caret=debug,caret_lib=debug,tantivy=warn,hyper=warn,reqwest=warn"));
    use tracing_subscriber::prelude::*;
    let stderr = tracing_subscriber::fmt::layer().with_target(true).compact();
    let registry = tracing_subscriber::registry().with(filter).with(stderr);
    match file {
        Some(f) => registry.with(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(std::sync::Mutex::new(f))).init(),
        None => registry.init(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting Caret");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_os::init())
        .manage(core::settings::SettingsState::load())
        .manage(core::workspace::WorkspaceState::default())
        .manage(commands::app::AppStats::default())
        .manage(core::diagnostics::DiagnosticsStore::default())
        .manage(verify::VerifyState::default())
        .manage(sandbox::SandboxState::default())
        .manage(acp::AcpState::default())
        .manage(core::fs::FileIndexState::default())
        .manage(core::search::SearchState::default())
        .manage(core::pty::PtyState::default())
        .manage(core::lsp::LspState::default())
        .manage(core::watcher::WatcherState::default())
        .manage(std::sync::Arc::new(ai::agent::manager::AgentManager::new()))
        // REGISTER:state (each domain adds `.manage(...)` lines here)
        .manage(core::cli::OpenQueue::default())
        .manage(ai::tab::TabState::default())
        .manage(ai::review::ReviewState::default())
        .menu(|app| menu::build(app))
        .on_menu_event(|app, ev| menu::on_menu_event(app, ev.id().0.as_str()))
        .setup(|app| {
            let handle = app.handle().clone();
            dev::automation::start(handle.clone());
            core::fs::init(&handle);
            core::watcher::init(&handle);
            core::pty::init(&handle);
            core::lsp::init(&handle);
            core::cli::init(&handle);
            app.state::<std::sync::Arc<ai::agent::manager::AgentManager>>().attach(handle.clone());
            // REGISTER:setup (domain initialisation, e.g. watchers, lsp manager, index manager)
            { let idx = index::IndexManager::new(handle.clone()); idx.install(); app.manage(idx); app.manage(index::DocsManager::new(handle.clone())); }
            { let m = mcp::McpManager::new(handle.clone()); app.manage(m.clone()); m.install(); }
            { let b = browser::BrowserManager::new(handle.clone()); b.install(); app.manage(b); }
            if let Some(w) = app.get_webview_window("main") {
                #[cfg(debug_assertions)]
                if std::env::var("CARET_DEVTOOLS").ok().as_deref() == Some("1") { w.open_devtools(); }
                let _ = w.set_title("Caret");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::app_info, commands::app::app_process_stats, commands::app::app_open_external, commands::app::app_show_in_folder, commands::app::app_open_devtools, commands::app::app_log,
            core::workspace::workspace_open, core::workspace::workspace_current, core::workspace::workspace_close, core::workspace::workspace_recent, core::workspace::workspace_remove_recent, core::workspace::workspace_ensure_project_dir,
            core::settings::settings_get, core::settings::settings_update, core::settings::settings_reset, core::settings::settings_set_secret, core::settings::settings_delete_secret, core::settings::settings_list_secrets, core::settings::settings_import_env_keys, core::settings::settings_path,
            dev::automation::automation_result,
            // REGISTER:commands:verify
            verify::verify_run, verify::verify_get, verify::verify_list, verify::verify_cancel, verify::verify_latest, verify::verify_apply_fix,
            // REGISTER:commands:review
            ai::agent::review_commands::chat_review_comments, ai::agent::review_commands::chat_add_review_comment, ai::agent::review_commands::chat_resolve_review_comment, ai::agent::review_commands::chat_delete_review_comment, ai::agent::review_commands::chat_send_review_comments, ai::agent::review_commands::chat_review_groups,
            // REGISTER:commands:sandbox
            sandbox::sandbox_status, sandbox::sandbox_classify, sandbox::mcp_tool_changes, sandbox::mcp_acknowledge_tool,
            // REGISTER:commands:acp
            acp::acp_list_agents, acp::acp_list_configs, acp::acp_add_agent, acp::acp_remove_agent, acp::acp_toggle_agent, acp::acp_probe, acp::acp_authenticate, acp::acp_set_mode, acp::acp_registry, acp::acp_install, acp::acp_logs, acp::acp_session_info, acp::acp_set_config_option, acp::acp_prepare_session,
            core::fs::fs_read_dir, core::fs::fs_read_file, core::fs::fs_write_file, core::fs::fs_stat, core::fs::fs_exists, core::fs::fs_create, core::fs::fs_rename, core::fs::fs_delete, core::fs::fs_copy, core::fs::fs_move, core::fs::fs_quick_open, core::fs::fs_read_image, core::fs::fs_list_files,
            // REGISTER:commands:fs
            // REGISTER:commands:cli
            core::cli::cli_take_pending_opens, core::cli::cli_status, core::cli::cli_install, core::cli::cli_uninstall,
            core::search::search_text, core::search::search_cancel, core::search::search_replace,
            // REGISTER:commands:search
            core::git::git_status, core::git::git_diff, core::git::git_diff_stat, core::git::git_stage, core::git::git_unstage, core::git::git_stage_all, core::git::git_unstage_all, core::git::git_discard, core::git::git_commit, core::git::git_log, core::git::git_commit_files, core::git::git_show_file, core::git::git_blame, core::git::git_branches, core::git::git_checkout, core::git::git_create_branch, core::git::git_delete_branch, core::git::git_push, core::git::git_pull, core::git::git_fetch, core::git::git_stash, core::git::git_stash_pop, core::git::git_worktrees, core::git::git_worktree_add, core::git::git_worktree_remove, core::git::git_init, core::git::git_remotes, core::git::git_default_branch,
            // REGISTER:commands:git
            core::pty::pty_spawn, core::pty::pty_write, core::pty::pty_resize, core::pty::pty_kill, core::pty::pty_list, core::pty::pty_read_buffer, core::pty::pty_run_command,
            // REGISTER:commands:pty
            core::pty::pty_blocks, core::pty::pty_block_output, core::pty::pty_status,
            core::lsp::lsp_configs, core::lsp::lsp_start, core::lsp::lsp_stop, core::lsp::lsp_send, core::lsp::lsp_status, core::lsp::lsp_diagnostics, core::lsp::lsp_diagnostics_for, core::lsp::lsp_report_diagnostics,
            // REGISTER:commands:lsp
            ai::agent::chat_commands::chat_subscribe, ai::agent::chat_commands::chat_list_sessions, ai::agent::chat_commands::chat_create_session, ai::agent::chat_commands::chat_get_session, ai::agent::chat_commands::chat_delete_session, ai::agent::chat_commands::chat_rename_session, ai::agent::chat_commands::chat_duplicate_session, ai::agent::chat_commands::chat_pin_session, ai::agent::chat_commands::chat_archive_session, ai::agent::chat_commands::chat_export_session, ai::agent::chat_commands::chat_send, ai::agent::chat_commands::chat_stop, ai::agent::chat_commands::chat_approve, ai::agent::chat_commands::chat_answer_question, ai::agent::chat_commands::chat_queue_remove, ai::agent::chat_commands::chat_queue_send_now, ai::agent::chat_commands::chat_accept_changes, ai::agent::chat_commands::chat_reject_changes, ai::agent::chat_commands::chat_pending_changes, ai::agent::chat_commands::chat_change_diff, ai::agent::chat_commands::chat_checkpoints, ai::agent::chat_commands::chat_restore_checkpoint, ai::agent::chat_commands::chat_set_todos, ai::agent::chat_commands::chat_summarize, ai::agent::chat_commands::chat_set_mode, ai::agent::chat_commands::chat_set_model, ai::agent::chat_commands::chat_regenerate, ai::agent::chat_commands::chat_edit_message, ai::agent::chat_commands::chat_list_modes, ai::agent::chat_commands::chat_context_preview, ai::agent::chat_commands::chat_mention_candidates, ai::agent::chat_commands::chat_tool_catalog,
            // REGISTER:commands:chat
            // REGISTER:commands:ai
            ai::single_shot_commands::ai_complete, ai::single_shot_commands::ai_next_edit, ai::single_shot_commands::ai_inline_edit, ai::single_shot_commands::ai_cancel, ai::single_shot_commands::ai_terminal_command, ai::single_shot_commands::ai_apply, ai::single_shot_commands::ai_commit_message, ai::single_shot_commands::ai_review, ai::single_shot_commands::ai_review_list, ai::single_shot_commands::ai_review_dismiss, ai::single_shot_commands::ai_review_dismiss_finding, ai::single_shot_commands::models_list_remote, ai::single_shot_commands::models_test,
            // REGISTER:commands:rules
            ai::agent::knowledge_commands::rules_list, ai::agent::knowledge_commands::rules_read, ai::agent::knowledge_commands::rules_create, ai::agent::knowledge_commands::rules_toggle, ai::agent::knowledge_commands::rules_delete,
            ai::agent::knowledge_commands::memories_list, ai::agent::knowledge_commands::memories_add, ai::agent::knowledge_commands::memories_update, ai::agent::knowledge_commands::memories_delete, ai::agent::knowledge_commands::memories_toggle,
            ai::agent::knowledge_commands::skills_list, ai::agent::knowledge_commands::skills_read, ai::agent::knowledge_commands::commands_list, ai::agent::knowledge_commands::subagents_list, ai::agent::knowledge_commands::hooks_list,
            ai::agent::knowledge_commands::plans_list, ai::agent::knowledge_commands::plans_read, ai::agent::knowledge_commands::plans_set_status, ai::agent::knowledge_commands::plans_update, ai::agent::knowledge_commands::plans_build,
            // REGISTER:commands:index
            index::commands::index_status, index::commands::index_start, index::commands::index_pause, index::commands::index_resume, index::commands::index_rebuild, index::commands::index_clear, index::commands::index_search,
            index::commands::docs_list, index::commands::docs_add, index::commands::docs_remove, index::commands::docs_refresh, index::commands::docs_search,
            // REGISTER:commands:mcp
            mcp::commands::mcp_list_servers, mcp::commands::mcp_list_configs, mcp::commands::mcp_add_server, mcp::commands::mcp_update_server, mcp::commands::mcp_remove_server, mcp::commands::mcp_toggle_server, mcp::commands::mcp_restart_server, mcp::commands::mcp_tools, mcp::commands::mcp_toggle_tool, mcp::commands::mcp_resources, mcp::commands::mcp_read_resource, mcp::commands::mcp_prompts, mcp::commands::mcp_get_prompt, mcp::commands::mcp_call_tool, mcp::commands::mcp_logs,
            // REGISTER:commands:browser
            browser::commands::browser_open, browser::commands::browser_list, browser::commands::browser_navigate, browser::commands::browser_snapshot, browser::commands::browser_click, browser::commands::browser_type, browser::commands::browser_eval, browser::commands::browser_back, browser::commands::browser_forward, browser::commands::browser_close,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Caret")
        .run(|app, event| {
            // `caret <path>` / Finder "Open with" / `caret://open?…` arrive here on macOS (also on cold start).
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            if let tauri::RunEvent::Opened { urls } = &event { core::cli::on_opened(app, urls); }
            #[cfg(not(any(target_os = "macos", target_os = "ios")))]
            let _ = (app, &event);
        });
}

//! Native file dialogs that cannot take the app down.
//!
//! `@tauri-apps/plugin-dialog` creates its panels through generated AppKit bindings that abort the process when
//! `+[NSOpenPanel openPanel]` returns nil. macOS does return nil when the remote-view service behind Open/Save panels
//! is unavailable (`NSXPCSharedListener … 'com.apple.view-bridge'` in the log), which took a whole multi-window Caret
//! down on 2026-11-13. The folder picker therefore goes through this module: on macOS the panel is created with a
//! nil-checked message send and the caller gets a `dialog_unavailable` error (the UI falls back to a typed path);
//! elsewhere the dialog plugin's Rust API is used.
use crate::error::{CaretError, Result};
use std::path::PathBuf;
use tauri::AppHandle;

pub const UNAVAILABLE: &str = "dialog_unavailable";

/// Options for the folder picker.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PickFolderOptions { pub title: Option<String>, pub default_path: Option<String> }

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::msg_send;
    use objc2_foundation::{NSArray, NSString, NSURL};

    /// Must run on the main thread. `Ok(None)` when the user cancelled.
    pub fn pick_folder(opts: &PickFolderOptions) -> std::result::Result<Option<PathBuf>, String> {
        let cls = AnyClass::get(c"NSOpenPanel").ok_or_else(|| "NSOpenPanel class is not available".to_string())?;
        // The one call the dialog plugin cannot survive: a nil panel is an error here, not a panic.
        let panel: Option<Retained<AnyObject>> = unsafe { msg_send![cls, openPanel] };
        let Some(panel) = panel else { return Err("macOS did not provide an Open panel (its view-bridge service is unavailable)".into()) };
        unsafe {
            let _: () = msg_send![&*panel, setCanChooseDirectories: Bool::YES];
            let _: () = msg_send![&*panel, setCanChooseFiles: Bool::NO];
            let _: () = msg_send![&*panel, setAllowsMultipleSelection: Bool::NO];
            let _: () = msg_send![&*panel, setCanCreateDirectories: Bool::YES];
            let _: () = msg_send![&*panel, setResolvesAliases: Bool::YES];
            if let Some(t) = opts.title.as_deref().filter(|t| !t.is_empty()) {
                let t = NSString::from_str(t);
                let _: () = msg_send![&*panel, setMessage: &*t];
                let _: () = msg_send![&*panel, setTitle: &*t];
            }
            if let Some(d) = opts.default_path.as_deref().filter(|d| std::path::Path::new(d).is_dir()) {
                let s = NSString::from_str(d);
                let url = NSURL::fileURLWithPath(&s);
                let _: () = msg_send![&*panel, setDirectoryURL: &*url];
            }
            // NSModalResponseOK == 1
            let response: isize = msg_send![&*panel, runModal];
            if response != 1 { return Ok(None); }
            let urls: Option<Retained<NSArray<NSURL>>> = msg_send![&*panel, URLs];
            let Some(urls) = urls else { return Ok(None) };
            let first = urls.iter().next();
            Ok(first.and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string())))
        }
    }
}

/// Show the folder picker and return the chosen folder (`None` when cancelled). Never panics on a missing panel.
pub async fn pick_folder(app: &AppHandle, opts: PickFolderOptions) -> Result<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = tokio::sync::oneshot::channel::<std::result::Result<Option<PathBuf>, String>>();
        let run = app.run_on_main_thread(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| native::pick_folder(&opts)))
                .unwrap_or_else(|_| Err("the Open panel failed while opening".into()));
            let _ = tx.send(r);
        });
        if let Err(e) = run { return Err(CaretError::new(UNAVAILABLE, format!("could not reach the main thread for the Open panel: {e}"))); }
        match rx.await {
            Ok(Ok(p)) => Ok(p.map(|p| p.to_string_lossy().to_string())),
            Ok(Err(msg)) => { tracing::error!(%msg, "folder picker unavailable"); Err(CaretError::new(UNAVAILABLE, msg)) }
            Err(_) => Err(CaretError::new(UNAVAILABLE, "the Open panel went away before answering")),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_dialog::DialogExt;
        let mut b = app.dialog().file();
        if let Some(t) = opts.title.as_deref() { b = b.set_title(t); }
        if let Some(d) = opts.default_path.as_deref() { b = b.set_directory(d); }
        let picked = tokio::task::spawn_blocking(move || b.blocking_pick_folder()).await.map_err(|e| CaretError::new(UNAVAILABLE, e.to_string()))?;
        Ok(picked.and_then(|f| f.into_path().ok()).map(|p| p.to_string_lossy().to_string()))
    }
}

/// Folder picker for the UI; `dialog_unavailable` tells the frontend to fall back to a typed path.
#[tauri::command]
pub async fn dialog_pick_folder(app: AppHandle, options: Option<PickFolderOptions>) -> Result<Option<String>> {
    pick_folder(&app, options.unwrap_or_default()).await
}

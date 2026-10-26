//! Native application menu. Items emit `caret://menu` with their id; the UI maps ids to commands.
use crate::events;
use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Runtime};

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let item = |id: &str, label: &str, accel: Option<&str>| MenuItem::with_id(app, id, label, true, accel);
    let app_menu = Submenu::with_items(app, "Caret", true, &[
        &PredefinedMenuItem::about(app, Some("About Caret"), Some(AboutMetadata { name: Some("Caret".into()), version: Some(env!("CARGO_PKG_VERSION").into()), ..Default::default() }))?,
        &PredefinedMenuItem::separator(app)?,
        &item("settings.open", "Settings…", Some("CmdOrCtrl+,"))?,
        &item("settings.caret", "Caret Settings…", Some("CmdOrCtrl+Shift+J"))?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::services(app, None)?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::hide(app, None)?,
        &PredefinedMenuItem::hide_others(app, None)?,
        &PredefinedMenuItem::show_all(app, None)?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::quit(app, None)?,
    ])?;
    let file = Submenu::with_items(app, "File", true, &[
        &item("file.newFile", "New File", None)?,
        &item("file.newWindow", "New Window", Some("CmdOrCtrl+Shift+N"))?,
        &PredefinedMenuItem::separator(app)?,
        &item("file.openFolder", "Open Folder…", Some("CmdOrCtrl+O"))?,
        &item("file.openRecent", "Open Recent…", None)?,
        &item("file.closeFolder", "Close Folder", None)?,
        &PredefinedMenuItem::separator(app)?,
        &item("file.save", "Save", None)?,
        &item("file.saveAll", "Save All", None)?,
        &item("file.revealInFinder", "Reveal in Finder", None)?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::close_window(app, None)?,
    ])?;
    let edit = Submenu::with_items(app, "Edit", true, &[
        &PredefinedMenuItem::undo(app, None)?, &PredefinedMenuItem::redo(app, None)?, &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::cut(app, None)?, &PredefinedMenuItem::copy(app, None)?, &PredefinedMenuItem::paste(app, None)?, &PredefinedMenuItem::select_all(app, None)?,
        &PredefinedMenuItem::separator(app)?,
        &item("edit.find", "Find", None)?, &item("edit.replace", "Replace", None)?, &item("edit.findInFiles", "Find in Files", None)?,
    ])?;
    let view = Submenu::with_items(app, "View", true, &[
        &item("view.commandPalette", "Command Palette…", None)?,
        &item("view.quickOpen", "Go to File…", None)?,
        &PredefinedMenuItem::separator(app)?,
        &item("view.explorer", "Explorer", None)?, &item("view.search", "Search", None)?, &item("view.scm", "Source Control", None)?, &item("view.agents", "Agents", None)?,
        &PredefinedMenuItem::separator(app)?,
        &item("view.toggleSidebar", "Toggle Primary Sidebar", None)?, &item("view.toggleChat", "Toggle AI Pane", None)?, &item("view.togglePanel", "Toggle Panel", None)?, &item("view.toggleAgentsLayout", "Toggle Agents Layout", None)?,
        &PredefinedMenuItem::separator(app)?,
        &item("view.zoomIn", "Zoom In", Some("CmdOrCtrl+="))?, &item("view.zoomOut", "Zoom Out", Some("CmdOrCtrl+-"))?, &item("view.zoomReset", "Reset Zoom", Some("CmdOrCtrl+0"))?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::fullscreen(app, None)?,
        &item("view.devtools", "Toggle Developer Tools", Some("Alt+CmdOrCtrl+I"))?,
    ])?;
    let go = Submenu::with_items(app, "Go", true, &[
        &item("go.back", "Back", None)?, &item("go.forward", "Forward", None)?, &PredefinedMenuItem::separator(app)?,
        &item("go.symbol", "Go to Symbol in Editor…", None)?, &item("go.line", "Go to Line…", None)?, &item("go.definition", "Go to Definition", None)?, &item("go.references", "Go to References", None)?,
    ])?;
    let ai = Submenu::with_items(app, "AI", true, &[
        &item("ai.newChat", "New Agent Chat", None)?, &item("ai.toggleChat", "Open Chat", None)?, &item("ai.inlineEdit", "Inline Edit (⌘K)", None)?,
        &item("ai.addSelectionToChat", "Add Selection to Chat", None)?, &PredefinedMenuItem::separator(app)?,
        &item("ai.review", "Review Changes (Bugbot)", None)?, &item("ai.commitMessage", "Generate Commit Message", None)?, &PredefinedMenuItem::separator(app)?,
        &item("ai.toggleTab", "Toggle Tab Completions", None)?, &item("ai.rules", "Rules…", None)?, &item("ai.mcp", "MCP Servers…", None)?, &item("ai.indexing", "Codebase Indexing…", None)?,
    ])?;
    let terminal = Submenu::with_items(app, "Terminal", true, &[
        &item("terminal.new", "New Terminal", None)?, &item("terminal.split", "Split Terminal", None)?, &item("terminal.kill", "Kill Terminal", None)?, &item("terminal.cmdk", "Terminal Cmd+K", None)?,
    ])?;
    let window = Submenu::with_items(app, "Window", true, &[
        &PredefinedMenuItem::minimize(app, None)?, &PredefinedMenuItem::maximize(app, None)?, &PredefinedMenuItem::separator(app)?, &item("window.agents", "Agents Window", None)?,
    ])?;
    let help = Submenu::with_items(app, "Help", true, &[
        &item("help.welcome", "Welcome", None)?, &item("help.shortcuts", "Keyboard Shortcuts", None)?, &item("help.docs", "Documentation", None)?, &item("help.openConfig", "Open Config Folder", None)?, &item("help.about", "About Caret", None)?,
    ])?;
    Menu::with_items(app, &[&app_menu, &file, &edit, &view, &go, &ai, &terminal, &window, &help])
}

pub fn on_menu_event(app: &AppHandle, id: &str) {
    events::emit(app, events::MENU, id.to_string());
}

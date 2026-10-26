//! Agent-driven browser windows (separate Tauri windows). Pages get an injected helper script;
//! results flow back through intercepted `caret-result:` navigations, so remote pages never get IPC.
pub mod manager;
pub mod commands;

pub use manager::BrowserManager;

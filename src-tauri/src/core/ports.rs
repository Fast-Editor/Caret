//! Cross-domain ports: the agent runner consumes these without depending on the concrete
//! index / MCP / browser / LSP implementations. Implementations register at setup via `set_*`.
use crate::error::Result;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeHit { pub path: String, pub start_line: u32, pub end_line: u32, pub score: f32, pub snippet: String, #[serde(default)] pub name: Option<String> }

#[async_trait::async_trait]
pub trait CodebaseSearchPort: Send + Sync {
    async fn search(&self, query: &str, limit: usize, path_prefix: Option<&str>) -> Result<Vec<CodeHit>>;
    fn is_ready(&self) -> bool;
    async fn search_docs(&self, query: &str, doc_ids: Option<&[String]>, limit: usize) -> Result<Vec<DocHit>>;
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocHit { pub doc_id: String, pub url: String, pub title: String, pub snippet: String, pub score: f32 }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolDesc { pub server_id: String, pub server_name: String, pub name: String, pub qualified_name: String, pub description: String, pub input_schema: Value, pub enabled: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourceDesc { pub server_id: String, pub uri: String, pub name: String, pub description: Option<String>, pub mime_type: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpContent { pub r#type: String, #[serde(default)] pub text: Option<String>, #[serde(default)] pub data: Option<String>, #[serde(default)] pub mime_type: Option<String>, #[serde(default)] pub uri: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCallResult { pub content: Vec<McpContent>, pub is_error: bool }

#[async_trait::async_trait]
pub trait McpPort: Send + Sync {
    async fn list_tools(&self) -> Vec<McpToolDesc>;
    async fn call_tool(&self, server_id: &str, name: &str, args: Value) -> Result<McpCallResult>;
    async fn list_resources(&self) -> Vec<McpResourceDesc>;
    async fn read_resource(&self, server_id: &str, uri: &str) -> Result<McpCallResult>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserElement { pub r#ref: String, pub tag: String, pub text: String, #[serde(default)] pub role: Option<String>, #[serde(default)] pub href: Option<String>, #[serde(default)] pub placeholder: Option<String>, #[serde(default)] pub value: Option<String>, #[serde(default)] pub name: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSnapshot { pub url: String, pub title: String, pub text: String, pub elements: Vec<BrowserElement>, pub console_logs: Vec<String> }

#[async_trait::async_trait]
pub trait BrowserPort: Send + Sync {
    async fn open(&self, url: &str) -> Result<String>; // returns tab id
    async fn navigate(&self, tab: &str, url: &str) -> Result<()>;
    async fn snapshot(&self, tab: &str) -> Result<BrowserSnapshot>;
    async fn click(&self, tab: &str, r#ref: &str) -> Result<()>;
    async fn type_text(&self, tab: &str, r#ref: &str, text: &str, submit: bool) -> Result<()>;
    async fn eval(&self, tab: &str, js: &str) -> Result<String>;
    async fn back(&self, tab: &str) -> Result<()>;
    async fn close(&self, tab: &str) -> Result<()>;
    async fn list(&self) -> Vec<(String, String, String)>; // (id, url, title)
}

/// Terminal/PTY buffers exposed to the agent (@Terminal mention, read_terminal tool).
pub trait TerminalPort: Send + Sync {
    fn list(&self) -> Vec<(String, String)>; // (id, title)
    fn read_buffer(&self, id: &str, lines: usize) -> Option<String>;
}

static CODEBASE: OnceCell<Arc<dyn CodebaseSearchPort>> = OnceCell::new();
static MCP: OnceCell<Arc<dyn McpPort>> = OnceCell::new();
static BROWSER: OnceCell<Arc<dyn BrowserPort>> = OnceCell::new();
static TERMINAL: OnceCell<Arc<dyn TerminalPort>> = OnceCell::new();

pub fn set_codebase(p: Arc<dyn CodebaseSearchPort>) { let _ = CODEBASE.set(p); }
pub fn codebase() -> Option<Arc<dyn CodebaseSearchPort>> { CODEBASE.get().cloned() }
pub fn set_mcp(p: Arc<dyn McpPort>) { let _ = MCP.set(p); }
pub fn mcp() -> Option<Arc<dyn McpPort>> { MCP.get().cloned() }
pub fn set_browser(p: Arc<dyn BrowserPort>) { let _ = BROWSER.set(p); }
pub fn browser() -> Option<Arc<dyn BrowserPort>> { BROWSER.get().cloned() }
pub fn set_terminal(p: Arc<dyn TerminalPort>) { let _ = TERMINAL.set(p); }
pub fn terminal() -> Option<Arc<dyn TerminalPort>> { TERMINAL.get().cloned() }

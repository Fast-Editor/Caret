//! MCP client: Cursor-compatible config discovery (.caret/mcp.json, .cursor/mcp.json, ~/.caret/mcp.json,
//! ~/.cursor/mcp.json), connection manager over rmcp (stdio / streamable HTTP), and the `McpPort`.
pub mod config;
pub mod manager;
pub mod commands;

pub use manager::McpManager;

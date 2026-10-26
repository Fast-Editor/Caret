//! Dynamic tools: MCP server tools and browser automation tools (via core::ports).
use super::{arg_bool, arg_str, cap_output, s_bool, s_str, schema, Tool, ToolContext, ToolOutput};
use crate::ai::agent::hooks::{self, HookEvent, HookPermission};
use crate::ai::agent::policy::ToolEffect;
use crate::core::ports::{self, McpToolDesc};
use crate::error::{CaretError, Result};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn sanitize(s: &str) -> String { s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect() }
pub fn mcp_tool_name(server_id: &str, tool: &str) -> String { let n = format!("mcp__{}__{}", sanitize(server_id), sanitize(tool)); n.chars().take(120).collect() }

pub struct McpTool { pub desc: McpToolDesc, pub name: String }
#[async_trait::async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str { &self.name }
    fn display_name(&self) -> &str { &self.desc.name }
    fn description(&self) -> String { format!("[MCP · {}] {}", self.desc.server_name, if self.desc.description.is_empty() { "MCP tool" } else { &self.desc.description }) }
    fn input_schema(&self) -> Value { let mut s = self.desc.input_schema.clone(); if !s.is_object() { s = json!({"type": "object", "properties": {}}); } if s.get("type").is_none() { s["type"] = json!("object"); } if s.get("properties").is_none() { s["properties"] = json!({}); } s }
    fn readonly(&self) -> bool { false }
    fn group(&self) -> &str { "mcp" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::Mcp { server_id: self.desc.server_id.clone(), tool: self.desc.name.clone() } }
    fn label(&self, _i: &Value) -> Option<String> { Some(format!("{} · {}", self.desc.server_name, self.desc.name)) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let Some(port) = ports::mcp() else { return Ok(ToolOutput::error("MCP is not available")) };
        let h = hooks::run(ctx.root.as_deref(), HookEvent::BeforeMcpExecution, json!({ "server": self.desc.server_id, "tool": self.desc.name, "input": input, "session_id": ctx.session_id })).await;
        if h.permission == HookPermission::Deny { return Ok(ToolOutput::error(format!("Blocked by hook{}", h.user_message.map(|m| format!(": {m}")).unwrap_or_default()))); }
        let res = match port.call_tool(&self.desc.server_id, &self.desc.name, input).await { Ok(r) => r, Err(e) => return Ok(ToolOutput::error(format!("MCP tool failed: {}", e.message))) };
        let mut text = String::new(); let mut images = vec![];
        for c in res.content {
            match c.r#type.as_str() {
                "text" => { text.push_str(c.text.as_deref().unwrap_or("")); text.push('\n'); }
                "image" => { if let (Some(d), Some(m)) = (c.data, c.mime_type) { images.push((m, d)); text.push_str("[image]\n"); } }
                "resource" => { text.push_str(&format!("[resource {}]\n{}\n", c.uri.unwrap_or_default(), c.text.unwrap_or_default())); }
                other => text.push_str(&format!("[{other}]\n")),
            }
        }
        let (t, _) = cap_output(text.trim(), ctx.max_output());
        Ok(ToolOutput { text: if t.is_empty() { "(no content)".into() } else { t }, images, is_error: res.is_error, ..Default::default() })
    }
}

pub async fn mcp_tools() -> Vec<Arc<dyn Tool>> {
    let Some(port) = ports::mcp() else { return vec![] };
    let descs = port.list_tools().await;
    // Trust-on-first-use pinning: records description/schema drift so the policy can demand a fresh approval.
    crate::sandbox::mcp_pins::check(&descs);
    descs.into_iter().filter(|t| t.enabled).map(|d| { let name = mcp_tool_name(&d.server_id, &d.name); Arc::new(McpTool { desc: d, name }) as Arc<dyn Tool> }).collect()
}

#[derive(Clone, Copy)]
pub enum BrowserOp { Open, Navigate, Snapshot, Click, Type, Eval, Back, Close }
pub struct BrowserTool(pub BrowserOp);
#[async_trait::async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &str { match self.0 { BrowserOp::Open => "browser_open", BrowserOp::Navigate => "browser_navigate", BrowserOp::Snapshot => "browser_snapshot", BrowserOp::Click => "browser_click", BrowserOp::Type => "browser_type", BrowserOp::Eval => "browser_eval", BrowserOp::Back => "browser_back", BrowserOp::Close => "browser_close" } }
    fn display_name(&self) -> &str { match self.0 { BrowserOp::Open => "Open browser", BrowserOp::Navigate => "Navigate", BrowserOp::Snapshot => "Page snapshot", BrowserOp::Click => "Click", BrowserOp::Type => "Type", BrowserOp::Eval => "Run script", BrowserOp::Back => "Back", BrowserOp::Close => "Close tab" } }
    fn description(&self) -> String { match self.0 {
        BrowserOp::Open => "Open a URL in Caret's built-in browser and return the tab id plus a page snapshot. Use to test web apps you are building (e.g. http://localhost:3000) or read interactive pages.",
        BrowserOp::Navigate => "Navigate an open browser tab to a new URL.",
        BrowserOp::Snapshot => "Get a text snapshot of the current page: title, visible text, and interactive elements with `ref` ids usable by browser_click / browser_type, plus recent console logs.",
        BrowserOp::Click => "Click an element by its `ref` from the latest snapshot.",
        BrowserOp::Type => "Type text into an input/textarea identified by `ref` (optionally press Enter).",
        BrowserOp::Eval => "Execute JavaScript in the page and return the JSON-serialized result (requires approval).",
        BrowserOp::Back => "Go back in the tab's history.",
        BrowserOp::Close => "Close a browser tab." }.into() }
    fn input_schema(&self) -> Value { match self.0 {
        BrowserOp::Open => schema(json!({ "url": s_str("URL to open") }), &["url"]),
        BrowserOp::Navigate => schema(json!({ "tab_id": s_str("Tab id"), "url": s_str("URL") }), &["tab_id", "url"]),
        BrowserOp::Snapshot | BrowserOp::Back | BrowserOp::Close => schema(json!({ "tab_id": s_str("Tab id") }), &["tab_id"]),
        BrowserOp::Click => schema(json!({ "tab_id": s_str("Tab id"), "ref": s_str("Element ref from snapshot") }), &["tab_id", "ref"]),
        BrowserOp::Type => schema(json!({ "tab_id": s_str("Tab id"), "ref": s_str("Element ref"), "text": s_str("Text to type"), "submit": s_bool("Press Enter afterwards") }), &["tab_id", "ref", "text"]),
        BrowserOp::Eval => schema(json!({ "tab_id": s_str("Tab id"), "js": s_str("JavaScript expression/statements") }), &["tab_id", "js"]) } }
    fn readonly(&self) -> bool { false }
    fn group(&self) -> &str { "browser" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::Browser { dangerous: matches!(self.0, BrowserOp::Eval) } }
    fn label(&self, i: &Value) -> Option<String> { arg_str(i, &["url", "ref", "text", "js", "tab_id"]).map(|s| s.chars().take(80).collect()) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let Some(b) = ports::browser() else { return Ok(ToolOutput::error("Browser automation is not available")) };
        let tab = || arg_str(&input, &["tab_id", "tab"]).ok_or_else(|| CaretError::invalid("tab_id is required"));
        let snap = |s: ports::BrowserSnapshot| { let mut t = format!("URL: {}\nTitle: {}\n\n{}\n\nInteractive elements:\n", s.url, s.title, s.text.chars().take(12000).collect::<String>()); for e in s.elements.iter().take(150) { t.push_str(&format!("- [{}] <{}> {}{}{}\n", e.r#ref, e.tag, e.text.chars().take(80).collect::<String>(), e.href.as_ref().map(|h| format!(" href={h}")).unwrap_or_default(), e.placeholder.as_ref().map(|p| format!(" placeholder=\"{p}\"")).unwrap_or_default())); } if !s.console_logs.is_empty() { t.push_str("\nConsole:\n"); for l in s.console_logs.iter().rev().take(20) { t.push_str(&format!("  {l}\n")); } } t };
        let out = match self.0 {
            BrowserOp::Open => { let url = arg_str(&input, &["url"]).ok_or_else(|| CaretError::invalid("url is required"))?; let id = b.open(url).await?; tokio::time::sleep(std::time::Duration::from_millis(1500)).await; let s = b.snapshot(&id).await.map(snap).unwrap_or_else(|e| format!("(snapshot failed: {})", e.message)); format!("Opened tab {id}\n{s}") }
            BrowserOp::Navigate => { let id = tab()?; b.navigate(id, arg_str(&input, &["url"]).ok_or_else(|| CaretError::invalid("url is required"))?).await?; tokio::time::sleep(std::time::Duration::from_millis(1500)).await; b.snapshot(id).await.map(snap).unwrap_or_else(|e| format!("Navigated (snapshot failed: {})", e.message)) }
            BrowserOp::Snapshot => snap(b.snapshot(tab()?).await?),
            BrowserOp::Click => { let id = tab()?; b.click(id, arg_str(&input, &["ref"]).ok_or_else(|| CaretError::invalid("ref is required"))?).await?; tokio::time::sleep(std::time::Duration::from_millis(800)).await; b.snapshot(id).await.map(snap).unwrap_or_else(|_| "Clicked.".into()) }
            BrowserOp::Type => { let id = tab()?; b.type_text(id, arg_str(&input, &["ref"]).ok_or_else(|| CaretError::invalid("ref is required"))?, arg_str(&input, &["text"]).unwrap_or(""), arg_bool(&input, &["submit"]).unwrap_or(false)).await?; tokio::time::sleep(std::time::Duration::from_millis(600)).await; b.snapshot(id).await.map(snap).unwrap_or_else(|_| "Typed.".into()) }
            BrowserOp::Eval => b.eval(tab()?, arg_str(&input, &["js", "script"]).ok_or_else(|| CaretError::invalid("js is required"))?).await?,
            BrowserOp::Back => { let id = tab()?; b.back(id).await?; tokio::time::sleep(std::time::Duration::from_millis(800)).await; b.snapshot(id).await.map(snap).unwrap_or_else(|_| "Went back.".into()) }
            BrowserOp::Close => { b.close(tab()?).await?; "Closed.".into() }
        };
        let (t, _) = cap_output(&out, ctx.max_output());
        Ok(ToolOutput::text(t))
    }
}
pub fn browser_tools() -> Vec<Arc<dyn Tool>> { [BrowserOp::Open, BrowserOp::Navigate, BrowserOp::Snapshot, BrowserOp::Click, BrowserOp::Type, BrowserOp::Eval, BrowserOp::Back, BrowserOp::Close].into_iter().map(|o| Arc::new(BrowserTool(o)) as Arc<dyn Tool>).collect() }

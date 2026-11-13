//! MCP server configuration files and Caret-side enable/disable state.
use crate::error::{CaretError, Result};
use crate::paths;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport { Stdio, Http, Sse }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub transport: McpTransport,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub env: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub headers: Option<BTreeMap<String, String>>,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub cwd: Option<String>,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub disabled_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ServerState { enabled: Option<bool>, disabled_tools: Vec<String> }

fn state_path() -> PathBuf { paths::config_dir().join("mcp-state.json") }
fn load_state() -> BTreeMap<String, ServerState> { std::fs::read(state_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default() }
fn save_state(s: &BTreeMap<String, ServerState>) -> Result<()> { std::fs::create_dir_all(paths::config_dir())?; std::fs::write(state_path(), serde_json::to_vec_pretty(s)?)?; Ok(()) }

/// Expand `${env:VAR}`, `${workspaceFolder}`, `${userHome}` and leading `~`.
pub fn expand(s: &str, root: Option<&Path>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("${") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let Some(j) = after.find('}') else { out.push_str(&rest[i..]); rest = ""; break };
        let key = &after[..j];
        let val = if let Some(v) = key.strip_prefix("env:") { std::env::var(v).unwrap_or_default() }
            else if key == "workspaceFolder" || key == "workspaceRoot" { root.map(|r| r.to_string_lossy().to_string()).unwrap_or_default() }
            else if key == "workspaceFolderBasename" { root.and_then(|r| r.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default() }
            else if key == "userHome" { paths::home_dir().to_string_lossy().to_string() }
            else { format!("${{{key}}}") };
        out.push_str(&val);
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    if let Some(stripped) = out.strip_prefix("~/") { return paths::home_dir().join(stripped).to_string_lossy().to_string(); }
    out
}

fn infer_transport(entry: &Map<String, Value>) -> McpTransport {
    if let Some(t) = entry.get("type").and_then(|v| v.as_str()) {
        match t { "sse" => return McpTransport::Sse, "http" | "streamable-http" | "streamableHttp" | "streamable_http" => return McpTransport::Http, "stdio" => return McpTransport::Stdio, _ => {} }
    }
    if let Some(u) = entry.get("url").and_then(|v| v.as_str()) {
        return if u.trim_end_matches('/').ends_with("/sse") { McpTransport::Sse } else { McpTransport::Http };
    }
    McpTransport::Stdio
}

fn str_map(v: Option<&Value>) -> Option<BTreeMap<String, String>> {
    v.and_then(|v| v.as_object()).map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
}

/// Parse one `mcpServers` file into configs (no expansion applied yet to raw strings except when `expand_root` is Some).
pub fn parse_file(content: &str, source: &str, root: Option<&Path>) -> Vec<McpServerConfig> {
    let Ok(v) = serde_json::from_str::<Value>(content) else { return vec![] };
    let Some(servers) = v.get("mcpServers").and_then(|s| s.as_object()) else { return vec![] };
    servers.iter().filter_map(|(name, e)| {
        let e = e.as_object()?;
        let transport = infer_transport(e);
        let ex = |s: &str| expand(s, root);
        Some(McpServerConfig {
            id: format!("{source}:{name}"),
            name: name.clone(),
            transport,
            command: e.get("command").and_then(|c| c.as_str()).map(ex),
            args: e.get("args").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(ex)).collect()),
            env: str_map(e.get("env")).map(|m| m.into_iter().map(|(k, v)| (k, ex(&v))).collect()),
            url: e.get("url").and_then(|u| u.as_str()).map(ex),
            headers: str_map(e.get("headers")).map(|m| m.into_iter().map(|(k, v)| (k, ex(&v))).collect()),
            enabled: e.get("enabled").and_then(|b| b.as_bool()).unwrap_or(!e.get("disabled").and_then(|b| b.as_bool()).unwrap_or(false)),
            cwd: e.get("cwd").and_then(|c| c.as_str()).map(ex),
            source: source.to_string(),
            disabled_tools: e.get("disabledTools").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()),
        })
    }).collect()
}

pub fn file_for(scope: &str, root: Option<&Path>) -> Option<PathBuf> {
    match scope {
        "project" => root.map(|r| r.join(".caret").join("mcp.json")),
        "user" => Some(paths::config_dir().join("mcp.json")),
        "cursor-project" => root.map(|r| r.join(".cursor").join("mcp.json")),
        "cursor-user" => Some(paths::home_dir().join(".cursor").join("mcp.json")),
        _ => None,
    }
}

/// Scopes that live in a workspace folder (`.caret/mcp.json`, `.cursor/mcp.json`) as opposed to the user's home.
pub fn is_workspace_scope(scope: &str) -> bool { matches!(scope, "project" | "cursor-project") }
/// The scope part of a source-qualified server id (`project:fs` → `project`; a bare id counts as `user`).
pub fn scope_of(id: &str) -> &str { id.split_once(':').map(|(s, _)| s).unwrap_or("user") }

fn load_scopes(scopes: &[&str], root: Option<&Path>) -> Vec<McpServerConfig> {
    let mut out = vec![];
    for scope in scopes {
        if let Some(p) = file_for(scope, root) { if let Ok(c) = std::fs::read_to_string(&p) { out.extend(parse_file(&c, scope, root)); } }
    }
    out
}

/// Servers from the user's files (`~/.caret/mcp.json`, plus `~/.cursor/mcp.json` with Cursor compat). Caret-side
/// enable/disable state is not applied; see `apply_state`.
pub fn load_user(use_cursor_compat: bool) -> Vec<McpServerConfig> {
    let mut scopes = vec!["user"];
    if use_cursor_compat { scopes.push("cursor-user"); }
    load_scopes(&scopes, None)
}

/// Servers from one workspace's files (`<root>/.caret/mcp.json`, plus `<root>/.cursor/mcp.json` with Cursor compat),
/// with `${workspaceFolder}` expanded against that root. Ids are `project:<name>`, so two roots that both define
/// `fs` produce the same id: callers keep one set per root. State is not applied; see `apply_state`.
pub fn load_workspace(root: &Path, use_cursor_compat: bool) -> Vec<McpServerConfig> {
    let mut scopes = vec!["project"];
    if use_cursor_compat { scopes.push("cursor-project"); }
    load_scopes(&scopes, Some(root))
}

/// Apply the Caret-side enable/disable state (`~/.caret/mcp-state.json`, keyed by id) to loaded configs.
pub fn apply_state(configs: &mut [McpServerConfig]) {
    let st = load_state();
    for s in configs.iter_mut() {
        if let Some(x) = st.get(&s.id) {
            if let Some(en) = x.enabled { s.enabled = en; }
            if !x.disabled_tools.is_empty() { let mut d = s.disabled_tools.clone().unwrap_or_default(); for t in &x.disabled_tools { if !d.contains(t) { d.push(t.clone()); } } s.disabled_tools = Some(d); }
        }
    }
}

/// Every server visible from `root`: the user's plus that workspace's (none when `root` is `None`), with state
/// applied. Project overrides user on name clash is NOT applied — ids are source-qualified.
pub fn load_all(root: Option<&Path>, use_cursor_compat: bool) -> Vec<McpServerConfig> {
    let mut out = merge_visible(load_user(use_cursor_compat), root.map(|r| load_workspace(r, use_cursor_compat)).unwrap_or_default());
    apply_state(&mut out);
    out
}

/// The merge rule behind `load_all`: workspace servers first (the order the UI lists them), then the user's.
pub fn merge_visible(user: Vec<McpServerConfig>, workspace: Vec<McpServerConfig>) -> Vec<McpServerConfig> {
    let mut out = workspace;
    out.extend(user);
    out
}

pub fn set_enabled(id: &str, enabled: bool) -> Result<()> { let mut st = load_state(); st.entry(id.to_string()).or_default().enabled = Some(enabled); save_state(&st) }
pub fn set_tool_enabled(id: &str, tool: &str, enabled: bool) -> Result<()> {
    let mut st = load_state(); let e = st.entry(id.to_string()).or_default();
    if enabled { e.disabled_tools.retain(|t| t != tool); } else if !e.disabled_tools.iter().any(|t| t == tool) { e.disabled_tools.push(tool.to_string()); }
    save_state(&st)
}

fn to_entry(c: &McpServerConfig) -> Value {
    let mut m = Map::new();
    match c.transport {
        McpTransport::Stdio => { m.insert("command".into(), Value::String(c.command.clone().unwrap_or_default())); if let Some(a) = &c.args { m.insert("args".into(), serde_json::to_value(a).unwrap_or(Value::Null)); } if let Some(e) = &c.env { if !e.is_empty() { m.insert("env".into(), serde_json::to_value(e).unwrap_or(Value::Null)); } } if let Some(cwd) = &c.cwd { m.insert("cwd".into(), Value::String(cwd.clone())); } }
        McpTransport::Http | McpTransport::Sse => { m.insert("url".into(), Value::String(c.url.clone().unwrap_or_default())); if c.transport == McpTransport::Sse { m.insert("type".into(), Value::String("sse".into())); } if let Some(h) = &c.headers { if !h.is_empty() { m.insert("headers".into(), serde_json::to_value(h).unwrap_or(Value::Null)); } } }
    }
    if !c.enabled { m.insert("enabled".into(), Value::Bool(false)); }
    if let Some(d) = &c.disabled_tools { if !d.is_empty() { m.insert("disabledTools".into(), serde_json::to_value(d).unwrap_or(Value::Null)); } }
    Value::Object(m)
}

/// Write/replace a server in a Caret-owned file (`project` → .caret/mcp.json, `user` → ~/.caret/mcp.json).
pub fn save_server(scope: &str, root: Option<&Path>, c: &McpServerConfig) -> Result<()> {
    let scope = if scope.starts_with("cursor") { if scope == "cursor-project" { "project" } else { "user" } } else { scope };
    let path = file_for(scope, root).ok_or_else(|| CaretError::invalid(format!("unknown scope {scope}")))?;
    let mut doc: Value = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| serde_json::json!({}));
    if !doc.is_object() { doc = serde_json::json!({}); }
    let obj = doc.as_object_mut().unwrap();
    let servers = obj.entry("mcpServers").or_insert_with(|| Value::Object(Map::new()));
    if !servers.is_object() { *servers = Value::Object(Map::new()); }
    servers.as_object_mut().unwrap().insert(c.name.clone(), to_entry(c));
    if let Some(p) = path.parent() { std::fs::create_dir_all(p)?; }
    std::fs::write(&path, serde_json::to_string_pretty(&doc)?)?;
    Ok(())
}

pub fn remove_server(id: &str, root: Option<&Path>) -> Result<()> {
    let (scope, name) = id.split_once(':').ok_or_else(|| CaretError::invalid("bad server id"))?;
    let path = file_for(scope, root).ok_or_else(|| CaretError::invalid(format!("unknown scope {scope}")))?;
    let mut doc: Value = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| serde_json::json!({}));
    if let Some(servers) = doc.get_mut("mcpServers").and_then(|s| s.as_object_mut()) { servers.remove(name); }
    std::fs::write(&path, serde_json::to_string_pretty(&doc)?)?;
    let mut st = load_state(); st.remove(id); save_state(&st)?;
    Ok(())
}

pub fn slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    s.trim_matches('_').to_string()
}
pub fn qualified_tool_name(server_name: &str, tool: &str) -> String { format!("mcp__{}__{}", slug(server_name), tool) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_cursor_format_and_infers_transport() {
        std::env::set_var("CARET_TEST_TOKEN", "abc");
        let json = r#"{"mcpServers": {
            "fs": {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "${workspaceFolder}"], "env": {"TOKEN": "${env:CARET_TEST_TOKEN}"}},
            "remote": {"url": "https://mcp.example.com/mcp", "headers": {"Authorization": "Bearer ${env:CARET_TEST_TOKEN}"}},
            "legacy": {"url": "https://mcp.example.com/sse"},
            "off": {"command": "x", "disabled": true}
        }}"#;
        let root = Path::new("/tmp/proj");
        let v = parse_file(json, "project", Some(root));
        assert_eq!(v.len(), 4);
        let fs = v.iter().find(|s| s.name == "fs").unwrap();
        assert_eq!(fs.transport, McpTransport::Stdio);
        assert_eq!(fs.args.as_ref().unwrap()[2], "/tmp/proj");
        assert_eq!(fs.env.as_ref().unwrap()["TOKEN"], "abc");
        assert_eq!(fs.id, "project:fs");
        assert_eq!(v.iter().find(|s| s.name == "remote").unwrap().transport, McpTransport::Http);
        assert_eq!(v.iter().find(|s| s.name == "remote").unwrap().headers.as_ref().unwrap()["Authorization"], "Bearer abc");
        assert_eq!(v.iter().find(|s| s.name == "legacy").unwrap().transport, McpTransport::Sse);
        assert!(!v.iter().find(|s| s.name == "off").unwrap().enabled);
        assert_eq!(qualified_tool_name("My Server!", "read_file"), "mcp__My_Server__read_file");
    }
    #[test]
    fn expands_placeholders() {
        assert_eq!(expand("${workspaceFolderBasename}/x", Some(Path::new("/a/b"))), "b/x");
        assert!(expand("~/bin", None).ends_with("/bin"));
        assert_eq!(expand("${unknown}", None), "${unknown}");
    }
    #[test]
    fn save_and_remove_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let c = McpServerConfig { id: "project:demo".into(), name: "demo".into(), transport: McpTransport::Stdio, command: Some("node".into()), args: Some(vec!["s.js".into()]), env: None, url: None, headers: None, enabled: true, cwd: None, source: "project".into(), disabled_tools: None };
        save_server("project", Some(root), &c).unwrap();
        let content = std::fs::read_to_string(root.join(".caret/mcp.json")).unwrap();
        let parsed = parse_file(&content, "project", Some(root));
        assert_eq!(parsed.len(), 1); assert_eq!(parsed[0].command.as_deref(), Some("node"));
        remove_server("project:demo", Some(root)).unwrap();
        let content = std::fs::read_to_string(root.join(".caret/mcp.json")).unwrap();
        assert!(parse_file(&content, "project", Some(root)).is_empty());
    }

    fn write_project(root: &Path, servers: &str) {
        std::fs::create_dir_all(root.join(".caret")).unwrap();
        std::fs::write(root.join(".caret/mcp.json"), format!(r#"{{"mcpServers": {{{servers}}}}}"#)).unwrap();
    }
    fn user_cfg(name: &str) -> McpServerConfig {
        McpServerConfig { id: format!("user:{name}"), name: name.into(), transport: McpTransport::Stdio, command: Some("u".into()), args: None, env: None, url: None, headers: None, enabled: true, cwd: None, source: "user".into(), disabled_tools: None }
    }

    #[test]
    fn workspace_configs_are_loaded_per_root_and_merged_with_the_user_layer() {
        let a = tempfile::tempdir().unwrap(); let b = tempfile::tempdir().unwrap();
        write_project(a.path(), r#""fs": {"command": "npx", "args": ["${workspaceFolder}"]}, "only-a": {"command": "a"}"#);
        write_project(b.path(), r#""fs": {"command": "node", "args": ["${workspaceFolder}"]}"#);
        let wa = load_workspace(a.path(), false);
        let wb = load_workspace(b.path(), false);
        let names = |v: &[McpServerConfig]| { let mut n: Vec<String> = v.iter().map(|c| c.id.clone()).collect(); n.sort(); n };
        assert_eq!(names(&wa), vec!["project:fs".to_string(), "project:only-a".to_string()]);
        assert_eq!(names(&wb), vec!["project:fs".to_string()], "b does not see a's servers");
        // the same id in two roots resolves against its own root
        let fs_a = wa.iter().find(|c| c.id == "project:fs").unwrap(); let fs_b = wb.iter().find(|c| c.id == "project:fs").unwrap();
        assert_eq!(fs_a.command.as_deref(), Some("npx")); assert_eq!(fs_b.command.as_deref(), Some("node"));
        assert_eq!(fs_a.args.as_ref().unwrap()[0], a.path().to_string_lossy()); assert_eq!(fs_b.args.as_ref().unwrap()[0], b.path().to_string_lossy());
        assert!(wa.iter().all(|c| is_workspace_scope(&c.source)));
        // a root without a config contributes nothing
        let empty = tempfile::tempdir().unwrap();
        assert!(load_workspace(empty.path(), false).is_empty());
        // merge: the user layer is shared, the workspace layer is the root's own
        let user = vec![user_cfg("github")];
        let va = merge_visible(user.clone(), wa.clone()); let vb = merge_visible(user.clone(), wb.clone());
        assert_eq!(names(&va), vec!["project:fs".to_string(), "project:only-a".to_string(), "user:github".to_string()]);
        assert_eq!(names(&vb), vec!["project:fs".to_string(), "user:github".to_string()]);
        assert_eq!(names(&merge_visible(user, vec![])), vec!["user:github".to_string()], "an empty window sees only user servers");
    }

    #[test]
    fn scope_helpers() {
        assert!(is_workspace_scope("project") && is_workspace_scope("cursor-project"));
        assert!(!is_workspace_scope("user") && !is_workspace_scope("cursor-user") && !is_workspace_scope("other"));
        assert_eq!(scope_of("project:fs"), "project");
        assert_eq!(scope_of("cursor-user:x:y"), "cursor-user");
        assert_eq!(scope_of("legacy"), "user");
    }
}

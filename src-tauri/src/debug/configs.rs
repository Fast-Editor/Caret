//! Launch configurations: `.vscode/launch.json` / `.caret/launch.json` (JSONC), auto-detected defaults, and
//! `${variable}` substitution. Pure functions with tests; no I/O except reading the two files and package.json.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchConfig {
    pub name: String,
    #[serde(rename = "type")] pub kind: String,
    pub request: String,
    /// "launch.json" | "caret" | "auto"
    pub source: String,
    /// The full configuration object as the adapter will receive it (after substitution).
    pub config: Value,
}

/// Strip `//` and `/* */` comments and trailing commas so `serde_json` can parse VS Code's JSONC files.
pub fn strip_jsonc(src: &str) -> String {
    let b = src.as_bytes(); let mut out = String::with_capacity(src.len());
    let mut i = 0; let mut in_str = false; let mut esc = false;
    while i < b.len() {
        let c = b[i] as char;
        if in_str {
            out.push(c);
            if esc { esc = false } else if c == '\\' { esc = true } else if c == '"' { in_str = false }
            i += 1; continue;
        }
        if c == '"' { in_str = true; out.push(c); i += 1; continue; }
        if c == '/' && i + 1 < b.len() && b[i + 1] == b'/' { while i < b.len() && b[i] != b'\n' { i += 1; } continue; }
        if c == '/' && i + 1 < b.len() && b[i + 1] == b'*' { i += 2; while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') { i += 1; } i += 2; continue; }
        out.push(c); i += 1;
    }
    // trailing commas: `,` followed only by whitespace then `}` or `]`
    let mut cleaned = String::with_capacity(out.len()); let cb = out.as_bytes(); let mut j = 0; let mut in_s = false; let mut e2 = false;
    while j < cb.len() {
        let c = cb[j] as char;
        if in_s { cleaned.push(c); if e2 { e2 = false } else if c == '\\' { e2 = true } else if c == '"' { in_s = false } j += 1; continue; }
        if c == '"' { in_s = true; cleaned.push(c); j += 1; continue; }
        if c == ',' {
            let mut k = j + 1; while k < cb.len() && (cb[k] as char).is_whitespace() { k += 1; }
            if k < cb.len() && (cb[k] == b'}' || cb[k] == b']') { j += 1; continue; }
        }
        cleaned.push(c); j += 1;
    }
    cleaned
}

/// Variables available for `${...}` substitution.
#[derive(Debug, Clone, Default)]
pub struct Vars { pub workspace: Option<String>, pub file: Option<String> }

fn subst_str(s: &str, v: &Vars) -> String {
    let mut out = String::with_capacity(s.len()); let mut rest = s;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let Some(end_rel) = rest[start..].find('}') else { out.push_str(&rest[start..]); return out };
        let key = &rest[start + 2..start + end_rel];
        let file = v.file.as_deref().unwrap_or("");
        let ws = v.workspace.as_deref().unwrap_or("");
        let fp = Path::new(file);
        let val: Option<String> = match key {
            "workspaceFolder" | "workspaceRoot" | "cwd" => Some(ws.to_string()),
            "workspaceFolderBasename" => Some(Path::new(ws).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
            "file" => Some(file.to_string()),
            "fileBasename" => Some(fp.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
            "fileBasenameNoExtension" => Some(fp.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
            "fileDirname" => Some(fp.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()),
            "fileExtname" => Some(fp.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default()),
            "relativeFile" => Some(if !ws.is_empty() { fp.strip_prefix(ws).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| file.to_string()) } else { file.to_string() }),
            "pathSeparator" => Some(std::path::MAIN_SEPARATOR.to_string()),
            k if k.starts_with("env:") => Some(std::env::var(&k[4..]).unwrap_or_default()),
            _ => None,
        };
        match val { Some(v) => out.push_str(&v), None => out.push_str(&rest[start..start + end_rel + 1]) }
        rest = &rest[start + end_rel + 1..];
    }
    out.push_str(rest); out
}

/// Substitute variables in every string of a JSON value.
pub fn substitute(v: &Value, vars: &Vars) -> Value {
    match v {
        Value::String(s) => Value::String(subst_str(s, vars)),
        Value::Array(a) => Value::Array(a.iter().map(|x| substitute(x, vars)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), substitute(x, vars))).collect()),
        other => other.clone(),
    }
}

fn configs_from_file(path: &Path, source: &str) -> Vec<LaunchConfig> {
    let Ok(text) = std::fs::read_to_string(path) else { return vec![] };
    let Ok(v) = serde_json::from_str::<Value>(&strip_jsonc(&text)) else { return vec![] };
    let list = v.get("configurations").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    list.into_iter().filter_map(|c| {
        let name = c.get("name")?.as_str()?.to_string();
        let kind = c.get("type")?.as_str()?.to_string();
        let request = c.get("request").and_then(|r| r.as_str()).unwrap_or("launch").to_string();
        Some(LaunchConfig { name, kind, request, source: source.into(), config: c })
    }).collect()
}

/// Everything the Start dropdown should offer: files first, then auto-detected defaults for the current file / project.
pub fn discover(workspace: Option<&Path>, current_file: Option<&str>) -> Vec<LaunchConfig> {
    let mut out = vec![];
    if let Some(ws) = workspace {
        out.extend(configs_from_file(&ws.join(".caret/launch.json"), "caret"));
        out.extend(configs_from_file(&ws.join(".vscode/launch.json"), "launch.json"));
    }
    let ws_s = workspace.map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let ext = current_file.and_then(|f| Path::new(f).extension()).map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let base = current_file.and_then(|f| Path::new(f).file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if matches!(ext.as_str(), "js" | "mjs" | "cjs" | "ts" | "mts" | "cts" | "jsx" | "tsx") {
        out.push(LaunchConfig { name: format!("Run {base} (Node)"), kind: "node".into(), request: "launch".into(), source: "auto".into(),
            config: json!({ "name": format!("Run {base} (Node)"), "type": "node", "request": "launch", "program": "${file}", "cwd": "${workspaceFolder}", "console": "internalConsole", "skipFiles": ["<node_internals>/**"] }) });
    }
    if ext == "py" {
        out.push(LaunchConfig { name: format!("Run {base} (Python)"), kind: "python".into(), request: "launch".into(), source: "auto".into(),
            config: json!({ "name": format!("Run {base} (Python)"), "type": "python", "request": "launch", "program": "${file}", "cwd": "${workspaceFolder}", "console": "internalConsole", "justMyCode": true }) });
    }
    if let Some(ws) = workspace {
        if let Ok(pkg) = std::fs::read_to_string(ws.join("package.json")) {
            if let Ok(v) = serde_json::from_str::<Value>(&pkg) {
                if let Some(scripts) = v.get("scripts").and_then(|s| s.as_object()) {
                    for (name, _) in scripts.iter().take(12) {
                        out.push(LaunchConfig { name: format!("npm run {name}"), kind: "node".into(), request: "launch".into(), source: "auto".into(),
                            config: json!({ "name": format!("npm run {name}"), "type": "node", "request": "launch", "runtimeExecutable": "npm", "runtimeArgs": ["run", name], "cwd": "${workspaceFolder}", "console": "internalConsole", "skipFiles": ["<node_internals>/**"] }) });
                    }
                }
            }
        }
        if ws.join("package.json").exists() || !ext.is_empty() {
            out.push(LaunchConfig { name: "Attach to Node process (port 9229)".into(), kind: "node".into(), request: "attach".into(), source: "auto".into(),
                config: json!({ "name": "Attach to Node process (port 9229)", "type": "node", "request": "attach", "port": 9229, "cwd": "${workspaceFolder}", "skipFiles": ["<node_internals>/**"] }) });
        }
    }
    let vars = Vars { workspace: Some(ws_s).filter(|s| !s.is_empty()), file: current_file.map(|s| s.to_string()) };
    for c in out.iter_mut() { c.config = substitute(&c.config, &vars); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strips_comments_and_trailing_commas_but_not_inside_strings() {
        let src = r#"{
  // version
  "version": "0.2.0", /* block */
  "configurations": [
    { "name": "A // not a comment", "type": "node", "url": "http://x/*y*/", },
  ],
}"#;
        let v: Value = serde_json::from_str(&strip_jsonc(src)).unwrap();
        assert_eq!(v["configurations"][0]["name"], "A // not a comment");
        assert_eq!(v["configurations"][0]["url"], "http://x/*y*/");
    }
    #[test]
    fn substitutes_variables() {
        let vars = Vars { workspace: Some("/ws/proj".into()), file: Some("/ws/proj/src/app.test.ts".into()) };
        let v = substitute(&json!({ "program": "${file}", "cwd": "${workspaceFolder}", "name": "${fileBasenameNoExtension}-${fileExtname}", "dir": "${fileDirname}", "rel": "${relativeFile}", "ws": "${workspaceFolderBasename}", "keep": "${unknownVar}", "arr": ["${fileBasename}"] }), &vars);
        assert_eq!(v["program"], "/ws/proj/src/app.test.ts"); assert_eq!(v["cwd"], "/ws/proj"); assert_eq!(v["name"], "app.test-.ts");
        assert_eq!(v["dir"], "/ws/proj/src"); assert_eq!(v["rel"], "src/app.test.ts"); assert_eq!(v["ws"], "proj"); assert_eq!(v["keep"], "${unknownVar}"); assert_eq!(v["arr"][0], "app.test.ts");
    }
    #[test]
    fn discovers_file_configs_and_defaults() {
        let dir = tempfile::tempdir().unwrap(); let ws = dir.path();
        std::fs::create_dir_all(ws.join(".vscode")).unwrap();
        std::fs::write(ws.join(".vscode/launch.json"), r#"{ "configurations": [ { "name": "Debug server", "type": "node", "request": "launch", "program": "${workspaceFolder}/server.js", }, ] }"#).unwrap();
        std::fs::write(ws.join("package.json"), r#"{"scripts": {"test": "vitest", "dev": "vite"}}"#).unwrap();
        let file = ws.join("src/app.ts").to_string_lossy().to_string();
        let list = discover(Some(ws), Some(&file));
        assert_eq!(list[0].name, "Debug server"); assert_eq!(list[0].source, "launch.json"); assert_eq!(list[0].config["program"], format!("{}/server.js", ws.display()));
        assert!(list.iter().any(|c| c.name == "Run app.ts (Node)" && c.config["program"] == file));
        assert!(list.iter().any(|c| c.name == "npm run test" && c.config["runtimeExecutable"] == "npm"));
        assert!(list.iter().any(|c| c.request == "attach" && c.config["port"] == 9229));
        assert!(!list.iter().any(|c| c.kind == "python"));
    }
}

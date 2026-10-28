//! Helpers shared by the single-shot AI features: model picking, lenient JSON extraction, git CLI.
use crate::ai::llm::{resolve_model, ResolvedModel};
use crate::core::settings::{ModelConfig, ProviderConfig, Settings, SettingsState};
use crate::error::{CaretError, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

pub fn usable(p: &ProviderConfig, secrets: &SettingsState) -> bool {
    p.enabled && p.api_key_ref.as_deref().map(|r| secrets.secret(r).is_some()).unwrap_or(true)
}

/// Pick a model: explicit id → (prefer_fast: tab model, then a usable fast model) → default.
pub fn pick_model(settings: &Settings, secrets: &SettingsState, explicit: Option<&str>, prefer_fast: bool) -> Result<ResolvedModel> {
    if let Some(id) = explicit.filter(|s| !s.is_empty() && *s != "auto") { return resolve_model(settings, secrets, Some(id)); }
    if prefer_fast {
        if let Some(id) = settings.tab.model_id.as_deref() { if let Ok(r) = resolve_model(settings, secrets, Some(id)) { if usable(&r.provider, secrets) { return Ok(r); } } }
        let fast: Vec<&ModelConfig> = settings.models.iter().filter(|m| m.enabled && (m.is_fast == Some(true) || m.tags.as_ref().map(|t| t.iter().any(|x| x == "fast")).unwrap_or(false))).collect();
        for m in fast { if let Ok(r) = resolve_model(settings, secrets, Some(&m.id)) { if usable(&r.provider, secrets) { return Ok(r); } } }
    }
    resolve_model(settings, secrets, None)
}

/// Like `pick_model`, but an Auto preference goes through the Lynkr router when it is enabled, healthy and
/// `router.routeSingleShot` is on. `prefer_fast` requests (Tab, commit, apply, terminal ⌘K) always stay direct.
pub fn pick_model_auto(app: Option<&AppHandle>, settings: &Settings, secrets: &SettingsState, explicit: Option<&str>, prefer_fast: bool, session_id: Option<&str>) -> Result<ResolvedModel> {
    let is_auto = explicit.map(|s| s.is_empty() || s == "auto").unwrap_or(true);
    if is_auto && !prefer_fast && settings.router.route_single_shot {
        if let Some(r) = crate::ai::router::resolve_auto(app, settings, session_id) { return Ok(r); }
    }
    pick_model(settings, secrets, explicit, prefer_fast)
}

pub fn workspace_root(app: &AppHandle) -> Option<PathBuf> { app.state::<crate::core::workspace::WorkspaceState>().root() }

/// Strip a single surrounding markdown fence (```lang … ```), if present.
pub fn strip_fences(text: &str) -> String {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest.strip_suffix("```").unwrap_or(rest);
        let body = match rest.find('\n') { Some(i) => &rest[i + 1..], None => "" };
        return body.trim_end_matches('\n').to_string();
    }
    t.to_string()
}

/// Extract the first balanced JSON object/array from free-form model output.
pub fn extract_json(text: &str) -> Option<Value> {
    let t = strip_fences(text);
    if let Ok(v) = serde_json::from_str::<Value>(t.trim()) { return Some(v); }
    let bytes = t.as_bytes();
    for (start, &b) in bytes.iter().enumerate() {
        if b != b'{' && b != b'[' { continue; }
        let (open, close) = if b == b'{' { (b'{', b'}') } else { (b'[', b']') };
        let mut depth = 0i32; let mut in_str = false; let mut esc = false;
        for (i, &c) in bytes[start..].iter().enumerate() {
            if in_str { if esc { esc = false; } else if c == b'\\' { esc = true; } else if c == b'"' { in_str = false; } continue; }
            match c { b'"' => in_str = true, x if x == open => depth += 1, x if x == close => { depth -= 1; if depth == 0 { if let Ok(v) = serde_json::from_str::<Value>(&t[start..start + i + 1]) { return Some(v); } break; } } _ => {} }
        }
    }
    None
}

/// Extract text between `<tag>` and `</tag>` (lenient: missing close → to end; missing open → whole text).
pub fn extract_tag<'a>(text: &'a str, tag: &str) -> &'a str {
    let open = format!("<{tag}>"); let close = format!("</{tag}>");
    let start = text.find(&open).map(|i| i + open.len()).unwrap_or(0);
    let end = text[start..].find(&close).map(|i| start + i).unwrap_or(text.len());
    &text[start..end]
}

pub async fn run_git(root: &Path, args: &[&str]) -> Result<String> {
    let out = tokio::process::Command::new("git").args(args).current_dir(root).output().await.map_err(|e| CaretError::io(format!("git not available: {e}")))?;
    if !out.status.success() { return Err(CaretError::new("git", String::from_utf8_lossy(&out.stderr).trim().to_string())); }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn read_text_capped(path: &Path, max_bytes: usize) -> Result<(String, bool)> {
    let data = std::fs::read(path)?;
    let truncated = data.len() > max_bytes;
    let slice = if truncated { &data[..max_bytes] } else { &data[..] };
    Ok((String::from_utf8_lossy(slice).to_string(), truncated))
}

pub fn now_ms() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) }

/// Language id from a path extension (for prompts).
pub fn language_for(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
        "rs" => "rust", "ts" | "mts" | "cts" => "typescript", "tsx" => "typescriptreact", "js" | "mjs" | "cjs" => "javascript", "jsx" => "javascriptreact",
        "py" => "python", "go" => "go", "java" => "java", "kt" => "kotlin", "swift" => "swift", "c" | "h" => "c", "cc" | "cpp" | "cxx" | "hpp" => "cpp", "cs" => "csharp",
        "rb" => "ruby", "php" => "php", "sh" | "bash" | "zsh" => "shellscript", "json" => "json", "yml" | "yaml" => "yaml", "toml" => "toml", "md" => "markdown",
        "html" => "html", "css" => "css", "scss" => "scss", "sql" => "sql", "lua" => "lua", "zig" => "zig", "dart" => "dart", "vue" => "vue", "svelte" => "svelte", _ => "plaintext",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn json_extraction_and_fences() {
        assert_eq!(extract_json("Sure!\n```json\n{\"a\": [1,2]}\n```").unwrap()["a"][1], 2);
        assert_eq!(extract_json("text { \"x\": \"}\" } trailing").unwrap()["x"], "}");
        assert!(extract_json("nothing").is_none());
        assert_eq!(strip_fences("```rust\nfn x() {}\n```"), "fn x() {}");
        assert_eq!(extract_tag("pre <code>body</code> post", "code"), "body");
        assert_eq!(extract_tag("no tags", "code"), "no tags");
    }
}

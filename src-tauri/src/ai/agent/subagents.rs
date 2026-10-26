//! Subagent definitions: `<root>/.caret/agents/*.md`, `<root>/.cursor/agents/*.md`, `~/.caret/agents/*.md`.
//! Frontmatter: name, description, model, tools (list), readonly (bool). Body = system prompt.
use crate::ai::agent::frontmatter;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentInfo { pub id: String, pub name: String, pub description: String, pub path: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub model_id: Option<String>, #[serde(default, skip_serializing_if = "Option::is_none")] pub tools: Option<Vec<String>>, pub source: String }

#[derive(Debug, Clone)]
pub struct SubagentDef { pub info: SubagentInfo, pub system_prompt: String, pub readonly: bool }

pub fn search_dirs(root: Option<&Path>) -> Vec<(PathBuf, &'static str)> { search_dirs_with(root, &paths::config_dir()) }
pub fn search_dirs_with(root: Option<&Path>, user_config: &Path) -> Vec<(PathBuf, &'static str)> {
    let mut v = vec![];
    if let Some(r) = root { v.push((r.join(".caret").join("agents"), "project")); v.push((r.join(".cursor").join("agents"), "cursor")); }
    v.push((user_config.join("agents"), "user"));
    v
}

fn parse_def(file: &Path, source: &str) -> Option<SubagentDef> {
    if file.extension().and_then(|e| e.to_str()) != Some("md") { return None; }
    let stem = file.file_stem()?.to_string_lossy().to_string();
    let text = std::fs::read_to_string(file).ok()?;
    let (fm, body) = frontmatter::parse(&text);
    let fm = fm.unwrap_or_default();
    let name = fm.get_str("name").filter(|s| !s.trim().is_empty()).unwrap_or_else(|| stem.clone());
    let description = fm.get_str("description").filter(|s| !s.trim().is_empty()).or_else(|| frontmatter::first_heading(body)).unwrap_or_else(|| name.clone());
    let model_id = fm.get_str("model").or_else(|| fm.get_str("modelId")).filter(|s| !s.trim().is_empty() && s != "inherit");
    let tools = { let t = fm.get_list("tools"); if t.is_empty() { None } else { Some(t) } };
    let readonly = fm.get_bool("readonly").or_else(|| fm.get_bool("read-only")).unwrap_or(false);
    Some(SubagentDef { info: SubagentInfo { id: format!("{source}:{stem}"), name, description, path: file.to_string_lossy().to_string(), model_id, tools, source: source.into() }, system_prompt: body.trim().to_string(), readonly })
}

pub fn list_defs_in(dirs: &[(PathBuf, &'static str)]) -> Vec<SubagentDef> {
    let mut out: Vec<SubagentDef> = vec![];
    for (dir, source) in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        for f in files { if let Some(d) = parse_def(&f, source) { if !out.iter().any(|x| x.info.name.eq_ignore_ascii_case(&d.info.name)) { out.push(d); } } }
    }
    out
}
pub fn list(root: Option<&Path>) -> Vec<SubagentInfo> { list_defs_in(&search_dirs(root)).into_iter().map(|d| d.info).collect() }

pub fn get(root: Option<&Path>, id: &str) -> Option<SubagentDef> { get_in(&search_dirs(root), id) }
pub fn get_in(dirs: &[(PathBuf, &'static str)], id: &str) -> Option<SubagentDef> {
    let all = list_defs_in(dirs);
    all.iter().find(|d| d.info.id == id).or_else(|| all.iter().find(|d| d.info.name.eq_ignore_ascii_case(id))).or_else(|| all.iter().find(|d| d.info.id.split(':').nth(1).map(|s| s.eq_ignore_ascii_case(id)).unwrap_or(false))).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_definitions() {
        let tmp = tempfile::tempdir().unwrap(); let root = tmp.path().join("ws"); let user = tmp.path().join("cfg");
        let w = |p: PathBuf, s: &str| { std::fs::create_dir_all(p.parent().unwrap()).unwrap(); std::fs::write(p, s).unwrap(); };
        w(root.join(".caret/agents/reviewer.md"), "---\nname: Code Reviewer\ndescription: Reviews diffs for bugs\nmodel: claude-sonnet-5-5\ntools: [read_file, grep_search]\nreadonly: true\n---\nYou review code.\n");
        w(root.join(".cursor/agents/explorer.md"), "# Explorer\nFind things fast.");
        w(user.join("agents/reviewer.md"), "---\nname: code reviewer\n---\nshadowed by project");
        let dirs = search_dirs_with(Some(&root), &user);
        let all = list_defs_in(&dirs);
        assert_eq!(all.len(), 2);
        let r = get_in(&dirs, "Code Reviewer").unwrap();
        assert_eq!(r.info.id, "project:reviewer"); assert_eq!(r.info.model_id.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(r.info.tools.as_ref().unwrap(), &vec!["read_file".to_string(), "grep_search".to_string()]); assert!(r.readonly); assert_eq!(r.system_prompt, "You review code.");
        let e = get_in(&dirs, "explorer").unwrap();
        assert_eq!(e.info.description, "Explorer"); assert!(!e.readonly); assert!(e.info.tools.is_none());
        assert!(get_in(&dirs, "missing").is_none());
    }
}

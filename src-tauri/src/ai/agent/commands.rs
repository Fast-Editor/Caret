//! Slash commands: `<root>/.caret/commands/*.md`, `<root>/.cursor/commands/*.md`, `~/.caret/commands/*.md` + builtins.
use crate::ai::agent::frontmatter;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommandInfo { pub name: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub description: Option<String>, #[serde(default, skip_serializing_if = "Option::is_none")] pub path: Option<String>, pub source: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub argument_hint: Option<String> }

pub const BUILTINS: &[(&str, &str, Option<&str>)] = &[
    ("agent", "Switch this chat to Agent mode", None),
    ("ask", "Switch this chat to Ask mode (read-only)", None),
    ("plan", "Switch to Plan mode and write a plan before building", Some("[goal]")),
    ("debug", "Switch to Debug mode (hypothesis-driven debugging)", Some("[symptom]")),
    ("summarize", "Summarize the conversation so far to free context", None),
    ("review", "Review the current changes for bugs (Bugbot)", Some("[base branch]")),
    ("commit", "Generate a commit message for staged changes", None),
    ("clear-context", "Drop attached context items from this chat", None),
    ("help", "List available slash commands", None),
];

pub fn search_dirs(root: Option<&Path>) -> Vec<(PathBuf, &'static str)> { search_dirs_with(root, &paths::config_dir()) }
pub fn search_dirs_with(root: Option<&Path>, user_config: &Path) -> Vec<(PathBuf, &'static str)> {
    let mut v = vec![];
    if let Some(r) = root { v.push((r.join(".caret").join("commands"), "project")); v.push((r.join(".cursor").join("commands"), "cursor")); }
    v.push((user_config.join("commands"), "user"));
    v
}

fn parse_cmd(file: &Path, source: &str) -> Option<SlashCommandInfo> {
    if file.extension().and_then(|e| e.to_str()) != Some("md") { return None; }
    let name = file.file_stem()?.to_string_lossy().to_string();
    let text = std::fs::read_to_string(file).ok()?;
    let (fm, body) = frontmatter::parse(&text);
    let fm = fm.unwrap_or_default();
    let description = fm.get_str("description").filter(|s| !s.trim().is_empty()).or_else(|| body.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| l.trim_start_matches('#').trim().chars().take(140).collect()));
    let argument_hint = fm.get_str("argument-hint").or_else(|| fm.get_str("argumentHint")).or_else(|| fm.get_str("args")).filter(|s| !s.trim().is_empty());
    Some(SlashCommandInfo { name, description, path: Some(file.to_string_lossy().to_string()), source: source.into(), argument_hint })
}

pub fn list_in(dirs: &[(PathBuf, &'static str)]) -> Vec<SlashCommandInfo> {
    let mut out: Vec<SlashCommandInfo> = vec![];
    for (dir, source) in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        for f in files { if let Some(c) = parse_cmd(&f, source) { if !out.iter().any(|x| x.name == c.name) { out.push(c); } } }
    }
    for (name, desc, hint) in BUILTINS { if !out.iter().any(|x| &x.name == name) { out.push(SlashCommandInfo { name: name.to_string(), description: Some(desc.to_string()), path: None, source: "builtin".into(), argument_hint: hint.map(String::from) }); } }
    out
}
pub fn list(root: Option<&Path>) -> Vec<SlashCommandInfo> { list_in(&search_dirs(root)) }

/// Expand a user-defined command into prompt text. Builtins return None (the runner handles them).
pub fn expand(root: Option<&Path>, name: &str, args: &str) -> Option<String> { expand_in(&search_dirs(root), name, args) }
pub fn expand_in(dirs: &[(PathBuf, &'static str)], name: &str, args: &str) -> Option<String> {
    let name = name.trim_start_matches('/');
    let cmd = list_in(dirs).into_iter().find(|c| c.name == name)?;
    let path = cmd.path?;
    let text = std::fs::read_to_string(path).ok()?;
    let (_, body) = frontmatter::parse(&text);
    Some(substitute(body, args))
}

pub fn substitute(template: &str, args: &str) -> String {
    let args = args.trim();
    let parts: Vec<&str> = args.split_whitespace().collect();
    let mut out = String::with_capacity(template.len() + args.len());
    let mut used_placeholder = false;
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' {
            let rest: String = chars[i + 1..].iter().take(9).collect();
            if rest.starts_with("ARGUMENTS") { out.push_str(args); i += 10; used_placeholder = true; continue; }
            if let Some(d) = chars.get(i + 1).and_then(|c| c.to_digit(10)) { if d >= 1 { out.push_str(parts.get(d as usize - 1).copied().unwrap_or("")); i += 2; used_placeholder = true; continue; } }
        }
        out.push(chars[i]); i += 1;
    }
    let mut out = out.trim_end().to_string();
    if !used_placeholder && !args.is_empty() { out.push_str("\n\n"); out.push_str(args); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lists_and_expands() {
        let tmp = tempfile::tempdir().unwrap(); let root = tmp.path().join("ws"); let user = tmp.path().join("cfg");
        let w = |p: PathBuf, s: &str| { std::fs::create_dir_all(p.parent().unwrap()).unwrap(); std::fs::write(p, s).unwrap(); };
        w(root.join(".caret/commands/fix-tests.md"), "---\ndescription: Fix failing tests\nargument-hint: <test name>\n---\nRun the suite and fix $ARGUMENTS. Focus on $1 first.");
        w(root.join(".cursor/commands/fix-tests.md"), "shadowed");
        w(root.join(".cursor/commands/explain.md"), "# Explain the selection\nExplain it simply.");
        w(user.join("commands/standup.md"), "Write a standup note.");
        let dirs = search_dirs_with(Some(&root), &user);
        let all = list_in(&dirs);
        let ft = all.iter().find(|c| c.name == "fix-tests").unwrap();
        assert_eq!(ft.source, "project"); assert_eq!(ft.argument_hint.as_deref(), Some("<test name>"));
        assert_eq!(all.iter().find(|c| c.name == "explain").unwrap().description.as_deref(), Some("Explain the selection"));
        assert!(all.iter().any(|c| c.name == "standup" && c.source == "user"));
        assert!(all.iter().any(|c| c.name == "plan" && c.source == "builtin"));
        assert_eq!(expand_in(&dirs, "/fix-tests", "auth login"), Some("Run the suite and fix auth login. Focus on auth first.".into()));
        assert_eq!(expand_in(&dirs, "standup", "for Monday"), Some("Write a standup note.\n\nfor Monday".into()));
        assert_eq!(expand_in(&dirs, "plan", "x"), None);
        assert_eq!(expand_in(&dirs, "nope", ""), None);
    }
}

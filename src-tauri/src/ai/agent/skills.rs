//! Skills: `<root>/.caret/skills/*/SKILL.md`, `<root>/.cursor/skills/*/SKILL.md`, `~/.caret/skills/*/SKILL.md`, `~/.claude/skills/*/SKILL.md`.
use crate::ai::agent::frontmatter;
use crate::error::{CaretError, Result};
use crate::paths;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo { pub id: String, pub name: String, pub description: String, pub path: String, pub source: String }

pub fn search_dirs(root: Option<&Path>) -> Vec<(PathBuf, &'static str)> { search_dirs_with(root, &paths::config_dir(), &paths::home_dir().join(".claude")) }
pub fn search_dirs_with(root: Option<&Path>, user_config: &Path, claude_dir: &Path) -> Vec<(PathBuf, &'static str)> {
    let mut v = vec![];
    if let Some(r) = root { v.push((r.join(".caret").join("skills"), "project")); v.push((r.join(".cursor").join("skills"), "cursor")); }
    v.push((user_config.join("skills"), "user"));
    v.push((claude_dir.join("skills"), "claude"));
    v
}

fn skill_from_dir(dir: &Path, source: &str) -> Option<SkillInfo> {
    let file = dir.join("SKILL.md");
    let text = std::fs::read_to_string(&file).ok()?;
    let (fm, body) = frontmatter::parse(&text);
    let fm = fm.unwrap_or_default();
    let dir_name = dir.file_name()?.to_string_lossy().to_string();
    let name = fm.get_str("name").filter(|s| !s.trim().is_empty()).or_else(|| frontmatter::first_heading(body)).unwrap_or_else(|| dir_name.clone());
    let description = fm.get_str("description").filter(|s| !s.trim().is_empty()).unwrap_or_else(|| body.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).unwrap_or("").chars().take(200).collect());
    Some(SkillInfo { id: format!("{source}:{dir_name}"), name, description, path: file.to_string_lossy().to_string(), source: source.into() })
}

pub fn list_in(dirs: &[(PathBuf, &'static str)]) -> Vec<SkillInfo> {
    let mut out: Vec<SkillInfo> = vec![];
    for (dir, source) in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
        entries.sort();
        for d in entries { if let Some(s) = skill_from_dir(&d, source) { if !out.iter().any(|x| x.name == s.name) { out.push(s); } } }
    }
    out
}
pub fn list(root: Option<&Path>) -> Vec<SkillInfo> { list_in(&search_dirs(root)) }

fn find(root: Option<&Path>, skill_id: &str) -> Option<SkillInfo> {
    let all = list(root);
    all.iter().find(|s| s.id == skill_id).or_else(|| all.iter().find(|s| s.name.eq_ignore_ascii_case(skill_id))).or_else(|| all.iter().find(|s| s.id.split(':').nth(1) == Some(skill_id))).cloned()
}

/// Full SKILL.md plus a listing of the skill's other files (relative paths) so the agent can `read_file` them.
pub fn read(root: Option<&Path>, skill_id: &str) -> Result<String> {
    let s = find(root, skill_id).ok_or_else(|| CaretError::not_found(format!("Skill {skill_id}")))?;
    read_file(Path::new(&s.path))
}
pub fn read_file(skill_md: &Path) -> Result<String> {
    let text = std::fs::read_to_string(skill_md)?;
    let dir = skill_md.parent().unwrap_or(Path::new("."));
    let mut files: Vec<String> = walkdir::WalkDir::new(dir).max_depth(4).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file() && e.file_name() != "SKILL.md").take(200)
        .map(|e| e.path().strip_prefix(dir).map(|p| p.to_string_lossy().to_string()).unwrap_or_default()).collect();
    files.sort();
    let mut out = text;
    if !files.is_empty() {
        out.push_str(&format!("\n\n## Files in this skill (directory: {})\n", dir.display()));
        for f in files { out.push_str(&format!("- {f}\n")); }
    }
    Ok(out)
}

/// Compact index for the system prompt (≤ 60 entries).
pub fn prompt_index(root: Option<&Path>) -> Option<String> { prompt_index_from(&list(root)) }
pub fn prompt_index_from(skills: &[SkillInfo]) -> Option<String> {
    if skills.is_empty() { return None; }
    let mut s = String::from("## Skills (call the `skill` tool with the name to load instructions)\n");
    for sk in skills.iter().take(60) { s.push_str(&format!("- {}: {}\n", sk.name, sk.description.lines().next().unwrap_or("").chars().take(160).collect::<String>())); }
    Some(s.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovers_reads_and_indexes_skills() {
        let tmp = tempfile::tempdir().unwrap(); let root = tmp.path().join("ws"); let user = tmp.path().join("cfg"); let claude = tmp.path().join("claude");
        let w = |p: PathBuf, s: &str| { std::fs::create_dir_all(p.parent().unwrap()).unwrap(); std::fs::write(p, s).unwrap(); };
        w(root.join(".caret/skills/deploy/SKILL.md"), "---\nname: deploy\ndescription: Deploy the service safely\n---\n# Deploy\nsteps");
        w(root.join(".caret/skills/deploy/scripts/run.sh"), "echo hi");
        w(root.join(".cursor/skills/review-pr/SKILL.md"), "# Review PR\nReview a pull request thoroughly.\n");
        w(user.join("skills/deploy/SKILL.md"), "---\nname: deploy\ndescription: user dup\n---\n");
        w(claude.join("skills/docx/SKILL.md"), "---\nname: docx\ndescription: Word documents\n---\nbody");
        let dirs = search_dirs_with(Some(&root), &user, &claude);
        let all = list_in(&dirs);
        let names: Vec<_> = all.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["deploy", "Review PR", "docx"]);
        assert_eq!(all[0].source, "project"); assert_eq!(all[1].description, "Review a pull request thoroughly."); assert_eq!(all[2].source, "claude");
        let text = read_file(Path::new(&all[0].path)).unwrap();
        assert!(text.contains("# Deploy") && text.contains("- scripts/run.sh"));
        let idx = prompt_index_from(&all).unwrap();
        assert!(idx.starts_with("## Skills") && idx.contains("- deploy: Deploy the service safely") && idx.contains("- docx: Word documents"));
        assert!(prompt_index_from(&[]).is_none());
    }
}

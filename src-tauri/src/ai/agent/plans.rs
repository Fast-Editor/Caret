//! Plan files: `<root>/.caret/plans/*.md` (and `.cursor/plans` read-only compat).
//! Frontmatter: title, status (draft|approved|building|done), session, created, updated.
use crate::ai::agent::frontmatter::{self, FmValue};
use crate::error::{CaretError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInfo { pub path: String, pub title: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub session_id: Option<String>, pub updated_at: u64, pub status: String }

pub const STATUSES: [&str; 4] = ["draft", "approved", "building", "done"];

fn now_ms() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) }
fn iso_now() -> String { chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true) }
fn parse_time(s: Option<String>, fallback: u64) -> u64 { s.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok()).map(|d| d.timestamp_millis() as u64).unwrap_or(fallback) }

pub fn plans_dir(root: &Path) -> PathBuf { root.join(".caret").join("plans") }

fn info_for(file: &Path) -> Option<PlanInfo> {
    let text = std::fs::read_to_string(file).ok()?;
    let (fm, body) = frontmatter::parse(&text);
    let fm = fm.unwrap_or_default();
    let mtime = std::fs::metadata(file).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
    let title = fm.get_str("title").filter(|s| !s.trim().is_empty()).or_else(|| frontmatter::first_heading(body)).unwrap_or_else(|| file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
    let status = fm.get_str("status").map(|s| s.to_lowercase()).filter(|s| STATUSES.contains(&s.as_str())).unwrap_or_else(|| "draft".into());
    Some(PlanInfo { path: file.to_string_lossy().to_string(), title, session_id: fm.get_str("session").or_else(|| fm.get_str("sessionId")).filter(|s| !s.is_empty()), updated_at: parse_time(fm.get_str("updated"), mtime), status })
}

pub fn list(root: Option<&Path>) -> Vec<PlanInfo> {
    let Some(root) = root else { return vec![] };
    let mut out = vec![];
    for dir in [plans_dir(root), root.join(".cursor").join("plans")] {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.filter_map(|e| e.ok()) { let p = e.path(); if p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("md") { if let Some(i) = info_for(&p) { out.push(i); } } }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then_with(|| a.title.cmp(&b.title)));
    out
}

/// Plan body (markdown without frontmatter).
pub fn read(path: &str) -> Result<String> {
    let text = std::fs::read_to_string(path)?;
    Ok(frontmatter::parse(&text).1.to_string())
}
pub fn info(path: &str) -> Option<PlanInfo> { info_for(Path::new(path)) }

fn rewrite(path: &str, mutate: impl FnOnce(&mut Vec<(String, FmValue)>, &str) -> String) -> Result<()> {
    let p = Path::new(path);
    let text = std::fs::read_to_string(p)?;
    let (fm, body) = frontmatter::parse(&text);
    let mut fields: Vec<(String, FmValue)> = fm.map(|f| f.map.into_iter().collect()).unwrap_or_default();
    if !fields.iter().any(|(k, _)| k == "title") { fields.insert(0, ("title".into(), FmValue::Str(frontmatter::first_heading(body).unwrap_or_else(|| p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())))); }
    let new_body = mutate(&mut fields, body);
    set_field(&mut fields, "updated", FmValue::Str(iso_now()));
    let refs: Vec<(&str, FmValue)> = fields.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    let tmp = p.with_extension("md.tmp");
    std::fs::write(&tmp, format!("{}{}", frontmatter::render(&refs), new_body))?;
    std::fs::rename(&tmp, p)?;
    Ok(())
}
fn set_field(fields: &mut Vec<(String, FmValue)>, key: &str, v: FmValue) { if let Some(f) = fields.iter_mut().find(|(k, _)| k == key) { f.1 = v; } else { fields.push((key.into(), v)); } }

pub fn set_status(path: &str, status: &str) -> Result<()> {
    let status = status.to_lowercase();
    if !STATUSES.contains(&status.as_str()) { return Err(CaretError::invalid(format!("invalid plan status '{status}'"))); }
    rewrite(path, |fields, body| { set_field(fields, "status", FmValue::Str(status.clone())); body.to_string() })
}

pub fn update(path: &str, content: &str) -> Result<()> {
    let content = content.trim_start_matches('\n').to_string();
    rewrite(path, |_fields, _body| if content.ends_with('\n') { content.clone() } else { format!("{content}\n") })
}

/// Create `<root>/.caret/plans/YYYY-MM-DD-<slug>.md` (unique) with status `draft`.
pub fn create(root: &Path, title: &str, content: &str, session_id: Option<&str>) -> Result<PathBuf> {
    let dir = plans_dir(root);
    std::fs::create_dir_all(&dir)?;
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let slug = frontmatter::slugify(title);
    let mut file = dir.join(format!("{date}-{slug}.md"));
    let mut n = 2;
    while file.exists() { file = dir.join(format!("{date}-{slug}-{n}.md")); n += 1; }
    let now = iso_now();
    let mut fields: Vec<(&str, FmValue)> = vec![("title", FmValue::Str(title.trim().to_string())), ("status", FmValue::Str("draft".into()))];
    if let Some(s) = session_id { fields.push(("session", FmValue::Str(s.to_string()))); }
    fields.push(("created", FmValue::Str(now.clone()))); fields.push(("updated", FmValue::Str(now)));
    let body = content.trim_start_matches('\n');
    let body = if body.trim_start().starts_with("# ") { body.to_string() } else { format!("# {}\n\n{body}", title.trim()) };
    std::fs::write(&file, format!("{}{}{}", frontmatter::render(&fields), body, if body.ends_with('\n') { "" } else { "\n" }))?;
    let _ = now_ms();
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn create_list_status_update() {
        let tmp = tempfile::tempdir().unwrap(); let root = tmp.path();
        let p1 = create(root, "Add Login: OAuth", "## Steps\n1. do it\n", Some("sess-1")).unwrap();
        assert!(p1.file_name().unwrap().to_string_lossy().ends_with("-add-login-oauth.md"));
        let p2 = create(root, "Add Login: OAuth", "x", None).unwrap();
        assert!(p2.to_string_lossy().ends_with("-add-login-oauth-2.md"));
        std::fs::create_dir_all(root.join(".cursor/plans")).unwrap();
        std::fs::write(root.join(".cursor/plans/legacy.md"), "---\ntitle: Legacy\nstatus: approved\nupdated: 2020-01-01T00:00:00Z\n---\nold").unwrap();
        let all = list(Some(root));
        assert_eq!(all.len(), 3);
        assert_eq!(all.last().unwrap().title, "Legacy"); assert_eq!(all.last().unwrap().status, "approved");
        let first = all.iter().find(|p| p.path == p1.to_string_lossy()).unwrap();
        assert_eq!(first.title, "Add Login: OAuth"); assert_eq!(first.status, "draft"); assert_eq!(first.session_id.as_deref(), Some("sess-1"));
        let body = read(&first.path).unwrap();
        assert!(body.starts_with("# Add Login: OAuth\n\n## Steps"));
        set_status(&first.path, "Building").unwrap();
        assert_eq!(info(&first.path).unwrap().status, "building");
        assert!(set_status(&first.path, "bogus").is_err());
        update(&first.path, "# New body\n- step").unwrap();
        assert_eq!(read(&first.path).unwrap(), "# New body\n- step\n");
        let i = info(&first.path).unwrap(); assert_eq!(i.status, "building"); assert_eq!(i.title, "Add Login: OAuth");
        assert!(list(None).is_empty());
    }
}

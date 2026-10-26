//! Project/user rules: `.caret/rules/**`, `.cursor/rules/**` (compat), nested rule dirs, `.cursorrules`/`.caretrules`,
//! `AGENTS.md`, `CLAUDE.md`, `~/.caret/rules/**`. Kinds: always · auto (globs) · agent (description) · manual.
use crate::ai::agent::frontmatter::{self, FmValue};
use crate::core::settings::Settings;
use crate::error::{CaretError, Result};
use crate::paths;
use crate::tokens;
use globset::{Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleInfo {
    pub id: String, pub path: String, pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub description: Option<String>,
    pub globs: Vec<String>, pub always_apply: bool, pub kind: String, pub source: String, pub enabled: bool, pub size_bytes: u64,
}

/// A discovered rule with its on-disk location and (for nested rules) the directory it is scoped to.
#[derive(Debug, Clone)]
pub struct Discovered { pub info: RuleInfo, pub file: PathBuf, pub scope_dir: Option<PathBuf> }

pub const MAX_RULES_TOKENS: usize = 24_000;
const RULE_EXTS: [&str; 2] = ["mdc", "md"];
const SKIP_DIRS: [&str; 12] = [".git", "node_modules", "target", "dist", "build", ".next", ".venv", "venv", "__pycache__", ".cache", "vendor", "out"];

fn state_path() -> PathBuf { paths::config_dir().join("rules-state.json") }
fn user_rules_dir() -> PathBuf { paths::config_dir().join("rules") }

#[derive(Default, Serialize, Deserialize)]
struct RulesState { #[serde(default)] disabled: BTreeSet<String> }
fn load_state(p: &Path) -> RulesState { std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default() }
fn save_state(p: &Path, st: &RulesState) -> Result<()> { if let Some(d) = p.parent() { std::fs::create_dir_all(d)?; } std::fs::write(p, serde_json::to_string_pretty(st)?)?; Ok(()) }

fn source_rank(source: &str) -> u8 { match source { "agents-md" => 0, "claude-md" => 1, "legacy" => 2, "project" => 3, "cursor-project" => 4, "nested" => 5, "user" => 6, _ => 7 } }

fn is_rule_file(p: &Path) -> bool { p.extension().and_then(|e| e.to_str()).map(|e| RULE_EXTS.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false) }

fn parse_rule_file(file: &Path, id: String, path_display: String, source: &str, force_always: bool, disabled: &BTreeSet<String>) -> Option<RuleInfo> {
    let text = std::fs::read_to_string(file).ok()?;
    let (fm, _body) = frontmatter::parse(&text);
    let fm = fm.unwrap_or_default();
    let description = fm.get_str("description").filter(|s| !s.trim().is_empty());
    let globs: Vec<String> = fm.get_list("globs").into_iter().map(|g| g.trim_start_matches("./").trim_start_matches('/').to_string()).filter(|g| !g.is_empty()).collect();
    let always_apply = force_always || fm.get_bool("alwaysApply").unwrap_or(false);
    let kind = if always_apply { "always" } else if !globs.is_empty() { "auto" } else if description.is_some() { "agent" } else { "manual" };
    let name = fm.get_str("name").filter(|s| !s.trim().is_empty()).unwrap_or_else(|| {
        if source == "legacy" || source == "agents-md" || source == "claude-md" { file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() }
        else { file.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() }
    });
    Some(RuleInfo { enabled: !disabled.contains(&id), id, path: path_display, name, description, globs, always_apply, kind: kind.into(), source: source.into(), size_bytes: text.len() as u64 })
}

fn walk_rule_dir(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() { return; }
    for e in walkdir::WalkDir::new(dir).max_depth(6).follow_links(false).sort_by_file_name().into_iter().filter_map(|e| e.ok()) {
        if e.file_type().is_file() && is_rule_file(e.path()) { out.push(e.path().to_path_buf()); }
        if out.len() > 500 { break; }
    }
}

/// Discover every rule from every source. Pure (no global state) variant used by tests.
pub fn discover_with(root: Option<&Path>, settings: &Settings, user_dir: &Path, state_file: &Path) -> Vec<Discovered> {
    let disabled = load_state(state_file).disabled;
    let mut out: Vec<Discovered> = vec![];
    let mut push = |file: PathBuf, id: String, display: String, source: &str, force_always: bool, scope: Option<PathBuf>| {
        if let Some(info) = parse_rule_file(&file, id, display, source, force_always, &disabled) { out.push(Discovered { info, file, scope_dir: scope }); }
    };
    if let Some(root) = root {
        for (dir_name, source) in [(".caret", "project"), (".cursor", "cursor-project")] {
            if source == "cursor-project" && !settings.general.use_cursor_compat_dirs { continue; }
            let mut files = vec![]; walk_rule_dir(&root.join(dir_name).join("rules"), &mut files);
            for f in files { let rel = paths::rel_to(root, &f); push(f, format!("{source}:{rel}"), rel, source, false, None); }
        }
        // nested rule dirs anywhere below root (respecting .gitignore)
        let mut visited = 0usize;
        let walker = ignore::WalkBuilder::new(root).hidden(false).git_ignore(true).git_global(true).git_exclude(true).max_depth(Some(8))
            .filter_entry(|e| { let n = e.file_name().to_string_lossy(); !(e.file_type().map(|t| t.is_dir()).unwrap_or(false) && SKIP_DIRS.contains(&n.as_ref())) }).build();
        for e in walker.filter_map(|e| e.ok()) {
            visited += 1; if visited > 40_000 { break; }
            if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
            let name = e.file_name().to_string_lossy().to_string();
            if name != "rules" { continue; }
            let Some(parent) = e.path().parent() else { continue };
            let pname = parent.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if pname != ".cursor" && pname != ".caret" { continue; }
            if pname == ".cursor" && !settings.general.use_cursor_compat_dirs { continue; }
            let scope = parent.parent().map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf());
            if scope == root { continue; } // handled above
            let mut files = vec![]; walk_rule_dir(e.path(), &mut files);
            for f in files { let rel = paths::rel_to(root, &f); push(f, format!("nested:{rel}"), rel, "nested", false, Some(scope.clone())); }
        }
        for (fname, source) in [(".cursorrules", "legacy"), (".caretrules", "legacy"), ("AGENTS.md", "agents-md"), ("CLAUDE.md", "claude-md")] {
            let f = root.join(fname);
            if f.is_file() { push(f, format!("{source}:{fname}"), fname.to_string(), source, true, None); }
        }
    }
    let mut files = vec![]; walk_rule_dir(user_dir, &mut files);
    for f in files { let rel = paths::rel_to(user_dir, &f); push(f.clone(), format!("user:{rel}"), f.to_string_lossy().to_string(), "user", false, None); }
    out.sort_by(|a, b| source_rank(&a.info.source).cmp(&source_rank(&b.info.source)).then_with(|| a.info.path.cmp(&b.info.path)));
    out.dedup_by(|a, b| a.info.id == b.info.id);
    out
}

pub fn discover(root: Option<&Path>, settings: &Settings) -> Vec<Discovered> { discover_with(root, settings, &user_rules_dir(), &state_path()) }

pub fn list(root: Option<&Path>, settings: &Settings) -> Vec<RuleInfo> { discover(root, settings).into_iter().map(|d| d.info).collect() }

pub fn resolve_path(root: Option<&Path>, settings: &Settings, rule_id: &str) -> Option<PathBuf> {
    discover(root, settings).into_iter().find(|d| d.info.id == rule_id).map(|d| d.file)
}

pub fn read(root: Option<&Path>, rule_id: &str) -> Result<String> {
    let settings = Settings::default();
    let file = resolve_path(root, &settings, rule_id).ok_or_else(|| CaretError::not_found(format!("Rule {rule_id}")))?;
    Ok(std::fs::read_to_string(file)?)
}

fn body_of(file: &Path) -> String { std::fs::read_to_string(file).map(|t| frontmatter::parse(&t).1.to_string()).unwrap_or_default() }

fn build_globset(globs: &[String]) -> Option<GlobSet> {
    let mut b = GlobSetBuilder::new();
    let mut any = false;
    for g in globs {
        let g = g.trim().trim_start_matches("./").trim_start_matches('/');
        if g.is_empty() { continue; }
        match GlobBuilder::new(g).literal_separator(false).case_insensitive(true).build() { Ok(gl) => { b.add(gl); any = true; } Err(_) => { if let Ok(gl) = Glob::new(&format!("**/{g}")) { b.add(gl); any = true; } } }
    }
    if !any { return None; }
    b.build().ok()
}

fn to_rel<'a>(root: Option<&Path>, f: &'a str) -> String {
    let p = Path::new(f);
    if p.is_absolute() { if let Some(r) = root { if let Ok(rel) = p.strip_prefix(r) { return rel.to_string_lossy().to_string(); } } }
    f.trim_start_matches("./").to_string()
}

fn matches_files(d: &Discovered, root: Option<&Path>, rel_files: &[String]) -> bool {
    let Some(set) = build_globset(&d.info.globs) else { return false };
    let scope_rel: Option<String> = match (&d.scope_dir, root) { (Some(s), Some(r)) => Some(paths::rel_to(r, s)), _ => None };
    rel_files.iter().any(|f| {
        let candidate = if let Some(sr) = &scope_rel {
            let prefix = format!("{}/", sr.trim_end_matches('/'));
            match f.strip_prefix(&prefix) { Some(rest) => rest.to_string(), None => return false }
        } else { f.clone() };
        let base = Path::new(&candidate).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        set.is_match(&candidate) || set.is_match(&base)
    })
}

/// Rules applicable to `files`: always-apply (incl. legacy/AGENTS.md/CLAUDE.md) + auto (glob match) + requested ids.
/// Output is bounded to ~24k tokens: lowest-priority (auto, then requested) dropped first; always-apply content truncated last.
pub fn applicable(root: Option<&Path>, settings: &Settings, files: &[String], requested_ids: &[String]) -> Vec<(RuleInfo, String)> {
    applicable_from(&discover(root, settings), root, files, requested_ids)
}

pub fn applicable_from(all: &[Discovered], root: Option<&Path>, files: &[String], requested_ids: &[String]) -> Vec<(RuleInfo, String)> {
    let rel_files: Vec<String> = files.iter().map(|f| to_rel(root, f)).collect();
    let requested: HashSet<&str> = requested_ids.iter().map(|s| s.as_str()).collect();
    let mut always = vec![]; let mut req = vec![]; let mut auto = vec![];
    for d in all {
        if requested.contains(d.info.id.as_str()) { req.push(d); continue; }
        if !d.info.enabled { continue; }
        if d.info.always_apply { always.push(d); continue; }
        if d.info.kind == "auto" && matches_files(d, root, &rel_files) { auto.push(d); }
    }
    // also allow requesting by name (e.g. "@my-rule")
    for d in all { if !requested.contains(d.info.id.as_str()) && requested.contains(d.info.name.as_str()) && !req.iter().any(|x| x.info.id == d.info.id) && !always.iter().any(|x| x.info.id == d.info.id) { req.push(d); } }
    let mut ordered: Vec<(RuleInfo, String, u8)> = vec![];
    for (bucket, pri) in [(&always, 0u8), (&req, 1u8), (&auto, 2u8)] {
        for d in bucket.iter() { let body = body_of(&d.file); if body.trim().is_empty() { continue; } ordered.push((d.info.clone(), body, pri)); }
    }
    let mut total: usize = ordered.iter().map(|(_, b, _)| tokens::estimate(b)).sum();
    // drop lowest priority from the end until within budget
    while total > MAX_RULES_TOKENS {
        if let Some(pos) = ordered.iter().rposition(|(_, _, p)| *p == 2).or_else(|| ordered.iter().rposition(|(_, _, p)| *p == 1)) {
            let (_, b, _) = ordered.remove(pos); total -= tokens::estimate(&b);
        } else { break; }
    }
    if total > MAX_RULES_TOKENS {
        // only always-apply rules remain; truncate the largest until it fits
        let mut guard = 0;
        while total > MAX_RULES_TOKENS && guard < 16 {
            guard += 1;
            if let Some((idx, _)) = ordered.iter().enumerate().max_by_key(|(_, (_, b, _))| tokens::estimate(b)) {
                let cur = tokens::estimate(&ordered[idx].1);
                let target = cur.saturating_sub(total - MAX_RULES_TOKENS).max(256);
                let (t, _) = tokens::truncate_middle(&ordered[idx].1, target);
                total = total - cur + tokens::estimate(&t);
                ordered[idx].1 = t;
            } else { break; }
        }
    }
    ordered.into_iter().map(|(i, b, _)| (i, b)).collect()
}

/// Rules the agent may request on demand (`fetch_rules`): enabled, have a description, not always-apply.
pub fn agent_requested(root: Option<&Path>, settings: &Settings) -> Vec<RuleInfo> {
    list(root, settings).into_iter().filter(|r| r.enabled && !r.always_apply && r.description.is_some()).collect()
}

pub fn set_enabled(rule_id: &str, enabled: bool) -> Result<()> { set_enabled_at(&state_path(), rule_id, enabled) }
pub fn set_enabled_at(state_file: &Path, rule_id: &str, enabled: bool) -> Result<()> {
    let mut st = load_state(state_file);
    if enabled { st.disabled.remove(rule_id); } else { st.disabled.insert(rule_id.to_string()); }
    save_state(state_file, &st)
}

/// Create a rule file. `scope`: "project" → `<root>/.caret/rules/<slug>.mdc`, "user" → `~/.caret/rules/<slug>.mdc`.
pub fn create(root: Option<&Path>, name: &str, kind: &str, globs: &[String], description: Option<&str>, content: &str, scope: &str) -> Result<PathBuf> {
    create_in(root, &user_rules_dir(), name, kind, globs, description, content, scope)
}
pub fn create_in(root: Option<&Path>, user_dir: &Path, name: &str, kind: &str, globs: &[String], description: Option<&str>, content: &str, scope: &str) -> Result<PathBuf> {
    let dir = match scope { "user" => user_dir.to_path_buf(), _ => root.ok_or_else(CaretError::no_workspace)?.join(".caret").join("rules") };
    std::fs::create_dir_all(&dir)?;
    let slug = frontmatter::slugify(name);
    let mut file = dir.join(format!("{slug}.mdc"));
    let mut n = 2;
    while file.exists() { file = dir.join(format!("{slug}-{n}.mdc")); n += 1; }
    let always = kind == "always";
    let mut fm = String::from("---\n");
    if let Some(d) = description.filter(|d| !d.trim().is_empty()) { fm.push_str(&frontmatter::render(&[("description", FmValue::Str(d.trim().to_string()))]).trim_start_matches("---\n").trim_end_matches("---\n").to_string()); }
    // Cursor-compatible unquoted comma-separated globs
    let globs_line = if kind == "auto" && !globs.is_empty() { globs.iter().map(|g| g.trim()).filter(|g| !g.is_empty()).collect::<Vec<_>>().join(",") } else { String::new() };
    fm.push_str(&format!("globs: {globs_line}\nalwaysApply: {always}\n---\n"));
    let body = content.trim_start_matches('\n');
    std::fs::write(&file, format!("{fm}{body}{}", if body.ends_with('\n') { "" } else { "\n" }))?;
    Ok(file)
}

pub fn delete(root: Option<&Path>, settings: &Settings, rule_id: &str) -> Result<()> {
    let d = discover(root, settings).into_iter().find(|d| d.info.id == rule_id).ok_or_else(|| CaretError::not_found(format!("Rule {rule_id}")))?;
    if !matches!(d.info.source.as_str(), "project" | "cursor-project" | "user" | "nested") { return Err(CaretError::denied(format!("{} rules cannot be deleted from here", d.info.source))); }
    std::fs::remove_file(&d.file)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn w(p: &Path, s: &str) { fs::create_dir_all(p.parent().unwrap()).unwrap(); fs::write(p, s).unwrap(); }

    #[test]
    fn discovers_all_sources_and_kinds() {
        let tmp = tempfile::tempdir().unwrap(); let root = tmp.path().join("ws"); let user = tmp.path().join("user-rules"); let state = tmp.path().join("state.json");
        w(&root.join(".caret/rules/style.mdc"), "---\ndescription: Style guide\nglobs: *.ts, *.tsx\nalwaysApply: false\n---\nUse 2 spaces.\n");
        w(&root.join(".cursor/rules/always.mdc"), "---\nalwaysApply: true\n---\nAlways do X.\n");
        w(&root.join(".cursor/rules/agent.mdc"), "---\ndescription: When touching db\n---\nDB rule.\n");
        w(&root.join(".cursor/rules/manual.mdc"), "Manual rule body.\n");
        w(&root.join("packages/api/.cursor/rules/api.mdc"), "---\nglobs: **/*.go\n---\nAPI rule.\n");
        w(&root.join(".cursorrules"), "Legacy rule.\n");
        w(&root.join("AGENTS.md"), "# Agents\nRepo instructions.\n");
        w(&user.join("global.md"), "---\nalwaysApply: true\n---\nUser global.\n");
        let settings = Settings::default();
        let all = discover_with(Some(&root), &settings, &user, &state);
        let by_id = |id: &str| all.iter().find(|d| d.info.id == id).unwrap_or_else(|| panic!("missing {id}: {:?}", all.iter().map(|d| d.info.id.clone()).collect::<Vec<_>>()));
        assert_eq!(by_id("project:.caret/rules/style.mdc").info.kind, "auto");
        assert_eq!(by_id("project:.caret/rules/style.mdc").info.globs, vec!["*.ts", "*.tsx"]);
        assert_eq!(by_id("cursor-project:.cursor/rules/always.mdc").info.kind, "always");
        assert_eq!(by_id("cursor-project:.cursor/rules/agent.mdc").info.kind, "agent");
        assert_eq!(by_id("cursor-project:.cursor/rules/manual.mdc").info.kind, "manual");
        let nested = by_id("nested:packages/api/.cursor/rules/api.mdc");
        assert_eq!(nested.info.source, "nested"); assert!(nested.scope_dir.as_ref().unwrap().ends_with("packages/api"));
        assert!(by_id("legacy:.cursorrules").info.always_apply);
        assert_eq!(by_id("agents-md:AGENTS.md").info.name, "AGENTS.md");
        assert_eq!(by_id("user:global.md").info.source, "user");
        // compat off hides .cursor rules
        let mut s2 = Settings::default(); s2.general.use_cursor_compat_dirs = false;
        let all2 = discover_with(Some(&root), &s2, &user, &state);
        assert!(all2.iter().all(|d| d.info.source != "cursor-project" && d.info.source != "nested"));
    }

    #[test]
    fn applicability_globs_nesting_requested_and_toggle() {
        let tmp = tempfile::tempdir().unwrap(); let root = tmp.path().join("ws"); let user = tmp.path().join("u"); let state = tmp.path().join("state.json");
        w(&root.join(".caret/rules/ts.mdc"), "---\nglobs: *.ts\n---\nTS rule.\n");
        w(&root.join(".caret/rules/always.mdc"), "---\nalwaysApply: true\n---\nAlways.\n");
        w(&root.join(".caret/rules/manual.mdc"), "Manual.\n");
        w(&root.join("pkg/.caret/rules/pkg.mdc"), "---\nglobs: src/**/*.rs\n---\nPkg rule.\n");
        let settings = Settings::default();
        let all = discover_with(Some(&root), &settings, &user, &state);
        let ids = |v: &Vec<(RuleInfo, String)>| v.iter().map(|(i, _)| i.id.clone()).collect::<Vec<_>>();
        let a = applicable_from(&all, Some(&root), &["src/app.ts".into()], &[]);
        assert_eq!(ids(&a), vec!["project:.caret/rules/always.mdc", "project:.caret/rules/ts.mdc"]);
        let b = applicable_from(&all, Some(&root), &["pkg/src/lib/x.rs".into()], &[]);
        assert!(ids(&b).contains(&"nested:pkg/.caret/rules/pkg.mdc".to_string()));
        let c = applicable_from(&all, Some(&root), &["other/src/lib/x.rs".into()], &[]);
        assert!(!ids(&c).contains(&"nested:pkg/.caret/rules/pkg.mdc".to_string()));
        let d = applicable_from(&all, Some(&root), &[root.join("README.md").to_string_lossy().to_string()], &["project:.caret/rules/manual.mdc".into()]);
        assert_eq!(ids(&d), vec!["project:.caret/rules/always.mdc", "project:.caret/rules/manual.mdc"]);
        assert_eq!(d[1].1.trim(), "Manual.");
        // toggle off the ts rule
        set_enabled_at(&state, "project:.caret/rules/ts.mdc", false).unwrap();
        let all = discover_with(Some(&root), &settings, &user, &state);
        assert!(!all.iter().find(|d| d.info.id == "project:.caret/rules/ts.mdc").unwrap().info.enabled);
        let e = applicable_from(&all, Some(&root), &["src/app.ts".into()], &[]);
        assert_eq!(ids(&e), vec!["project:.caret/rules/always.mdc"]);
        set_enabled_at(&state, "project:.caret/rules/ts.mdc", true).unwrap();
        assert!(discover_with(Some(&root), &settings, &user, &state).iter().all(|d| d.info.enabled));
    }

    #[test]
    fn budget_drops_auto_first_then_truncates_always() {
        let tmp = tempfile::tempdir().unwrap(); let root = tmp.path().join("ws"); let user = tmp.path().join("u"); let state = tmp.path().join("s.json");
        let big = "lorem ipsum dolor sit amet ".repeat(6000); // ~ 36k tokens
        w(&root.join(".caret/rules/big-auto.mdc"), &format!("---\nglobs: *.ts\n---\n{big}"));
        w(&root.join(".caret/rules/small.mdc"), "---\nalwaysApply: true\n---\nSmall always.\n");
        let all = discover_with(Some(&root), &Settings::default(), &user, &state);
        let a = applicable_from(&all, Some(&root), &["a.ts".into()], &[]);
        assert_eq!(a.len(), 1); assert_eq!(a[0].0.name, "small");
        w(&root.join(".caret/rules/big-always.mdc"), &format!("---\nalwaysApply: true\n---\n{big}"));
        let all = discover_with(Some(&root), &Settings::default(), &user, &state);
        let b = applicable_from(&all, Some(&root), &[], &[]);
        let total: usize = b.iter().map(|(_, c)| tokens::estimate(c)).sum();
        assert!(total <= MAX_RULES_TOKENS + 64, "total {total}");
        assert!(b.iter().any(|(_, c)| c.contains("truncated")));
    }

    #[test]
    fn create_and_delete_rule_files() {
        let tmp = tempfile::tempdir().unwrap(); let root = tmp.path().join("ws"); let user = tmp.path().join("u"); let state = tmp.path().join("s.json");
        fs::create_dir_all(&root).unwrap();
        let f = create_in(Some(&root), &user, "React Components", "auto", &["*.tsx".into(), "src/**/*.jsx".into()], Some("How we write React"), "Use function components.", "project").unwrap();
        assert!(f.ends_with(".caret/rules/react-components.mdc"));
        let text = fs::read_to_string(&f).unwrap();
        assert!(text.contains("globs: *.tsx,src/**/*.jsx"), "{text}");
        assert!(text.contains("description: How we write React"));
        let all = discover_with(Some(&root), &Settings::default(), &user, &state);
        let r = &all[0].info; assert_eq!(r.kind, "auto"); assert_eq!(r.globs, vec!["*.tsx", "src/**/*.jsx"]); assert_eq!(r.description.as_deref(), Some("How we write React"));
        let f2 = create_in(Some(&root), &user, "React Components", "always", &[], None, "x", "project").unwrap();
        assert!(f2.ends_with("react-components-2.mdc"));
        let fu = create_in(None, &user, "Global", "manual", &[], None, "y", "user").unwrap();
        assert!(fu.starts_with(&user));
        fs::remove_file(f2).unwrap();
        let text = fs::read_to_string(&fu).unwrap(); assert!(text.contains("alwaysApply: false"));
    }
}

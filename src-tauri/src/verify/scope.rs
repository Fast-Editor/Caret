//! Scope resolution: turn a `VerifyScope` into concrete touched files with before/after text and added lines.
use super::VerifyScope;
use crate::ai::agent::manager::AgentManager;
use crate::ai::agent::session::ChangeKind;
use crate::ai::util::run_git;
use crate::error::{CaretError, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang { Js, Ts, Python, Rust, Go, Shell, Sql, Yaml, Json, Toml, Markdown, Java, Ruby, Php, CSharp, Other }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileClass { Source, Test, Doc, Config, Manifest, Lockfile, Ci, Infra, Other }

#[derive(Debug, Clone)]
pub struct TouchedFile {
    /// Workspace-relative path (forward slashes).
    pub path: String,
    pub abs: PathBuf,
    pub before: Option<String>,
    pub after: Option<String>,
    /// (new-file line number, text) for added/changed lines.
    pub added_lines: Vec<(u32, String)>,
    pub removed_lines: Vec<(u32, String)>,
    pub diff: String,
    pub lang: Lang,
    pub class: FileClass,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone)]
pub struct ResolvedScope {
    pub root: PathBuf,
    pub files: Vec<TouchedFile>,
    pub session_id: Option<String>,
    pub additions: u32,
    pub deletions: u32,
    pub final_assistant_text: Option<String>,
}
impl ResolvedScope {
    pub fn source_files(&self) -> impl Iterator<Item = &TouchedFile> { self.files.iter().filter(|f| f.class == FileClass::Source) }
    pub fn langs(&self) -> std::collections::HashSet<Lang> { self.files.iter().map(|f| f.lang).collect() }
    pub fn rel_paths(&self) -> Vec<String> { self.files.iter().map(|f| f.path.clone()).collect() }
}

pub fn lang_of(path: &str) -> Lang {
    let p = path.to_ascii_lowercase();
    let ext = p.rsplit('.').next().unwrap_or("");
    let base = p.rsplit('/').next().unwrap_or(&p);
    match ext {
        "ts" | "tsx" | "mts" | "cts" => Lang::Ts,
        "js" | "jsx" | "mjs" | "cjs" => Lang::Js,
        "py" | "pyi" => Lang::Python,
        "rs" => Lang::Rust,
        "go" => Lang::Go,
        "sh" | "bash" | "zsh" => Lang::Shell,
        "sql" => Lang::Sql,
        "yml" | "yaml" => Lang::Yaml,
        "json" | "jsonc" => Lang::Json,
        "toml" => Lang::Toml,
        "md" | "mdx" => Lang::Markdown,
        "java" | "kt" | "kts" => Lang::Java,
        "rb" => Lang::Ruby,
        "php" => Lang::Php,
        "cs" => Lang::CSharp,
        _ => if base == "dockerfile" || base == "makefile" { Lang::Shell } else { Lang::Other },
    }
}

pub fn classify(path: &str) -> FileClass {
    let p = path.replace('\\', "/");
    let lower = p.to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower).to_string();
    let lockfiles = ["package-lock.json", "pnpm-lock.yaml", "yarn.lock", "bun.lockb", "bun.lock", "cargo.lock", "poetry.lock", "uv.lock", "pipfile.lock", "go.sum", "composer.lock", "gemfile.lock"];
    if lockfiles.contains(&base.as_str()) { return FileClass::Lockfile; }
    let manifests = ["package.json", "cargo.toml", "pyproject.toml", "requirements.txt", "requirements-dev.txt", "setup.py", "setup.cfg", "pipfile", "go.mod", "gemfile", "composer.json", "build.gradle", "pom.xml"];
    if manifests.contains(&base.as_str()) || base.starts_with("requirements") && base.ends_with(".txt") { return FileClass::Manifest; }
    if lower.contains(".github/workflows/") || lower.contains(".gitlab-ci") || lower.contains(".circleci/") || base == "jenkinsfile" || lower.contains("azure-pipelines") || lower.contains(".buildkite/") { return FileClass::Ci; }
    if base == "dockerfile" || base.starts_with("dockerfile.") || base.starts_with("docker-compose") || lower.ends_with(".tf") || lower.ends_with(".tfvars") || lower.contains("/terraform/") || lower.starts_with("terraform/") || lower.contains("/k8s/") || lower.starts_with("k8s/") || lower.contains("/helm/") || lower.starts_with("helm/") || lower.contains("/infra/") || lower.starts_with("infra/") || lower.contains("/kubernetes/") { return FileClass::Infra; }
    let is_test = lower.contains("/tests/") || lower.starts_with("tests/") || lower.contains("/test/") || lower.starts_with("test/") || lower.contains("__tests__/") || lower.contains(".test.") || lower.contains(".spec.") || lower.contains("_test.") || base.starts_with("test_") || lower.ends_with("_spec.rb") || lower.contains("/spec/");
    if is_test { return FileClass::Test; }
    if lower.ends_with(".md") || lower.ends_with(".mdx") || lower.ends_with(".rst") || lower.ends_with(".txt") || lower.starts_with("docs/") || lower.contains("/docs/") || base == "license" || base == "changelog.md" { return FileClass::Doc; }
    match lang_of(&lower) {
        Lang::Json | Lang::Yaml | Lang::Toml => FileClass::Config,
        Lang::Other => if base.starts_with('.') || base.ends_with("rc") || base.ends_with(".ini") || base.ends_with(".cfg") || base.ends_with(".env") || base.starts_with(".env") || base.ends_with(".conf") || base.ends_with(".properties") { FileClass::Config } else { FileClass::Other },
        _ => FileClass::Source,
    }
}

/// Parse a unified diff into (added lines by new-line-number, removed lines by old-line-number).
pub fn parse_unified(diff: &str) -> (Vec<(u32, String)>, Vec<(u32, String)>) {
    let mut added = vec![]; let mut removed = vec![];
    let mut new_ln: u32 = 0; let mut old_ln: u32 = 0; let mut in_hunk = false;
    for line in diff.lines() {
        if line.starts_with("@@") {
            in_hunk = true;
            // @@ -a,b +c,d @@
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(o) = parts.get(1) { old_ln = o.trim_start_matches('-').split(',').next().and_then(|n| n.parse().ok()).unwrap_or(1); }
            if let Some(n) = parts.get(2) { new_ln = n.trim_start_matches('+').split(',').next().and_then(|n| n.parse().ok()).unwrap_or(1); }
            continue;
        }
        if !in_hunk { continue; }
        if line.starts_with("+++") || line.starts_with("---") { continue; }
        if let Some(t) = line.strip_prefix('+') { added.push((new_ln, t.to_string())); new_ln += 1; }
        else if let Some(t) = line.strip_prefix('-') { removed.push((old_ln, t.to_string())); old_ln += 1; }
        else if line.starts_with('\\') { /* \ No newline at end of file */ }
        else { new_ln += 1; old_ln += 1; }
    }
    (added, removed)
}

/// Split a multi-file unified diff (git format) into (path, per-file diff).
pub fn split_git_diff(diff: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    let mut cur: Option<(String, String)> = None;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            if let Some(c) = cur.take() { out.push(c); }
            // diff --git a/x b/y → use b/ path
            let b = line.split(" b/").nth(1).map(|s| s.to_string()).unwrap_or_default();
            cur = Some((b, String::new()));
        }
        if let Some(c) = cur.as_mut() {
            if line.starts_with("+++ b/") { c.0 = line.trim_start_matches("+++ b/").to_string(); }
            c.1.push_str(line); c.1.push('\n');
        }
    }
    if let Some(c) = cur.take() { out.push(c); }
    out.into_iter().filter(|(p, _)| !p.is_empty() && p != "/dev/null").collect()
}

fn mk(root: &Path, rel: &str, before: Option<String>, after: Option<String>, diff: String, kind: ChangeKind) -> TouchedFile {
    let (added, removed) = if diff.trim().is_empty() {
        match (&before, &after) {
            (None, Some(a)) => (a.lines().enumerate().map(|(i, l)| (i as u32 + 1, l.to_string())).collect(), vec![]),
            (Some(b), Some(a)) => { let (d, _, _) = crate::ai::agent::tools::edit::unified_diff(rel, b, a); parse_unified(&d) }
            _ => (vec![], vec![]),
        }
    } else { parse_unified(&diff) };
    TouchedFile { path: rel.to_string(), abs: root.join(rel), before, after, added_lines: added, removed_lines: removed, diff, lang: lang_of(rel), class: classify(rel), kind }
}

fn read_opt(p: &Path) -> Option<String> { std::fs::read(p).ok().filter(|b| b.len() <= 4 * 1024 * 1024 && !b.contains(&0u8)).map(|b| String::from_utf8_lossy(&b).to_string()) }

pub async fn resolve(app: &AppHandle, scope: &VerifyScope) -> Result<ResolvedScope> {
    let root = app.state::<crate::core::workspace::WorkspaceState>().root().ok_or_else(CaretError::no_workspace)?;
    let mut files: Vec<TouchedFile> = vec![];
    let mut final_text = None;
    match scope.kind.as_str() {
        "session" | "change-set" => {
            let sid = scope.session_id.clone().ok_or_else(|| CaretError::invalid("sessionId required for session scope"))?;
            let mgr = app.state::<Arc<AgentManager>>();
            let h = mgr.handle(&sid)?;
            let s = h.session.lock();
            let sroot = s.root().unwrap_or_else(|| root.clone());
            final_text = s.messages.iter().rev().find(|m| m.role == crate::ai::agent::session::MessageRole::Assistant).map(|m| m.text());
            let selected: Vec<_> = if let Some(ids) = &scope.change_ids {
                s.changes.iter().filter(|c| ids.contains(&c.change_id)).cloned().collect()
            } else {
                let last_turn = s.changes.iter().rev().find_map(|c| c.turn_id.clone());
                s.changes.iter().filter(|c| c.status != crate::ai::agent::session::ChangeStatus::Rejected && (last_turn.is_none() || c.turn_id == last_turn)).cloned().collect()
            };
            drop(s);
            // collapse multiple changes to the same path: earliest `before`, latest `after`
            let mut by_path: std::collections::BTreeMap<String, (Option<String>, Option<String>, ChangeKind)> = Default::default();
            for c in selected {
                let rel = c.path.trim_start_matches("./").to_string();
                let e = by_path.entry(rel).or_insert((c.before.clone(), None, c.kind));
                e.1 = c.after.clone(); if matches!(c.kind, ChangeKind::Delete) { e.2 = ChangeKind::Delete; }
            }
            for (rel, (before, after, kind)) in by_path {
                let abs = sroot.join(&rel);
                let after = if matches!(kind, ChangeKind::Delete) { None } else { after.or_else(|| read_opt(&abs)) };
                let diff = match (&before, &after) { (Some(b), Some(a)) => crate::ai::agent::tools::edit::unified_diff(&rel, b, a).0, _ => String::new() };
                files.push(mk(&sroot, &rel, before, after, diff, kind));
            }
            return finish(sroot, files, Some(sid), final_text);
        }
        "staged" | "working-tree" | "branch" => {
            let mut args: Vec<String> = vec!["diff".into(), "--no-color".into(), "--no-ext-diff".into(), "--unified=3".into()];
            let base_rev: Option<String> = match scope.kind.as_str() {
                "staged" => { args.push("--cached".into()); None }
                "branch" => { let base = scope.base.clone().unwrap_or_else(|| "HEAD".into()); args.push(format!("{base}...HEAD").replace("...HEAD", "")); Some(base) }
                _ => None,
            };
            let diff = run_git(&root, &args.iter().map(|s| s.as_str()).collect::<Vec<_>>()).await.unwrap_or_default();
            for (rel, fdiff) in split_git_diff(&diff) {
                let abs = root.join(&rel);
                let before = match scope.kind.as_str() {
                    "branch" => run_git(&root, &["show", &format!("{}:{}", base_rev.clone().unwrap_or_else(|| "HEAD".into()), rel)]).await.ok(),
                    _ => run_git(&root, &["show", &format!("HEAD:{rel}")]).await.ok(),
                };
                let after = if scope.kind == "staged" { run_git(&root, &["show", &format!(":{rel}")]).await.ok() } else { read_opt(&abs) };
                let kind = match (&before, &after) { (None, Some(_)) => ChangeKind::Create, (Some(_), None) => ChangeKind::Delete, _ => ChangeKind::Modify };
                files.push(mk(&root, &rel, before, after, fdiff, kind));
            }
            if scope.kind == "working-tree" {
                if let Ok(untracked) = run_git(&root, &["ls-files", "--others", "--exclude-standard"]).await {
                    for rel in untracked.lines().map(str::trim).filter(|l| !l.is_empty()).take(200) {
                        let abs = root.join(rel);
                        if let Some(after) = read_opt(&abs) { files.push(mk(&root, rel, None, Some(after), String::new(), ChangeKind::Create)); }
                    }
                }
            }
        }
        "files" => {
            for f in scope.files.clone().unwrap_or_default() {
                let abs = crate::paths::confine(&root, &f)?;
                let rel = crate::paths::rel_to(&root, &abs);
                if let Some(after) = read_opt(&abs) { files.push(mk(&root, &rel, None, Some(after), String::new(), ChangeKind::Modify)); }
            }
        }
        other => return Err(CaretError::invalid(format!("unknown verify scope kind: {other}"))),
    }
    finish(root, files, scope.session_id.clone(), final_text)
}

fn finish(root: PathBuf, files: Vec<TouchedFile>, session_id: Option<String>, final_text: Option<String>) -> Result<ResolvedScope> {
    let additions = files.iter().map(|f| f.added_lines.len() as u32).sum();
    let deletions = files.iter().map(|f| f.removed_lines.len() as u32).sum();
    Ok(ResolvedScope { root, files, session_id, additions, deletions, final_assistant_text: final_text })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies_paths() {
        assert_eq!(classify("src/lib.rs"), FileClass::Source);
        assert_eq!(classify("tests/pricing.test.mjs"), FileClass::Test);
        assert_eq!(classify("src/__tests__/a.ts"), FileClass::Test);
        assert_eq!(classify("README.md"), FileClass::Doc);
        assert_eq!(classify("package.json"), FileClass::Manifest);
        assert_eq!(classify("package-lock.json"), FileClass::Lockfile);
        assert_eq!(classify(".github/workflows/ci.yml"), FileClass::Ci);
        assert_eq!(classify("infra/main.tf"), FileClass::Infra);
        assert_eq!(classify("config/app.yaml"), FileClass::Config);
        assert_eq!(lang_of("a/b.tsx"), Lang::Ts);
    }
    #[test]
    fn parses_unified_diff() {
        let d = "--- a/x.ts\n+++ b/x.ts\n@@ -1,3 +1,4 @@\n a\n-b\n+B\n+C\n c\n";
        let (add, rem) = parse_unified(d);
        assert_eq!(add, vec![(2, "B".into()), (3, "C".into())]);
        assert_eq!(rem, vec![(2, "b".into())]);
    }
    #[test]
    fn splits_git_diff() {
        let d = "diff --git a/one.ts b/one.ts\n--- a/one.ts\n+++ b/one.ts\n@@ -1 +1 @@\n-a\n+b\ndiff --git a/two.py b/two.py\nnew file mode 100644\n--- /dev/null\n+++ b/two.py\n@@ -0,0 +1 @@\n+print(1)\n";
        let parts = split_git_diff(d);
        assert_eq!(parts.len(), 2); assert_eq!(parts[0].0, "one.ts"); assert_eq!(parts[1].0, "two.py");
        assert!(parts[1].1.contains("+print(1)"));
    }
}

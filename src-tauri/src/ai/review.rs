//! Bugbot-style code review: diff per scope → batched LLM review → streamed structured findings.
use crate::ai::cancel;
use crate::ai::llm::{self, LlmRequest, Message, SystemBlock};
use crate::ai::util::{extract_json, read_text_capped, run_git, workspace_root};
use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use crate::events;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRequest { pub scope: String, #[serde(default)] pub base: Option<String>, #[serde(default)] pub files: Option<Vec<String>>, #[serde(default)] pub focus: Option<String>, #[serde(default)] pub model_id: Option<String>, #[serde(default)] pub sha: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFinding {
    pub id: String, pub path: String, pub line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub end_line: Option<u32>,
    pub severity: String, pub title: String, pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub suggestion: Option<String>,
    pub category: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub confidence: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitDiffStat { pub files: u32, pub additions: u32, pub deletions: u32 }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewResult { pub id: String, pub findings: Vec<ReviewFinding>, pub summary: String, pub stats: GitDiffStat, pub created_at: u64, pub model_id: String, pub scope: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ReviewStreamEvent { Status { message: String }, Finding { finding: ReviewFinding }, Done { result: ReviewResult }, Error { message: String } }

#[derive(Default)]
pub struct ReviewState { results: RwLock<Vec<ReviewResult>>, loaded_for: RwLock<Option<PathBuf>> }

fn store_path(root: &Path) -> PathBuf { crate::paths::sub_dir("reviews").join(format!("{}.json", crate::paths::workspace_hash(root))) }

impl ReviewState {
    fn ensure_loaded(&self, root: Option<&Path>) {
        let Some(root) = root else { return };
        if self.loaded_for.read().as_deref() == Some(root) { return; }
        let v: Vec<ReviewResult> = std::fs::read_to_string(store_path(root)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        *self.results.write() = v;
        *self.loaded_for.write() = Some(root.to_path_buf());
    }
    fn persist(&self, root: Option<&Path>) { if let Some(root) = root { let _ = std::fs::write(store_path(root), serde_json::to_string(&*self.results.read()).unwrap_or_default()); } }
    pub fn list(&self, root: Option<&Path>) -> Vec<ReviewResult> { self.ensure_loaded(root); self.results.read().clone() }
    pub fn push(&self, root: Option<&Path>, r: ReviewResult) { self.ensure_loaded(root); { let mut v = self.results.write(); v.insert(0, r); v.truncate(20); } self.persist(root); }
    pub fn dismiss(&self, root: Option<&Path>, id: &str) { self.ensure_loaded(root); self.results.write().retain(|r| r.id != id); self.persist(root); }
    pub fn dismiss_finding(&self, root: Option<&Path>, id: &str, fid: &str) { self.ensure_loaded(root); for r in self.results.write().iter_mut() { if r.id == id { r.findings.retain(|f| f.id != fid); } } self.persist(root); }
}

/// One reviewable unit: a file with annotated content (diff hunks or full file).
#[derive(Debug, Clone)]
pub struct ReviewFile { pub path: String, pub text: String, pub tokens: usize }

/// Split a unified diff into per-file sections with new-file line numbers prefixed on '+'/' ' lines.
pub fn annotate_diff(diff: &str) -> Vec<ReviewFile> {
    let mut files = vec![];
    let mut cur_path: Option<String> = None;
    let mut cur = String::new();
    let mut new_line = 0u32;
    let flush = |files: &mut Vec<ReviewFile>, path: &Option<String>, text: &mut String| { if let Some(p) = path { if !text.trim().is_empty() { let t = std::mem::take(text); files.push(ReviewFile { path: p.clone(), tokens: crate::tokens::estimate(&t), text: t }); } } };
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            flush(&mut files, &cur_path, &mut cur);
            let b = rest.split(" b/").nth(1).unwrap_or(rest).trim_matches('"').to_string();
            cur_path = Some(b); cur.clear(); continue;
        }
        if line.starts_with("+++ ") || line.starts_with("--- ") || line.starts_with("index ") || line.starts_with("new file") || line.starts_with("deleted file") || line.starts_with("similarity") || line.starts_with("rename ") || line.starts_with("old mode") || line.starts_with("new mode") || line.starts_with("Binary files") { continue; }
        if let Some(h) = line.strip_prefix("@@") {
            // @@ -a,b +c,d @@
            if let Some(plus) = h.split('+').nth(1) { new_line = plus.split(|c: char| c == ',' || c == ' ').next().and_then(|s| s.parse().ok()).unwrap_or(1); }
            cur.push_str(line); cur.push('\n'); continue;
        }
        match line.chars().next() {
            Some('+') => { cur.push_str(&format!("L{new_line:>5} + {}\n", &line[1..])); new_line += 1; }
            Some('-') => { cur.push_str(&format!("       - {}\n", &line[1..])); }
            Some('\\') => {}
            _ => { let body = line.strip_prefix(' ').unwrap_or(line); cur.push_str(&format!("L{new_line:>5}   {body}\n")); new_line += 1; }
        }
    }
    flush(&mut files, &cur_path, &mut cur);
    files
}

pub fn batch_files(files: Vec<ReviewFile>, max_tokens: usize) -> Vec<Vec<ReviewFile>> {
    let mut batches: Vec<Vec<ReviewFile>> = vec![];
    let mut cur: Vec<ReviewFile> = vec![]; let mut cur_tok = 0usize;
    for mut f in files {
        if f.tokens > max_tokens { let (t, _) = crate::tokens::truncate_middle(&f.text, max_tokens); f.text = t; f.tokens = max_tokens; }
        if !cur.is_empty() && cur_tok + f.tokens > max_tokens { batches.push(std::mem::take(&mut cur)); cur_tok = 0; }
        cur_tok += f.tokens; cur.push(f);
    }
    if !cur.is_empty() { batches.push(cur); }
    batches
}

fn diff_stat(diff: &str) -> GitDiffStat {
    let mut s = GitDiffStat::default();
    for l in diff.lines() { if l.starts_with("diff --git") { s.files += 1; } else if l.starts_with('+') && !l.starts_with("+++") { s.additions += 1; } else if l.starts_with('-') && !l.starts_with("---") { s.deletions += 1; } }
    s
}

async fn build_input(root: &Path, req: &ReviewRequest) -> Result<(Vec<ReviewFile>, GitDiffStat, String)> {
    match req.scope.as_str() {
        "staged" => { let d = run_git(root, &["diff", "--cached", "--no-color", "--no-ext-diff"]).await?; Ok((annotate_diff(&d), diff_stat(&d), "staged changes".into())) }
        "branch" => {
            let base = match &req.base { Some(b) => b.clone(), None => default_branch(root).await };
            let d = run_git(root, &["diff", "--no-color", "--no-ext-diff", &format!("{base}...HEAD")]).await?;
            let wt = run_git(root, &["diff", "--no-color", "--no-ext-diff"]).await.unwrap_or_default();
            let all = format!("{d}\n{wt}");
            Ok((annotate_diff(&all), diff_stat(&all), format!("branch vs {base}")))
        }
        "commit" => { let sha = req.sha.clone().or_else(|| req.base.clone()).unwrap_or_else(|| "HEAD".into()); let d = run_git(root, &["show", "--no-color", "--format=", &sha]).await?; Ok((annotate_diff(&d), diff_stat(&d), format!("commit {sha}"))) }
        "files" => {
            let mut files = vec![];
            for f in req.files.clone().unwrap_or_default() {
                let abs = crate::paths::confine(root, &f)?;
                let (text, _) = read_text_capped(&abs, 200 * 1024)?;
                let numbered: String = text.lines().enumerate().map(|(i, l)| format!("L{:>5}   {l}\n", i + 1)).collect();
                files.push(ReviewFile { path: crate::paths::rel_to(root, &abs), tokens: crate::tokens::estimate(&numbered), text: numbered });
            }
            let n = files.len() as u32;
            Ok((files, GitDiffStat { files: n, additions: 0, deletions: 0 }, "selected files".into()))
        }
        _ => {
            let mut d = run_git(root, &["diff", "--no-color", "--no-ext-diff", "HEAD"]).await.or_else(|_| Ok::<_, CaretError>(String::new()))?;
            if d.trim().is_empty() { d = run_git(root, &["diff", "--no-color", "--no-ext-diff"]).await.unwrap_or_default(); }
            let mut files = annotate_diff(&d);
            let untracked = run_git(root, &["ls-files", "--others", "--exclude-standard"]).await.unwrap_or_default();
            for f in untracked.lines().take(30) {
                let abs = root.join(f);
                if let Ok((text, _)) = read_text_capped(&abs, 100 * 1024) {
                    if text.contains('\0') { continue; }
                    let numbered: String = text.lines().enumerate().map(|(i, l)| format!("L{:>5} + {l}\n", i + 1)).collect();
                    files.push(ReviewFile { path: f.to_string(), tokens: crate::tokens::estimate(&numbered), text: numbered });
                }
            }
            Ok((files, diff_stat(&d), "working tree".into()))
        }
    }
}

async fn default_branch(root: &Path) -> String {
    if let Ok(s) = run_git(root, &["symbolic-ref", "refs/remotes/origin/HEAD"]).await { if let Some(b) = s.trim().rsplit('/').next() { return format!("origin/{b}"); } }
    for b in ["main", "master", "develop"] { if run_git(root, &["rev-parse", "--verify", b]).await.is_ok() { return b.to_string(); } }
    "HEAD~1".into()
}

const SYSTEM: &str = "You are Bugbot, a senior code reviewer. Review the changes and report ONLY real problems: bugs, logic errors, crashes, race conditions, security vulnerabilities, data loss, broken error handling, performance pitfalls with real impact, and correctness regressions. Do NOT report style, naming, formatting, missing comments, or speculative nits. Prefer precision over recall — each finding must be actionable and specific.\nLines are prefixed with their new-file line number as `L123`. Respond with JSON only: {\"summary\": string, \"findings\": [{\"path\": string, \"line\": number, \"endLine\": number|null, \"severity\": \"critical\"|\"high\"|\"medium\"|\"low\"|\"info\", \"title\": string, \"description\": string, \"suggestion\": string|null, \"category\": string, \"confidence\": number}]}. Use an empty findings array when the code looks correct.";

pub async fn start_review(app: AppHandle, req: ReviewRequest, ch: Channel<ReviewStreamEvent>) -> Result<String> {
    let root = workspace_root(&app).ok_or_else(CaretError::no_workspace)?;
    let review_id = uuid::Uuid::new_v4().to_string();
    let settings_state = app.state::<SettingsState>();
    let settings = settings_state.effective();
    let resolved = crate::ai::util::pick_model_auto(Some(&app), &settings, &settings_state, req.model_id.as_deref(), false, Some(&format!("review-{review_id}")))?;
    let id2 = review_id.clone();
    tauri::async_runtime::spawn(async move {
        let token = cancel::register(&id2);
        let res = run_review(&app, root, req, resolved, id2.clone(), ch.clone(), token).await;
        cancel::unregister(&id2);
        if let Err(e) = res { if e.code != "cancelled" { let _ = ch.send(ReviewStreamEvent::Error { message: e.message.clone() }); } }
    });
    Ok(review_id)
}

async fn run_review(app: &AppHandle, root: PathBuf, req: ReviewRequest, resolved: llm::ResolvedModel, review_id: String, ch: Channel<ReviewStreamEvent>, token: tokio_util::sync::CancellationToken) -> Result<()> {
    let _ = ch.send(ReviewStreamEvent::Status { message: "Collecting changes…".into() });
    let (files, stats, label) = build_input(&root, &req).await?;
    if files.is_empty() { let _ = ch.send(ReviewStreamEvent::Error { message: format!("Nothing to review ({label})") }); return Ok(()); }
    let batches = batch_files(files, 8_000);
    let _ = ch.send(ReviewStreamEvent::Status { message: format!("Reviewing {} file batch(es) with {}…", batches.len(), resolved.model.display_name) });
    let sem = Arc::new(tokio::sync::Semaphore::new(3));
    let focus = req.focus.clone().map(|f| format!("\nReview focus requested by the user: {f}\n")).unwrap_or_default();
    let mut handles = vec![];
    for batch in batches {
        let sem = sem.clone(); let resolved = resolved.clone(); let ch = ch.clone(); let token = token.clone(); let focus = focus.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.ok()?;
            let mut body = String::new();
            for f in &batch { body.push_str(&format!("<file path=\"{}\">\n{}</file>\n", f.path, f.text)); }
            let mut r = LlmRequest::new(resolved.model.clone()).purpose("review");
            r.system = vec![SystemBlock { text: SYSTEM.into(), cache: true }];
            r.messages = vec![Message::user_text(format!("{focus}\n{body}"))];
            r.max_tokens = r.max_tokens.min(8_000);
            r.thinking.effort = crate::ai::llm::Effort::Medium;
            let (raw, _) = match llm::complete_text(&resolved, r, token).await { Ok(x) => x, Err(e) => { tracing::warn!(error = %e, "review batch failed"); return Some((String::new(), vec![], Some(e))); } };
            let v = extract_json(&raw)?;
            let summary = v["summary"].as_str().unwrap_or("").to_string();
            let paths: Vec<String> = batch.iter().map(|f| f.path.clone()).collect();
            let mut findings = vec![];
            for f in v["findings"].as_array().cloned().unwrap_or_default() {
                let path = f["path"].as_str().unwrap_or("").to_string();
                let path = if paths.contains(&path) { path } else { paths.iter().find(|p| p.ends_with(&path) || path.ends_with(p.as_str())).cloned().unwrap_or(path) };
                let finding = ReviewFinding {
                    id: uuid::Uuid::new_v4().to_string(), path, line: f["line"].as_u64().unwrap_or(1).max(1) as u32,
                    end_line: f["endLine"].as_u64().map(|x| x as u32), severity: f["severity"].as_str().unwrap_or("medium").to_string(),
                    title: f["title"].as_str().unwrap_or("Issue").to_string(), description: f["description"].as_str().unwrap_or("").to_string(),
                    suggestion: f["suggestion"].as_str().map(String::from), category: f["category"].as_str().unwrap_or("correctness").to_string(),
                    confidence: f["confidence"].as_f64().map(|c| c as f32),
                };
                let _ = ch.send(ReviewStreamEvent::Finding { finding: finding.clone() });
                findings.push(finding);
            }
            Some((summary, findings, None))
        }));
    }
    let mut all = vec![]; let mut summaries = vec![]; let mut errors = vec![];
    for h in handles { if let Ok(Some((s, f, e))) = h.await { if !s.is_empty() { summaries.push(s); } all.extend(f); if let Some(e) = e { errors.push(e); } } }
    if token.is_cancelled() { return Err(CaretError::cancelled()); }
    if all.is_empty() && !errors.is_empty() { return Err(errors.remove(0)); }
    let sev_rank = |s: &str| match s { "critical" => 0, "high" => 1, "medium" => 2, "low" => 3, _ => 4 };
    all.sort_by(|a, b| sev_rank(&a.severity).cmp(&sev_rank(&b.severity)).then(a.path.cmp(&b.path)).then(a.line.cmp(&b.line)));
    let summary = if summaries.is_empty() { format!("Reviewed {label}: {} finding(s).", all.len()) } else { summaries.join(" ") };
    let result = ReviewResult { id: review_id, findings: all, summary, stats, created_at: crate::ai::util::now_ms(), model_id: resolved.model.id.clone(), scope: req.scope.clone() };
    app.state::<ReviewState>().push(Some(&root), result.clone());
    events::emit(app, events::REVIEW_CHANGED, ());
    let _ = ch.send(ReviewStreamEvent::Done { result });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn annotates_diff_with_new_line_numbers() {
        let d = "diff --git a/src/x.rs b/src/x.rs\nindex 1..2 100644\n--- a/src/x.rs\n+++ b/src/x.rs\n@@ -10,3 +10,4 @@ fn f() {\n ctx\n-old\n+new1\n+new2\n ctx2\ndiff --git a/b.txt b/b.txt\n@@ -1 +1 @@\n-a\n+b\n";
        let files = annotate_diff(d);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/x.rs");
        assert!(files[0].text.contains("L   10   ctx"), "{}", files[0].text);
        assert!(files[0].text.contains("L   11 + new1") && files[0].text.contains("L   12 + new2") && files[0].text.contains("L   13   ctx2"));
        assert!(files[0].text.contains("       - old"));
        assert_eq!(files[1].path, "b.txt");
        let s = diff_stat(d); assert_eq!((s.files, s.additions, s.deletions), (2, 3, 2));
    }
    #[test]
    fn batches_by_token_budget() {
        let mk = |p: &str, n: usize| ReviewFile { path: p.into(), text: "x ".repeat(n), tokens: n };
        let b = batch_files(vec![mk("a", 5000), mk("b", 4000), mk("c", 100), mk("d", 20000)], 8000);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].len(), 1); assert_eq!(b[1].len(), 2); assert!(b[2][0].tokens <= 8000);
    }
}

//! Commit message generation from the staged (or working-tree) diff.
use crate::ai::llm::{self, LlmRequest, Message, SystemBlock};
use crate::ai::util::{extract_json, pick_model, run_git, workspace_root};
use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitMessageRequest { #[serde(default)] pub style: Option<String>, #[serde(default)] pub include_body: bool, #[serde(default)] pub model_id: Option<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitMessageResult { pub summary: String, pub body: String }

const NOISY: &[&str] = &["package-lock.json", "yarn.lock", "pnpm-lock.yaml", "Cargo.lock", "poetry.lock", "Gemfile.lock", "composer.lock", "go.sum", ".min.js", ".min.css", ".map", ".snap", "bun.lockb"];

/// Drop noisy files and truncate oversized hunks; cap total size to ~`max_tokens`.
pub fn trim_diff(diff: &str, max_tokens: usize) -> String {
    let mut out = String::new();
    for section in diff.split_inclusive("\ndiff --git ").map(|s| s.to_string()).collect::<Vec<_>>().into_iter().fold(Vec::<String>::new(), |mut acc, part| {
        // re-split on "diff --git" boundaries preserving the header
        if let Some(stripped) = part.strip_suffix("\ndiff --git ") { acc.push(stripped.to_string()); acc.push("diff --git ".to_string()); } else { match acc.last_mut() { Some(last) if last == "diff --git " => last.push_str(&part), _ => acc.push(part) } }
        acc
    }) {
        if section.is_empty() { continue; }
        let header = section.lines().next().unwrap_or("");
        if NOISY.iter().any(|n| header.contains(n)) { out.push_str(&format!("{header}\n(omitted: generated/lock file)\n")); continue; }
        let mut lines = 0usize;
        for l in section.lines() {
            lines += 1;
            if lines > 400 { out.push_str("… (hunk truncated)\n"); break; }
            out.push_str(l); out.push('\n');
        }
    }
    crate::tokens::truncate_middle(&out, max_tokens).0
}

const SYSTEM: &str = "You write excellent git commit messages. Given a diff, respond with JSON only: {\"summary\": string, \"body\": string}. The summary is imperative mood, ≤ 72 characters, no trailing period. The body (may be empty) explains WHAT changed and WHY as short bullet points (\"- …\"), wrapped at 72 columns, no restating the summary.";

pub async fn commit_message(app: &AppHandle, req: CommitMessageRequest) -> Result<CommitMessageResult> {
    let root = workspace_root(app).ok_or_else(CaretError::no_workspace)?;
    let mut diff = run_git(&root, &["diff", "--cached", "--no-color", "--no-ext-diff"]).await?;
    let mut scope_note = "staged changes";
    if diff.trim().is_empty() {
        diff = run_git(&root, &["diff", "--no-color", "--no-ext-diff"]).await?;
        scope_note = "unstaged changes (nothing is staged)";
        let untracked = run_git(&root, &["ls-files", "--others", "--exclude-standard"]).await.unwrap_or_default();
        if !untracked.trim().is_empty() { diff.push_str("\nUntracked files:\n"); for f in untracked.lines().take(50) { diff.push_str(&format!("  {f}\n")); } }
    }
    if diff.trim().is_empty() { return Err(CaretError::new("empty", "No changes to describe (working tree clean)")); }
    let trimmed = trim_diff(&diff, 12_000);
    let settings_state = app.state::<SettingsState>();
    let settings = settings_state.effective();
    let resolved = pick_model(&settings, &settings_state, req.model_id.as_deref(), true)?;
    let style = match req.style.as_deref() { Some("conventional") => "Use Conventional Commits (type(scope): summary — types: feat, fix, refactor, docs, test, chore, perf, build, ci).", _ => "Plain style (no type prefix)." };
    let body_req = if req.include_body { "Include a body." } else { "Leave body empty unless the change is non-obvious." };
    let branch = run_git(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).await.unwrap_or_default();
    let mut r = LlmRequest::new(resolved.model.clone()).no_thinking().max_tokens(600).purpose("commit-message");
    r.system = vec![SystemBlock { text: SYSTEM.into(), cache: true }];
    r.messages = vec![Message::user_text(format!("{style} {body_req}\nBranch: {}\nScope: {scope_note}\n\n<diff>\n{trimmed}\n</diff>", branch.trim()))];
    r.temperature = Some(0.2);
    let (raw, _) = llm::complete_text(&resolved, r, CancellationToken::new()).await?;
    let v = match extract_json(&raw) {
        Some(v) => v,
        None => {
            // Plain-text fallback: first meaningful line is the summary, the rest is the body.
            let cleaned = crate::ai::tab::strip_banner_lines(&raw);
            let cleaned = crate::ai::util::strip_fences(&cleaned);
            let mut lines = cleaned.lines().map(|l| l.trim()).filter(|l| !l.is_empty());
            let summary = lines.next().map(|l| l.trim_matches('"').trim_end_matches('.').chars().take(72).collect::<String>()).unwrap_or_default();
            if summary.is_empty() { return Err(CaretError::provider("model did not return a commit message")); }
            let body = lines.take_while(|l| !l.starts_with("```") && !l.to_ascii_lowercase().starts_with("command:")).take(12).collect::<Vec<_>>().join("\n");
            serde_json::json!({ "summary": summary, "body": body })
        }
    };
    let mut summary = v["summary"].as_str().unwrap_or("").trim().trim_end_matches('.').to_string();
    if summary.chars().count() > 72 { summary = summary.chars().take(72).collect(); }
    if summary.is_empty() { return Err(CaretError::provider("empty commit summary")); }
    let body = if req.include_body { v["body"].as_str().unwrap_or("").trim().to_string() } else { String::new() };
    Ok(CommitMessageResult { summary, body })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trims_noisy_files_and_long_hunks() {
        let mut d = String::from("diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n+fn x() {}\n");
        d.push_str("diff --git a/package-lock.json b/package-lock.json\n");
        for i in 0..50 { d.push_str(&format!("+  \"dep{i}\": \"1.0.0\",\n")); }
        d.push_str("diff --git a/big.txt b/big.txt\n");
        for i in 0..1000 { d.push_str(&format!("+line {i}\n")); }
        let t = trim_diff(&d, 12_000);
        assert!(t.contains("fn x() {}"));
        assert!(t.contains("omitted: generated/lock file") && !t.contains("dep10"));
        assert!(t.contains("hunk truncated") && !t.contains("line 999"));
    }
}

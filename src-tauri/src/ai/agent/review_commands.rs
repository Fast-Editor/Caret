//! Review-on-diff commands: inline comments on pending changes, review groups ("review by concern"),
//! and turning comments into a structured follow-up turn. Exposed as `api.review.*`.
use crate::ai::agent::manager::{AgentManager, SessionHandle};
use crate::ai::agent::session::*;
use crate::error::{CaretError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use tauri::State;

type Mgr<'a> = State<'a, Arc<AgentManager>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewGroup {
    pub id: String,
    pub title: String,
    /// intent | tool-call | file-kind | other
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub tool_call_id: Option<String>,
    pub change_ids: Vec<String>,
    pub collapsed_by_default: bool,
    pub order: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewReviewComment { pub path: String, pub line: u32, #[serde(default)] pub end_line: Option<u32>, #[serde(default)] pub side: Option<String>, pub text: String, #[serde(default)] pub change_id: Option<String>, #[serde(default)] pub finding_id: Option<String> }

fn emit_comments(mgr: &AgentManager, h: &SessionHandle) {
    let (sid, comments) = { let s = h.session.lock(); (s.id.clone(), s.review_comments.clone()) };
    mgr.persist(h);
    mgr.emit(AgentEvent::ReviewComments { session_id: sid, comments });
}

#[tauri::command(async)]
pub fn chat_review_comments(mgr: Mgr<'_>, session_id: String) -> Result<Vec<ReviewComment>> { let h = mgr.handle(&session_id)?; let v = h.session.lock().review_comments.clone(); Ok(v) }

#[tauri::command]
pub fn chat_add_review_comment(mgr: Mgr<'_>, session_id: String, comment: NewReviewComment) -> Result<ReviewComment> {
    if comment.text.trim().is_empty() { return Err(CaretError::new("invalid", "comment text is empty")); }
    let h = mgr.handle(&session_id)?;
    let c = ReviewComment { id: short_id(), path: comment.path.replace('\\', "/"), line: comment.line.max(1), end_line: comment.end_line, side: comment.side.unwrap_or_else(|| "after".into()), text: comment.text.trim().to_string(), change_id: comment.change_id, created_at: now_ms(), resolved: None, author: "user".into(), finding_id: comment.finding_id };
    { let mut s = h.session.lock(); s.review_comments.push(c.clone()); s.touch(); }
    emit_comments(&mgr, &h);
    Ok(c)
}

#[tauri::command]
pub fn chat_resolve_review_comment(mgr: Mgr<'_>, session_id: String, comment_id: String, resolved: Option<bool>) -> Result<()> {
    let h = mgr.handle(&session_id)?;
    { let mut s = h.session.lock(); let Some(c) = s.review_comments.iter_mut().find(|c| c.id == comment_id) else { return Err(CaretError::new("not_found", "comment not found")); }; c.resolved = if resolved.unwrap_or(true) { Some(true) } else { None }; s.touch(); }
    emit_comments(&mgr, &h);
    Ok(())
}

#[tauri::command]
pub fn chat_delete_review_comment(mgr: Mgr<'_>, session_id: String, comment_id: String) -> Result<()> {
    let h = mgr.handle(&session_id)?;
    { let mut s = h.session.lock(); let before = s.review_comments.len(); s.review_comments.retain(|c| c.id != comment_id); if s.review_comments.len() == before { return Err(CaretError::new("not_found", "comment not found")); } s.touch(); }
    emit_comments(&mgr, &h);
    Ok(())
}

/// Render unresolved comments as a structured follow-up turn (with the diff context around each line) and submit it.
#[tauri::command]
pub async fn chat_send_review_comments(mgr: Mgr<'_>, session_id: String, text: Option<String>) -> Result<String> {
    let h = mgr.handle(&session_id)?;
    let (comments, changes, mode, model_id, agent_id) = { let s = h.session.lock(); (s.review_comments.iter().filter(|c| c.resolved != Some(true)).cloned().collect::<Vec<_>>(), s.changes.clone(), s.mode.clone(), s.model_id.clone(), s.agent_id.clone()) };
    if comments.is_empty() && text.as_deref().map(|t| t.trim().is_empty()).unwrap_or(true) { return Err(CaretError::new("invalid", "no unresolved review comments to send")); }
    let body = format_review_turn(&comments, &changes, text.as_deref());
    let req = SendMessageRequest { session_id: session_id.clone(), text: body.clone(), context: vec![], images: vec![], mode, model_id, thinking: None, effort: None, use_worktree: None, queue: None, active_file: None, open_files: None, cursor_selection: None, agent_id, review_comments: Some(comments.clone()) };
    let mgr_arc: Arc<AgentManager> = mgr.inner().clone();
    mgr_arc.submit(req).await?;
    // Mark the sent comments resolved so the panel shows them as "addressed by follow-up".
    { let mut s = h.session.lock(); for c in s.review_comments.iter_mut() { if comments.iter().any(|x| x.id == c.id) { c.resolved = Some(true); } } s.touch(); }
    emit_comments(&mgr, &h);
    Ok(body)
}

/// Build the follow-up text: one section per file, each comment with ±3 lines of the *after* content.
pub fn format_review_turn(comments: &[ReviewComment], changes: &[PendingChange], extra: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str("Please address the following review comments on your changes. Apply the requested fixes, keep unrelated code untouched, and reply with a short summary of what changed per comment.\n");
    if let Some(t) = extra.map(str::trim).filter(|t| !t.is_empty()) { out.push_str("\nGeneral note: "); out.push_str(t); out.push('\n'); }
    let mut by_path: BTreeMap<&str, Vec<&ReviewComment>> = BTreeMap::new();
    for c in comments { by_path.entry(c.path.as_str()).or_default().push(c); }
    for (path, cs) in by_path {
        out.push_str(&format!("\n### {path}\n"));
        let content = changes.iter().filter(|ch| ch.path == path).last().and_then(|ch| ch.after.clone().or_else(|| ch.before.clone()));
        let mut cs = cs; cs.sort_by_key(|c| c.line);
        for c in cs {
            let range = c.end_line.filter(|e| *e > c.line).map(|e| format!("lines {}-{}", c.line, e)).unwrap_or_else(|| format!("line {}", c.line));
            out.push_str(&format!("\n- **{range}** ({}): {}\n", if c.side == "before" { "original" } else { "new" }, c.text));
            if let Some(src) = &content { if c.side != "before" {
                let lines: Vec<&str> = src.lines().collect();
                let lo = (c.line as usize).saturating_sub(4); let hi = (c.end_line.unwrap_or(c.line) as usize + 3).min(lines.len());
                if lo < hi { out.push_str("  ```\n"); for (i, l) in lines[lo..hi].iter().enumerate() { let n = lo + i + 1; let mark = if n as u32 >= c.line && n as u32 <= c.end_line.unwrap_or(c.line) { ">" } else { " " }; out.push_str(&format!("  {mark}{n:>4}| {l}\n")); } out.push_str("  ```\n"); }
            } }
        }
    }
    out
}

/// Logical order for review: types/interfaces → implementation → tests → docs → config/lockfiles.
pub fn file_rank(path: &str) -> u32 {
    let p = path.to_lowercase(); let base = p.rsplit('/').next().unwrap_or(&p);
    let is_lock = base.ends_with(".lock") || base == "package-lock.json" || base == "pnpm-lock.yaml" || base == "yarn.lock" || base == "cargo.lock" || base == "go.sum" || base == "poetry.lock" || base == "bun.lockb";
    if is_lock { return 60; }
    let config_names = ["package.json", "cargo.toml", "pyproject.toml", "tsconfig.json", "go.mod", "requirements.txt", ".eslintrc", "vite.config", "webpack.config", "jest.config", "vitest.config", "dockerfile", "makefile", ".env"];
    if config_names.iter().any(|c| base.starts_with(c)) || base.ends_with(".toml") || base.ends_with(".yaml") || base.ends_with(".yml") || base.ends_with(".ini") || base.ends_with(".cfg") || p.contains("/.github/") { return 50; }
    if base.ends_with(".md") || base.ends_with(".mdx") || base.ends_with(".rst") || base.ends_with(".txt") || p.contains("/docs/") { return 40; }
    if p.contains("/test") || p.contains("__tests__") || base.contains(".test.") || base.contains(".spec.") || base.starts_with("test_") || base.ends_with("_test.go") || base.ends_with("_test.py") || base.ends_with("_test.rs") { return 30; }
    if base.ends_with(".d.ts") || base.contains("types") || base.contains("schema") || base.contains("interface") || base.contains("contract") || base.contains("model") || base.ends_with(".proto") || base.ends_with(".graphql") { return 10; }
    20
}
pub fn rank_title(rank: u32) -> &'static str { match rank { 10 => "Types & contracts", 20 => "Implementation", 30 => "Tests", 40 => "Docs", 50 => "Config", _ => "Lockfiles" } }

/// Groups for the review panel. `by`: "intent" (default; one group per agent turn, titled by the
/// assistant's opening line), "tool-call", "file-kind" (types → impl → tests → docs → config/lock), or "file".
#[tauri::command(async)]
pub fn chat_review_groups(mgr: Mgr<'_>, session_id: String, by: Option<String>) -> Result<Vec<ReviewGroup>> {
    let h = mgr.handle(&session_id)?;
    let (changes, messages) = { let s = h.session.lock(); (s.changes.clone(), s.messages.iter().filter(|m| m.role == MessageRole::Assistant).map(|m| (m.id.clone(), m.text().lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim_start_matches(['#', '*', ' ']).chars().take(80).collect::<String>())).collect::<Vec<_>>()) };
    Ok(build_groups(&changes, &messages, by.as_deref().unwrap_or("intent")))
}

pub fn build_groups(changes: &[PendingChange], assistant_messages: &[(String, String)], by: &str) -> Vec<ReviewGroup> {
    let mut sorted: Vec<&PendingChange> = changes.iter().collect();
    sorted.sort_by(|a, b| file_rank(&a.path).cmp(&file_rank(&b.path)).then_with(|| a.path.cmp(&b.path)));
    let mut groups: Vec<ReviewGroup> = vec![];
    match by {
        "concern" | "file-kind" => {
            let mut buckets: BTreeMap<u32, Vec<String>> = BTreeMap::new();
            for c in &sorted { buckets.entry(file_rank(&c.path)).or_default().push(c.change_id.clone()); }
            for (i, (rank, ids)) in buckets.into_iter().enumerate() { groups.push(ReviewGroup { id: format!("kind-{rank}"), title: rank_title(rank).into(), kind: "file-kind".into(), turn_id: None, tool_call_id: None, change_ids: ids, collapsed_by_default: rank >= 40, order: i as u32 }); }
        }
        "file" => {
            let mut seen: Vec<String> = vec![];
            for c in &sorted { if !seen.contains(&c.path) { seen.push(c.path.clone()); } }
            for (i, path) in seen.iter().enumerate() { let ids: Vec<String> = sorted.iter().filter(|c| &c.path == path).map(|c| c.change_id.clone()).collect(); groups.push(ReviewGroup { id: format!("file-{i}"), title: path.clone(), kind: "other".into(), turn_id: None, tool_call_id: None, change_ids: ids, collapsed_by_default: file_rank(path) >= 50, order: i as u32 }); }
        }
        "tool-call" => {
            let mut order: Vec<String> = vec![];
            for c in changes { let t = c.tool_call_id.clone().unwrap_or_default(); if !order.contains(&t) { order.push(t); } }
            for (i, tc) in order.iter().enumerate() {
                let mine: Vec<&&PendingChange> = sorted.iter().filter(|c| c.tool_call_id.as_deref().unwrap_or("") == tc).collect();
                let title = if mine.len() == 1 { mine[0].path.clone() } else { format!("{} files", mine.len()) };
                let ids: Vec<String> = mine.iter().map(|c| c.change_id.clone()).collect();
                let turn_id = mine.first().and_then(|c| c.turn_id.clone());
                groups.push(ReviewGroup { id: format!("call-{tc}"), title, kind: "tool-call".into(), turn_id, tool_call_id: Some(tc.clone()), change_ids: ids, collapsed_by_default: mine.iter().all(|c| file_rank(&c.path) >= 50), order: i as u32 });
            }
        }
        _ => {
            // By turn (intent), in chronological order; within a turn, logical file order.
            let mut turn_order: Vec<String> = vec![];
            for c in changes { let t = c.turn_id.clone().unwrap_or_default(); if !turn_order.contains(&t) { turn_order.push(t); } }
            for (i, tid) in turn_order.iter().enumerate() {
                let ids: Vec<String> = sorted.iter().filter(|c| c.turn_id.as_deref().unwrap_or("") == tid).map(|c| c.change_id.clone()).collect();
                let title = assistant_messages.iter().find(|(mid, _)| mid == tid).map(|(_, t)| t.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| format!("Turn {}", i + 1));
                let lock_only = sorted.iter().filter(|c| c.turn_id.as_deref().unwrap_or("") == tid).all(|c| file_rank(&c.path) >= 50);
                groups.push(ReviewGroup { id: format!("turn-{tid}"), title, kind: "intent".into(), turn_id: Some(tid.clone()), tool_call_id: None, change_ids: ids, collapsed_by_default: lock_only, order: i as u32 });
            }
        }
    }
    groups
}

#[cfg(test)]
mod t {
    use super::*;
    fn ch(id: &str, path: &str, turn: &str) -> PendingChange { PendingChange { change_id: id.into(), session_id: "s".into(), path: path.into(), kind: ChangeKind::Modify, before: Some("a\nb\nc\nd\ne\nf\ng\n".into()), after: Some("a\nb\nC\nd\ne\nf\ng\n".into()), diff: String::new(), status: ChangeStatus::Pending, turn_id: Some(turn.into()), tool_call_id: Some("t".into()), additions: 1, deletions: 1, old_path: None } }
    #[test]
    fn ranks() { assert!(file_rank("src/types.ts") < file_rank("src/app.ts")); assert!(file_rank("src/app.ts") < file_rank("src/app.test.ts")); assert!(file_rank("README.md") < file_rank("package.json")); assert!(file_rank("package.json") < file_rank("package-lock.json")); }
    #[test]
    fn groups_by_turn_and_concern() {
        let changes = vec![ch("1", "package-lock.json", "t1"), ch("2", "src/app.ts", "t1"), ch("3", "src/types.ts", "t1"), ch("4", "src/app.test.ts", "t2")];
        let g = build_groups(&changes, &[("t1".into(), "Add feature".into())], "intent");
        assert_eq!(g.len(), 2); assert_eq!(g[0].title, "Add feature"); assert_eq!(g[0].change_ids, vec!["3", "2", "1"]); assert_eq!(g[1].title, "Turn 2");
        let c = build_groups(&changes, &[], "file-kind");
        assert_eq!(c[0].title, "Types & contracts"); assert!(c.last().unwrap().collapsed_by_default);
    }
    #[test]
    fn follow_up_text() {
        let comments = vec![ReviewComment { id: "c1".into(), path: "src/app.ts".into(), line: 3, end_line: None, side: "after".into(), text: "use const".into(), change_id: None, created_at: 0, resolved: None, author: "user".into(), finding_id: None }];
        let s = format_review_turn(&comments, &[ch("2", "src/app.ts", "t1")], Some("also run tests"));
        assert!(s.contains("### src/app.ts")); assert!(s.contains("**line 3**")); assert!(s.contains(">   3| C")); assert!(s.contains("General note: also run tests"));
    }
}

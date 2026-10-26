//! @-mention context: resolves `ContextItem`s into prompt blocks with per-kind budgets, and
//! produces mention candidates for the composer picker.
use crate::ai::agent::session::{ContextItem, ContextPreview, ContextPreviewItem, Range1};
use crate::ai::agent::tools::fs_tools::read_text;
use crate::ai::agent::tools::search_tools::{fuzzy_score, list_dirs};
use crate::ai::agent::{rules, skills};
use crate::core::diagnostics::DiagnosticsStore;
use crate::core::ports;
use crate::core::sessions::SessionStore;
use crate::core::settings::Settings;
use crate::tokens;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

pub struct ContextEnv<'a> {
    pub app: Option<&'a AppHandle>,
    pub root: Option<PathBuf>,
    pub cwd: PathBuf,
    pub settings: &'a Settings,
    pub ws_key: String,
    pub store: &'a SessionStore,
    pub files: Arc<Vec<String>>,
}

#[derive(Default)]
pub struct BuiltContext {
    pub text: String,
    /// (media_type, base64)
    pub images: Vec<(String, String)>,
    pub rule_ids: Vec<String>,
    pub preview: Vec<ContextPreviewItem>,
    pub total_tokens: u64,
}

const TOTAL_BUDGET: usize = 90_000;

fn budget_for(kind: &str) -> usize {
    match kind { "file" | "active-file" => 10_000, "folder" => 6_000, "code" | "selection" | "symbol" => 5_000, "codebase" => 6_000, "git-diff" | "git-staged" | "git-branch-diff" | "git-commit" => 9_000, "web" => 4_000, "docs" => 4_000, "terminal" => 3_000, "lint" => 3_000, "recent-changes" => 7_000, "past-chat" => 4_000, "rule" | "skill" => 4_000, "link" => 7_000, "note" | "plan" => 4_000, "mcp-resource" => 5_000, "open-files" => 12_000, _ => 3_000 }
}

pub async fn git(root: &Path, args: &[&str]) -> String {
    match tokio::process::Command::new("git").args(args).current_dir(root).env("GIT_PAGER", "cat").output().await {
        Ok(o) => { let mut s = String::from_utf8_lossy(&o.stdout).to_string(); if !o.status.success() { s.push_str(&String::from_utf8_lossy(&o.stderr)); } s }
        Err(e) => format!("(git unavailable: {e})"),
    }
}

fn resolve(env: &ContextEnv, p: &str) -> PathBuf { crate::paths::resolve(Some(&env.cwd), p) }
fn display(env: &ContextEnv, p: &Path) -> String { env.root.as_ref().and_then(|r| p.strip_prefix(r).ok()).map(|r| r.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string_lossy().to_string()) }

fn file_block(env: &ContextEnv, path: &str, range: Option<&Range1>) -> String {
    let abs = resolve(env, path);
    if abs.is_dir() { return folder_block(env, path); }
    match read_text(&abs) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let (s, e) = match range { Some(r) => ((r.start_line.max(1) as usize).min(lines.len().max(1)), (r.end_line as usize).min(lines.len())), None => (1, lines.len()) };
            let body: String = lines.iter().enumerate().skip(s.saturating_sub(1)).take(e.saturating_sub(s.saturating_sub(1))).map(|(i, l)| format!("{}| {}\n", i + 1, l)).collect();
            let hdr = if range.is_some() { format!("{} (lines {}-{})", display(env, &abs), s, e) } else { format!("{} ({} lines)", display(env, &abs), lines.len()) };
            format!("<file path=\"{}\">\n{}\n```\n{}```\n</file>", display(env, &abs), hdr, body)
        }
        Err(e) => format!("<file path=\"{path}\">(unreadable: {})</file>", e.message),
    }
}

fn folder_block(env: &ContextEnv, path: &str) -> String {
    let abs = resolve(env, path);
    let mut walker = ignore::WalkBuilder::new(&abs);
    walker.max_depth(Some(3)).hidden(true).git_ignore(true);
    let mut out = format!("<folder path=\"{}\">\n", display(env, &abs));
    let mut n = 0; let mut small: Vec<PathBuf> = vec![];
    for e in walker.build().flatten() {
        if e.depth() == 0 { continue; }
        let rel = e.path().strip_prefix(&abs).unwrap_or(e.path()).to_string_lossy().to_string();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        out.push_str(&format!("{}{}\n", rel, if is_dir { "/" } else { "" }));
        if !is_dir && e.metadata().map(|m| m.len() < 6000).unwrap_or(false) && small.len() < 8 { small.push(e.path().to_path_buf()); }
        n += 1; if n >= 300 { out.push_str("…\n"); break; }
    }
    for f in small { if let Ok(c) = read_text(&f) { out.push_str(&format!("\n--- {} ---\n{}\n", display(env, &f), c)); } }
    out.push_str("</folder>"); out
}

async fn item_block(env: &ContextEnv<'_>, item: &ContextItem, images: &mut Vec<(String, String)>, rule_ids: &mut Vec<String>) -> String {
    let kind = item.kind.as_str();
    match kind {
        "file" | "active-file" => item.path.as_deref().map(|p| file_block(env, p, None)).unwrap_or_default(),
        "code" | "selection" | "symbol" => match (&item.path, &item.data) {
            (Some(p), None) => file_block(env, p, item.range.as_ref()),
            (p, Some(d)) => format!("<selection path=\"{}\"{}>\n```\n{}\n```\n</selection>", p.clone().unwrap_or_default(), item.range.as_ref().map(|r| format!(" lines=\"{}-{}\"", r.start_line, r.end_line)).unwrap_or_default(), d),
            _ => String::new(),
        },
        "folder" => item.path.as_deref().map(|p| folder_block(env, p)).unwrap_or_default(),
        "codebase" => {
            let q = item.query.clone().or_else(|| Some(item.label.clone())).unwrap_or_default();
            match ports::codebase() { Some(p) if p.is_ready() => match p.search(&q, 10, None).await { Ok(hits) => format!("<codebase_search query=\"{q}\">\n{}\n</codebase_search>", hits.iter().map(|h| format!("### {}:{}-{}\n```\n{}\n```", h.path, h.start_line, h.end_line, h.snippet.trim_end())).collect::<Vec<_>>().join("\n\n")), Err(e) => format!("(codebase search failed: {})", e.message) }, _ => "(codebase index not ready; use grep_search/codebase_search tools)".into() }
        }
        "git-diff" | "git-staged" | "git-branch-diff" | "git-commit" => {
            let Some(root) = env.root.as_ref() else { return String::new() };
            let out = match kind {
                "git-diff" => { let s = git(root, &["status", "--short"]).await; let d = git(root, &["diff"]).await; format!("git status --short:\n{s}\ngit diff:\n{d}") }
                "git-staged" => git(root, &["diff", "--cached"]).await,
                "git-branch-diff" => { let base = item.base.clone().unwrap_or_else(|| "main".into()); let log = git(root, &["log", "--oneline", &format!("{base}..HEAD")]).await; let d = git(root, &["diff", &format!("{base}...HEAD")]).await; format!("commits since {base}:\n{log}\ndiff:\n{d}") }
                _ => { let sha = item.sha.clone().unwrap_or_else(|| "HEAD".into()); git(root, &["show", "--stat", "--patch", &sha]).await }
            };
            format!("<git kind=\"{kind}\">\n{out}\n</git>")
        }
        "web" => {
            let q = item.query.clone().unwrap_or_else(|| item.label.clone());
            let app = env.app.cloned();
            let secrets = move |id: &str| app.as_ref().and_then(|a| a.state::<crate::core::settings::SettingsState>().secret(id));
            match crate::ai::agent::tools::web_tools::search(env.settings, secrets, &q, 6).await { Ok(r) => format!("<web_search query=\"{q}\">\n{}\n</web_search>", crate::ai::agent::tools::web_tools::format_results(&r)), Err(e) => format!("(web search failed: {})", e.message) }
        }
        "link" => { let url = item.url.clone().unwrap_or_else(|| item.label.clone()); match crate::ai::agent::tools::web_tools::fetch_text(&url, 24_000).await { Ok((t, _, _)) => format!("<link url=\"{url}\">\n{t}\n</link>"), Err(e) => format!("(fetch failed for {url}: {})", e.message) } }
        "docs" => {
            let q = item.query.clone().unwrap_or_else(|| item.label.clone());
            let ids = item.doc_id.clone().map(|d| vec![d]);
            match ports::codebase() { Some(p) => match p.search_docs(&q, ids.as_deref(), 6).await { Ok(h) => format!("<docs query=\"{q}\">\n{}\n</docs>", h.iter().map(|d| format!("### {} — {}\n{}", d.title, d.url, d.snippet)).collect::<Vec<_>>().join("\n\n")), Err(e) => format!("(docs search failed: {})", e.message) }, None => "(docs not available)".into() }
        }
        "terminal" => { let Some(p) = ports::terminal() else { return "(no terminals)".into() }; let id = item.terminal_id.clone().or_else(|| p.list().first().map(|(i, _)| i.clone())); match id.and_then(|i| p.read_buffer(&i, 200)) { Some(b) => format!("<terminal_output>\n{b}\n</terminal_output>"), None => "(terminal buffer unavailable)".into() } }
        "lint" => { let paths = item.path.clone().map(|p| vec![resolve(env, &p).to_string_lossy().to_string()]); let s = env.app.map(|a| a.state::<DiagnosticsStore>().summary(paths.as_deref(), 150)).unwrap_or_default(); format!("<lint_errors>\n{}\n</lint_errors>", if s.is_empty() { "No diagnostics reported." } else { &s }) }
        "recent-changes" => {
            let Some(root) = env.root.as_ref() else { return String::new() };
            let st = git(root, &["status", "--short"]).await; let d = git(root, &["diff", "HEAD", "--stat"]).await; let full = git(root, &["diff", "HEAD"]).await;
            format!("<recent_changes>\nstatus:\n{st}\nstat:\n{d}\ndiff:\n{full}\n</recent_changes>")
        }
        "past-chat" => {
            let Some(sid) = item.session_id.as_ref() else { return String::new() };
            match env.store.load(&env.ws_key, sid) { Ok(s) => { let body = s.summary.clone().unwrap_or_else(|| s.messages.iter().rev().take(12).collect::<Vec<_>>().into_iter().rev().map(|m| format!("{:?}: {}", m.role, m.preview(600))).collect::<Vec<_>>().join("\n")); format!("<past_chat title=\"{}\">\n{body}\n</past_chat>", s.title) } Err(_) => "(chat not found)".into() }
        }
        "rule" => { let id = item.rule_id.clone().or_else(|| item.path.clone()).unwrap_or_default(); rule_ids.push(id.clone()); match rules::read(env.root.as_deref(), &id) { Ok(t) => format!("<rule name=\"{}\">\n{t}\n</rule>", item.label), Err(e) => format!("(rule unavailable: {})", e.message) } }
        "skill" => { let id = item.rule_id.clone().or_else(|| item.path.clone()).unwrap_or_else(|| item.label.clone()); match skills::read(env.root.as_deref(), &id) { Ok(t) => format!("<skill name=\"{}\">\n{t}\n</skill>", item.label), Err(e) => format!("(skill unavailable: {})", e.message) } }
        "note" | "plan" => match (&item.data, &item.path) { (Some(d), _) => format!("<note title=\"{}\">\n{d}\n</note>", item.label), (None, Some(p)) => file_block(env, p, None), _ => String::new() },
        "mcp-resource" => { let (Some(s), Some(u)) = (&item.server_id, &item.uri) else { return String::new() }; match ports::mcp() { Some(p) => match p.read_resource(s, u).await { Ok(r) => format!("<mcp_resource uri=\"{u}\">\n{}\n</mcp_resource>", r.content.iter().filter_map(|c| c.text.clone()).collect::<Vec<_>>().join("\n")), Err(e) => format!("(resource failed: {})", e.message) }, None => "(MCP unavailable)".into() } }
        "image" => { if let (Some(d), Some(m)) = (&item.data, &item.mime) { images.push((m.clone(), d.clone())); } String::new() }
        "open-files" => { let list: Vec<String> = item.data.as_deref().map(|d| d.lines().map(String::from).collect()).unwrap_or_default(); list.iter().take(6).map(|p| file_block(env, p, None)).collect::<Vec<_>>().join("\n") }
        _ => item.data.clone().map(|d| format!("<context label=\"{}\">\n{d}\n</context>", item.label)).unwrap_or_default(),
    }
}

pub async fn build(env: &ContextEnv<'_>, items: &[ContextItem]) -> BuiltContext {
    let mut out = BuiltContext::default();
    let mut total = 0usize;
    for item in items {
        let block = item_block(env, item, &mut out.images, &mut out.rule_ids).await;
        if block.is_empty() { out.preview.push(ContextPreviewItem { id: item.id.clone(), tokens: 0, truncated: false }); continue; }
        let (text, truncated) = tokens::truncate_middle(&block, budget_for(&item.kind));
        let t = tokens::estimate(&text);
        if total + t > TOTAL_BUDGET { out.text.push_str(&format!("\n(omitted @{}: context budget exhausted)\n", item.label)); out.preview.push(ContextPreviewItem { id: item.id.clone(), tokens: t as u64, truncated: true }); continue; }
        total += t;
        out.preview.push(ContextPreviewItem { id: item.id.clone(), tokens: t as u64, truncated });
        out.text.push_str(&text); out.text.push_str("\n\n");
    }
    out.total_tokens = total as u64;
    out
}

pub async fn preview(env: &ContextEnv<'_>, items: &[ContextItem]) -> ContextPreview {
    let b = build(env, items).await;
    ContextPreview { total_tokens: b.total_tokens, items: b.preview }
}

fn item(kind: &str, label: &str) -> ContextItem { ContextItem { id: format!("{kind}:{label}"), kind: kind.into(), label: label.into(), ..Default::default() } }

pub async fn mention_candidates(env: &ContextEnv<'_>, kind: &str, query: &str, limit: usize) -> Vec<ContextItem> {
    let q = query.trim();
    match kind {
        "file" | "code" | "active-file" => {
            let mut scored: Vec<(i32, &String)> = env.files.iter().filter_map(|f| fuzzy_score(f, q).map(|s| (s, f))).collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())));
            scored.into_iter().take(limit).map(|(_, f)| ContextItem { id: format!("file:{f}"), kind: "file".into(), label: f.rsplit('/').next().unwrap_or(f).to_string(), path: Some(f.clone()), ..Default::default() }).collect()
        }
        "folder" => { let dirs = list_dirs(&env.cwd, 5000); let mut scored: Vec<(i32, String)> = dirs.into_iter().filter_map(|d| fuzzy_score(&d, q).map(|s| (s, d))).collect(); scored.sort_by(|a, b| b.0.cmp(&a.0)); scored.into_iter().take(limit).map(|(_, d)| ContextItem { id: format!("folder:{d}"), kind: "folder".into(), label: d.clone(), path: Some(d), ..Default::default() }).collect() }
        "rule" => rules::list(env.root.as_deref(), env.settings).into_iter().filter(|r| q.is_empty() || r.name.to_lowercase().contains(&q.to_lowercase())).take(limit).map(|r| ContextItem { id: format!("rule:{}", r.id), kind: "rule".into(), label: r.name.clone(), rule_id: Some(r.id.clone()), path: Some(r.path.clone()), ..Default::default() }).collect(),
        "skill" => skills::list(env.root.as_deref()).into_iter().filter(|s| q.is_empty() || s.name.to_lowercase().contains(&q.to_lowercase())).take(limit).map(|s| ContextItem { id: format!("skill:{}", s.id), kind: "skill".into(), label: s.name.clone(), rule_id: Some(s.id.clone()), path: Some(s.path.clone()), ..Default::default() }).collect(),
        "past-chat" => env.store.read_index(&env.ws_key).into_iter().filter(|s| q.is_empty() || s.title.to_lowercase().contains(&q.to_lowercase())).take(limit).map(|s| ContextItem { id: format!("past-chat:{}", s.id), kind: "past-chat".into(), label: s.title.clone(), session_id: Some(s.id.clone()), ..Default::default() }).collect(),
        "terminal" => ports::terminal().map(|p| p.list().into_iter().map(|(id, t)| ContextItem { id: format!("terminal:{id}"), kind: "terminal".into(), label: t, terminal_id: Some(id), ..Default::default() }).collect()).unwrap_or_default(),
        "mcp-resource" => match ports::mcp() { Some(p) => p.list_resources().await.into_iter().filter(|r| q.is_empty() || r.name.to_lowercase().contains(&q.to_lowercase()) || r.uri.contains(q)).take(limit).map(|r| ContextItem { id: format!("mcp:{}:{}", r.server_id, r.uri), kind: "mcp-resource".into(), label: r.name.clone(), server_id: Some(r.server_id.clone()), uri: Some(r.uri.clone()), ..Default::default() }).collect(), None => vec![] },
        "git-commit" => { let Some(root) = env.root.as_ref() else { return vec![] }; let log = git(root, &["log", "--oneline", "-n", "30"]).await; log.lines().filter(|l| q.is_empty() || l.to_lowercase().contains(&q.to_lowercase())).take(limit).filter_map(|l| { let (sha, msg) = l.split_once(' ')?; Some(ContextItem { id: format!("commit:{sha}"), kind: "git-commit".into(), label: format!("{sha} {msg}"), sha: Some(sha.into()), ..Default::default() }) }).collect() }
        "git-branch-diff" => { let Some(root) = env.root.as_ref() else { return vec![] }; let b = git(root, &["branch", "--format=%(refname:short)"]).await; b.lines().filter(|l| q.is_empty() || l.contains(q)).take(limit).map(|l| ContextItem { id: format!("branch-diff:{l}"), kind: "git-branch-diff".into(), label: format!("Diff with {l}"), base: Some(l.to_string()), ..Default::default() }).collect() }
        "docs" => { let mut v = vec![ContextItem { id: "docs:*".into(), kind: "docs".into(), label: if q.is_empty() { "Docs".into() } else { format!("Docs: {q}") }, query: if q.is_empty() { None } else { Some(q.into()) }, ..Default::default() }]; if !q.is_empty() { if let Some(p) = ports::codebase() { if let Ok(h) = p.search_docs(q, None, limit.min(8)).await { v.extend(h.into_iter().map(|d| ContextItem { id: format!("doc:{}:{}", d.doc_id, d.url), kind: "docs".into(), label: d.title, query: Some(q.into()), doc_id: Some(d.doc_id), url: Some(d.url), ..Default::default() })); } } } v }
        "codebase" => vec![ContextItem { query: if q.is_empty() { None } else { Some(q.into()) }, ..item("codebase", if q.is_empty() { "Codebase" } else { q }) }],
        "web" => vec![ContextItem { query: if q.is_empty() { None } else { Some(q.into()) }, ..item("web", if q.is_empty() { "Web" } else { q }) }],
        "link" => if q.starts_with("http") { vec![ContextItem { url: Some(q.into()), ..item("link", q) }] } else { vec![] },
        "git-diff" => vec![item("git-diff", "Working tree diff"), item("git-staged", "Staged changes")],
        "lint" => vec![item("lint", "Lint errors")],
        "recent-changes" => vec![item("recent-changes", "Recent changes")],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn budgets_are_sane() { assert!(super::budget_for("file") > super::budget_for("lint")); assert!(super::budget_for("unknown") > 0); }
}

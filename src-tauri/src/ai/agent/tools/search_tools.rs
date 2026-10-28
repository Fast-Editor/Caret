//! Search tools: grep_search, file_search, codebase_search, search_docs.
use super::{arg_bool, arg_str, arg_u64, cap_output, s_arr, s_bool, s_int, s_str, schema, Tool, ToolContext, ToolOutput};
use crate::ai::agent::policy::ToolEffect;
use crate::core::ports;
use crate::error::{CaretError, Result};
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{sinks::UTF8, BinaryDetection, SearcherBuilder};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn tools() -> Vec<Arc<dyn Tool>> { vec![Arc::new(GrepSearch), Arc::new(FileSearch), Arc::new(CodebaseSearch), Arc::new(SearchDocs)] }

pub struct GrepHit { pub path: String, pub line: u64, pub text: String }

/// Regex search over a tree (gitignore-aware). Returns hits (capped) and whether the cap was reached.
pub fn grep(root: &Path, pattern: &str, include: Option<&str>, case_sensitive: Option<bool>, max_results: usize, display_root: &Path) -> Result<(Vec<GrepHit>, bool)> {
    let smart_ci = case_sensitive.unwrap_or_else(|| pattern.chars().any(|c| c.is_uppercase()));
    let matcher = RegexMatcherBuilder::new().case_insensitive(!smart_ci).line_terminator(Some(b'\n')).build(pattern).map_err(|e| CaretError::invalid(format!("Invalid regex: {e}")))?;
    let mut walker = ignore::WalkBuilder::new(root);
    if crate::paths::is_broad_root(root) { walker.max_depth(Some(1)); }
    walker.hidden(true).git_ignore(true).git_global(true).git_exclude(true).add_custom_ignore_filename(".caretignore").add_custom_ignore_filename(".cursorignore").max_filesize(Some(2 * 1024 * 1024));
    if let Some(glob) = include { let mut ov = ignore::overrides::OverrideBuilder::new(root); for g in glob.split(',') { let g = g.trim(); if g.is_empty() { continue; } let g2 = if g.contains('/') { g.to_string() } else { format!("**/{g}") }; let _ = ov.add(&g2); } if let Ok(o) = ov.build() { walker.overrides(o); } }
    let mut hits = vec![]; let mut limited = false;
    let mut searcher = SearcherBuilder::new().binary_detection(BinaryDetection::quit(0)).line_number(true).build();
    'outer: for e in walker.build().flatten() {
        if !e.file_type().map(|t| t.is_file()).unwrap_or(false) { continue; }
        let path = e.path();
        if path.components().any(|c| c.as_os_str() == ".git") { continue; }
        let rel = path.strip_prefix(display_root).unwrap_or(path).to_string_lossy().to_string();
        let mut file_hits = vec![];
        let _ = searcher.search_path(&matcher, path, UTF8(|ln, line| { file_hits.push(GrepHit { path: rel.clone(), line: ln, text: line.trim_end().chars().take(400).collect() }); Ok(file_hits.len() < 200) }));
        for h in file_hits { if hits.len() >= max_results { limited = true; break 'outer; } hits.push(h); }
    }
    Ok((hits, limited))
}

pub fn format_hits(hits: &[GrepHit], limited: bool, max_results: usize) -> String {
    if hits.is_empty() { return "No matches found.".into(); }
    let mut out = String::new(); let mut last = "";
    for h in hits { if h.path != last { out.push_str(&format!("\n{}:\n", h.path)); last = &h.path; } out.push_str(&format!("  {}: {}\n", h.line, h.text)); }
    if limited { out.push_str(&format!("\n(showing first {max_results} matches; refine the pattern or add an include filter for more)\n")); }
    out
}

pub struct GrepSearch;
#[async_trait::async_trait]
impl Tool for GrepSearch {
    fn name(&self) -> &str { "grep_search" }
    fn display_name(&self) -> &str { "Grep" }
    fn description(&self) -> String { "Fast exact/regex text search across the workspace (ripgrep semantics, respects .gitignore). Use it to find symbols, strings, imports or usages when you know what text to look for. Returns `path: line: text` grouped by file (max 50 results by default).".into() }
    fn input_schema(&self) -> Value { schema(json!({ "pattern": s_str("Regular expression (Rust regex syntax). Escape special characters for literal searches."), "path": s_str("Directory or file to search (default: workspace root)"), "include": s_str("Comma-separated glob(s) to restrict files, e.g. *.ts,*.tsx or src/**/*.rs"), "case_sensitive": s_bool("Case-sensitive search (default: smart case)"), "max_results": s_int("Maximum matches to return (default 50, max 200)") }), &["pattern"]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "search" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::ReadOnly }
    fn label(&self, i: &Value) -> Option<String> { arg_str(i, &["pattern", "query"]).map(String::from) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let pattern = arg_str(&input, &["pattern", "query", "regex"]).ok_or_else(|| CaretError::invalid("pattern is required"))?.to_string();
        let root = arg_str(&input, &["path", "search_path", "directory"]).map(|p| ctx.resolve(p)).unwrap_or_else(|| ctx.cwd.clone());
        if !root.exists() { return Ok(ToolOutput::error(format!("{} does not exist", root.display()))); }
        let include = arg_str(&input, &["include", "include_pattern", "glob"]).map(String::from);
        let cs = arg_bool(&input, &["case_sensitive"]);
        let max = arg_u64(&input, &["max_results"]).unwrap_or(50).clamp(1, 200) as usize;
        let display_root = ctx.root.clone().unwrap_or_else(|| ctx.cwd.clone());
        let (hits, limited) = tokio::task::spawn_blocking(move || grep(&root, &pattern, include.as_deref(), cs, max, &display_root)).await.map_err(|e| CaretError::internal(e.to_string()))??;
        let n = hits.len();
        let (text, _) = cap_output(&format_hits(&hits, limited, max), ctx.max_output());
        Ok(ToolOutput::text(text).with_summary(format!("{n} match{}", if n == 1 { "" } else { "es" })))
    }
}

/// Fuzzy subsequence score: higher is better; None if not a subsequence.
pub fn fuzzy_score(candidate: &str, query: &str) -> Option<i32> {
    if query.is_empty() { return Some(0); }
    let c: Vec<char> = candidate.chars().collect(); let q: Vec<char> = query.to_lowercase().chars().collect();
    let base = candidate.rsplit('/').next().unwrap_or(candidate).to_lowercase();
    let mut score = 0i32; let mut qi = 0; let mut last = -1i32;
    for (i, ch) in c.iter().enumerate() {
        if qi < q.len() && ch.to_lowercase().next() == Some(q[qi]) {
            score += 10; if last == i as i32 - 1 { score += 8; } if i == 0 || matches!(c[i - 1], '/' | '_' | '-' | '.') { score += 6; }
            last = i as i32; qi += 1;
        }
    }
    if qi < q.len() { return None; }
    if base.contains(&query.to_lowercase()) { score += 40; }
    if base == query.to_lowercase() { score += 60; }
    score -= (c.len() as i32) / 8;
    Some(score)
}

pub struct FileSearch;
#[async_trait::async_trait]
impl Tool for FileSearch {
    fn name(&self) -> &str { "file_search" }
    fn display_name(&self) -> &str { "Find files" }
    fn description(&self) -> String { "Fuzzy search for files by name or path fragment (like a quick-open box). Use when you know part of a filename but not where it lives. Returns up to 20 matching paths.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "query": s_str("Filename or path fragment, e.g. `userService` or `routes/api`") }), &["query"]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "search" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::ReadOnly }
    fn label(&self, i: &Value) -> Option<String> { arg_str(i, &["query"]).map(String::from) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let q = arg_str(&input, &["query", "pattern", "name"]).ok_or_else(|| CaretError::invalid("query is required"))?.to_string();
        let files = ctx.manager.file_list(&ctx.cwd).await;
        let mut scored: Vec<(i32, &String)> = files.iter().filter_map(|f| fuzzy_score(f, &q).map(|s| (s, f))).collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())));
        if scored.is_empty() { return Ok(ToolOutput::text(format!("No files matching `{q}` ({} files indexed).", files.len()))); }
        let list: Vec<String> = scored.iter().take(20).map(|(_, f)| (*f).clone()).collect();
        Ok(ToolOutput::text(list.join("\n")).with_summary(format!("{} file{}", list.len(), if list.len() == 1 { "" } else { "s" })))
    }
}

pub struct CodebaseSearch;
#[async_trait::async_trait]
impl Tool for CodebaseSearch {
    fn name(&self) -> &str { "codebase_search" }
    fn display_name(&self) -> &str { "Search codebase" }
    fn description(&self) -> String { "Semantic search over the indexed codebase: describe what you are looking for in natural language (e.g. \"where are JWT tokens validated\") and get the most relevant code chunks with file paths and line ranges. Use this to explore unfamiliar code; use grep_search for exact strings.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "query": s_str("Natural-language description of the code you want to find"), "target_directories": s_arr("Optional directory prefixes to restrict results", json!({"type": "string"})) }), &["query"]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "search" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::ReadOnly }
    fn label(&self, i: &Value) -> Option<String> { arg_str(i, &["query"]).map(String::from) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let q = arg_str(&input, &["query"]).ok_or_else(|| CaretError::invalid("query is required"))?.to_string();
        let prefix = input.get("target_directories").and_then(|v| v.as_array()).and_then(|a| a.first()).and_then(|v| v.as_str()).map(String::from);
        if let Some(port) = ports::codebase() {
            if port.is_ready() {
                let hits = port.search(&q, 12, prefix.as_deref()).await?;
                if hits.is_empty() { return Ok(ToolOutput::text("No relevant code found in the index. Try grep_search with specific identifiers.")); }
                let mut out = String::new();
                for h in &hits { out.push_str(&format!("### {}:{}-{}{}\n```\n{}\n```\n\n", h.path, h.start_line, h.end_line, h.name.as_ref().map(|n| format!(" ({n})")).unwrap_or_default(), h.snippet.trim_end())); }
                let (text, _) = cap_output(&out, ctx.max_output());
                return Ok(ToolOutput::text(text).with_summary(format!("{} result{}", hits.len(), if hits.len() == 1 { "" } else { "s" })));
            }
        }
        // Lexical fallback: OR of significant query tokens.
        let toks: Vec<String> = q.split(|c: char| !c.is_alphanumeric() && c != '_').filter(|t| t.len() >= 4).map(|t| regex::escape(t)).collect();
        if toks.is_empty() { return Ok(ToolOutput::text("Codebase index is not ready and the query has no searchable identifiers; try grep_search.")); }
        let pattern = toks.join("|");
        let root = prefix.map(|p| ctx.resolve(&p)).unwrap_or_else(|| ctx.cwd.clone());
        let display_root = ctx.root.clone().unwrap_or_else(|| ctx.cwd.clone());
        let (hits, limited) = tokio::task::spawn_blocking(move || grep(&root, &pattern, None, Some(false), 40, &display_root)).await.map_err(|e| CaretError::internal(e.to_string()))??;
        let (text, _) = cap_output(&format!("(Index not ready — lexical fallback over query terms)\n{}", format_hits(&hits, limited, 40)), ctx.max_output());
        Ok(ToolOutput::text(text).with_summary(format!("{} lexical hit{}", hits.len(), if hits.len() == 1 { "" } else { "s" })))
    }
}

pub struct SearchDocs;
#[async_trait::async_trait]
impl Tool for SearchDocs {
    fn name(&self) -> &str { "search_docs" }
    fn display_name(&self) -> &str { "Search docs" }
    fn description(&self) -> String { "Search the documentation sources the user has indexed (@Docs) — library/framework docs crawled into the local index. Use for API usage questions about third-party libraries.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "query": s_str("What to look up"), "doc_ids": s_arr("Restrict to these doc source ids", json!({"type": "string"})) }), &["query"]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "search" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::ReadOnly }
    fn label(&self, i: &Value) -> Option<String> { arg_str(i, &["query"]).map(String::from) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let q = arg_str(&input, &["query"]).ok_or_else(|| CaretError::invalid("query is required"))?;
        let ids: Option<Vec<String>> = input.get("doc_ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
        let Some(port) = ports::codebase() else { return Ok(ToolOutput::error("Docs search is not available (indexing service not running)")) };
        let hits = port.search_docs(q, ids.as_deref(), 8).await?;
        if hits.is_empty() { return Ok(ToolOutput::text("No documentation matched. The user can add docs in Settings → Indexing & Docs.")); }
        let mut out = String::new();
        for h in &hits { out.push_str(&format!("### {} — {}\n{}\n\n", h.title, h.url, h.snippet.trim())); }
        let (text, _) = cap_output(&out, ctx.max_output());
        Ok(ToolOutput::text(text).with_summary(format!("{} doc hit{}", hits.len(), if hits.len() == 1 { "" } else { "s" })))
    }
}

pub fn list_files(root: &Path, limit: usize) -> Vec<String> {
    if crate::paths::is_broad_root(root) { return vec![]; }
    let mut walker = ignore::WalkBuilder::new(root);
    walker.hidden(true).git_ignore(true).git_global(true).git_exclude(true).add_custom_ignore_filename(".caretignore").add_custom_ignore_filename(".cursorignore");
    let mut v = vec![];
    for e in walker.build().flatten() {
        if !e.file_type().map(|t| t.is_file()).unwrap_or(false) { continue; }
        if e.path().components().any(|c| c.as_os_str() == ".git") { continue; }
        v.push(e.path().strip_prefix(root).unwrap_or(e.path()).to_string_lossy().to_string());
        if v.len() >= limit { break; }
    }
    v
}
pub fn list_dirs(root: &Path, limit: usize) -> Vec<String> {
    if crate::paths::is_broad_root(root) { return vec![]; }
    let mut walker = ignore::WalkBuilder::new(root);
    walker.hidden(true).git_ignore(true).git_global(true).git_exclude(true).max_depth(Some(6));
    let mut v = vec![];
    for e in walker.build().flatten() {
        if e.depth() == 0 || !e.file_type().map(|t| t.is_dir()).unwrap_or(false) { continue; }
        if e.path().components().any(|c| c.as_os_str() == ".git") { continue; }
        v.push(e.path().strip_prefix(root).unwrap_or(e.path()).to_string_lossy().to_string());
        if v.len() >= limit { break; }
    }
    v
}
pub fn _unused(_p: PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fuzzy_prefers_basename() {
        assert!(fuzzy_score("src/services/userService.ts", "userservice").unwrap() > fuzzy_score("src/user/service/index.ts", "userservice").unwrap());
        assert!(fuzzy_score("a/b/c.rs", "zzz").is_none());
    }
    #[test]
    fn grep_finds_lines() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "fn alpha() {}\nfn beta() {}\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "alpha beta\n").unwrap();
        let (hits, limited) = grep(dir.path(), "alpha", None, None, 10, dir.path()).unwrap();
        assert_eq!(hits.len(), 2); assert!(!limited);
        let (hits, _) = grep(dir.path(), "alpha", Some("*.rs"), None, 10, dir.path()).unwrap();
        assert_eq!(hits.len(), 1); assert_eq!(hits[0].line, 1);
    }
}

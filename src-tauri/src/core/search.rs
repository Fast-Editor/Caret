//! Workspace text search (ripgrep crates) streamed over a Channel, cancellation, and replace-in-files.
use crate::core::workspace::WorkspaceState;
use crate::error::{CaretError, Result};
use dashmap::DashMap;
use grep_matcher::Matcher;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkContext, SinkContextKind, SinkMatch};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchQuery {
    pub query: String,
    pub is_regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub max_results: Option<usize>,
    pub use_ignore_files: bool,
    pub context_lines: Option<usize>,
    pub roots: Option<Vec<String>>,
}
impl Default for SearchQuery {
    fn default() -> Self { Self { query: String::new(), is_regex: false, case_sensitive: false, whole_word: false, includes: vec![], excludes: vec![], max_results: None, use_ignore_files: true, context_lines: None, roots: None } }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub path: String,
    pub line: u64,
    pub column: u32,
    pub line_text: String,
    pub match_start: u32,
    pub match_end: u32,
    #[serde(skip_serializing_if = "Option::is_none")] pub before: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")] pub after: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SearchStreamEvent {
    Match { #[serde(rename = "match")] m: SearchMatch },
    Done { total: usize, files: usize, limited: bool, elapsed_ms: u64 },
    Error { message: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceRequest { pub query: SearchQuery, pub replacement: String, #[serde(default)] pub paths: Option<Vec<String>> }

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceResult { pub files: usize, pub replacements: usize }

#[derive(Default)]
pub struct SearchState { active: Arc<DashMap<String, Arc<AtomicBool>>> }

const DEFAULT_MAX_RESULTS: usize = 20_000;
const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024;
const MAX_LINE_CHARS: usize = 1000;
const MAX_PER_FILE: usize = 2000;

fn build_walker(roots: &[PathBuf], q: &SearchQuery) -> Result<ignore::WalkBuilder> {
    let Some((first, rest)) = roots.split_first() else { return Err(CaretError::no_workspace()) };
    let mut wb = ignore::WalkBuilder::new(first);
    for r in rest { wb.add(r); }
    let ig = q.use_ignore_files;
    wb.hidden(false).git_ignore(ig).git_global(ig).git_exclude(ig).ignore(ig).parents(ig).require_git(false).follow_links(false).max_filesize(Some(MAX_FILE_SIZE))
        .threads(std::thread::available_parallelism().map(|n| n.get().min(8)).unwrap_or(4));
    if ig { wb.add_custom_ignore_filename(".caretignore"); wb.add_custom_ignore_filename(".cursorignore"); }
    if !q.includes.is_empty() || !q.excludes.is_empty() {
        let mut ob = ignore::overrides::OverrideBuilder::new(first);
        for g in &q.includes { let g = g.trim(); if !g.is_empty() { ob.add(g).map_err(|e| CaretError::invalid(format!("bad include glob '{g}': {e}")))?; } }
        for g in &q.excludes { let g = g.trim(); if !g.is_empty() { let pat = if let Some(r) = g.strip_prefix('!') { r.to_string() } else { format!("!{g}") }; ob.add(&pat).map_err(|e| CaretError::invalid(format!("bad exclude glob '{g}': {e}")))?; } }
        wb.overrides(ob.build().map_err(|e| CaretError::invalid(e.to_string()))?);
    }
    wb.filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        !(is_dir && (name == ".git" || name == ".caret-index"))
    });
    Ok(wb)
}

fn build_matcher(q: &SearchQuery) -> Result<grep_regex::RegexMatcher> {
    let pattern = if q.is_regex { q.query.clone() } else { regex::escape(&q.query) };
    grep_regex::RegexMatcherBuilder::new()
        .case_insensitive(!q.case_sensitive).case_smart(false).word(q.whole_word).multi_line(false).line_terminator(Some(b'\n'))
        .build(&pattern).map_err(|e| CaretError::invalid(format!("invalid pattern: {e}")))
}

fn char_idx(bytes: &[u8], byte_off: usize) -> u32 { String::from_utf8_lossy(&bytes[..byte_off.min(bytes.len())]).chars().count() as u32 }

/// Collects matches for one file.
struct FileSink<'a> {
    matcher: &'a grep_regex::RegexMatcher,
    path: String,
    ctx: usize,
    before: VecDeque<String>,
    matches: Vec<SearchMatch>,
    total: &'a AtomicUsize,
    max: usize,
    cancel: &'a AtomicBool,
}
impl<'a> Sink for FileSink<'a> {
    type Error = std::io::Error;
    fn matched(&mut self, _s: &Searcher, m: &SinkMatch<'_>) -> std::result::Result<bool, Self::Error> {
        if self.cancel.load(Ordering::Relaxed) || self.total.load(Ordering::Relaxed) >= self.max || self.matches.len() >= MAX_PER_FILE { return Ok(false); }
        let mut line = m.bytes();
        while let Some((&last, rest)) = line.split_last() { if last == b'\n' || last == b'\r' { line = rest; } else { break; } }
        let line_no = m.line_number().unwrap_or(0);
        let mut text = String::from_utf8_lossy(line).to_string();
        if text.chars().count() > MAX_LINE_CHARS { text = text.chars().take(MAX_LINE_CHARS).collect::<String>() + "…"; }
        let before = if self.ctx > 0 && !self.before.is_empty() { Some(self.before.drain(..).collect()) } else { None };
        let mut first = true;
        let mut ranges = vec![];
        let _ = self.matcher.find_iter(line, |r| { ranges.push((r.start(), r.end())); ranges.len() < 200 });
        if ranges.is_empty() { ranges.push((0, line.len().min(1))); }
        for (s, e) in ranges {
            let ms = char_idx(line, s); let me = char_idx(line, e);
            self.matches.push(SearchMatch { path: self.path.clone(), line: line_no, column: ms + 1, line_text: text.clone(), match_start: ms, match_end: me, before: if first { before.clone() } else { None }, after: if self.ctx > 0 { Some(vec![]) } else { None } });
            first = false;
            let n = self.total.fetch_add(1, Ordering::Relaxed) + 1;
            if n >= self.max { return Ok(false); }
        }
        Ok(true)
    }
    fn context(&mut self, _s: &Searcher, c: &SinkContext<'_>) -> std::result::Result<bool, Self::Error> {
        if self.ctx == 0 { return Ok(true); }
        let mut t = String::from_utf8_lossy(c.bytes()).to_string();
        while t.ends_with('\n') || t.ends_with('\r') { t.pop(); }
        match c.kind() {
            SinkContextKind::Before => { if self.before.len() >= self.ctx { self.before.pop_front(); } self.before.push_back(t); }
            SinkContextKind::After => { if let Some(last) = self.matches.last_mut() { if let Some(a) = last.after.as_mut() { if a.len() < self.ctx { a.push(t); } } } }
            SinkContextKind::Other => {}
        }
        Ok(true)
    }
    fn context_break(&mut self, _s: &Searcher) -> std::result::Result<bool, Self::Error> { self.before.clear(); Ok(true) }
}

fn run_search(roots: Vec<PathBuf>, q: SearchQuery, on_event: Channel<SearchStreamEvent>, cancel: Arc<AtomicBool>) {
    let t0 = std::time::Instant::now();
    let send = |ev: SearchStreamEvent| { let _ = on_event.send(ev); };
    if q.query.is_empty() { send(SearchStreamEvent::Done { total: 0, files: 0, limited: false, elapsed_ms: 0 }); return; }
    let matcher = match build_matcher(&q) { Ok(m) => m, Err(e) => { send(SearchStreamEvent::Error { message: e.message }); return; } };
    let wb = match build_walker(&roots, &q) { Ok(w) => w, Err(e) => { send(SearchStreamEvent::Error { message: e.message }); return; } };
    let max = q.max_results.unwrap_or(DEFAULT_MAX_RESULTS).max(1);
    let ctx = q.context_lines.unwrap_or(0).min(10);
    let total = AtomicUsize::new(0);
    let files = AtomicUsize::new(0);
    let limited = AtomicBool::new(false);
    wb.build_parallel().run(|| {
        let mut searcher = SearcherBuilder::new().line_number(true).before_context(ctx).after_context(ctx).binary_detection(BinaryDetection::quit(0)).build();
        let matcher = &matcher; let total = &total; let files = &files; let limited = &limited; let cancel = &*cancel; let on_event = on_event.clone();
        Box::new(move |res| {
            if cancel.load(Ordering::Relaxed) { return ignore::WalkState::Quit; }
            if total.load(Ordering::Relaxed) >= max { limited.store(true, Ordering::Relaxed); return ignore::WalkState::Quit; }
            let Ok(entry) = res else { return ignore::WalkState::Continue };
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) { return ignore::WalkState::Continue; }
            let path = entry.path().to_string_lossy().to_string();
            let mut sink = FileSink { matcher, path, ctx, before: VecDeque::new(), matches: Vec::new(), total, max, cancel };
            if let Err(e) = searcher.search_path(matcher, entry.path(), &mut sink) { tracing::debug!(error = %e, path = %entry.path().display(), "search skipped"); }
            if !sink.matches.is_empty() {
                files.fetch_add(1, Ordering::Relaxed);
                for m in sink.matches { let _ = on_event.send(SearchStreamEvent::Match { m }); }
            }
            ignore::WalkState::Continue
        })
    });
    let t = total.load(Ordering::Relaxed);
    send(SearchStreamEvent::Done { total: t, files: files.load(Ordering::Relaxed), limited: limited.load(Ordering::Relaxed) || t >= max, elapsed_ms: t0.elapsed().as_millis() as u64 });
}

fn roots_for(app: &AppHandle, q: &SearchQuery) -> Result<Vec<PathBuf>> {
    let ws = app.state::<WorkspaceState>().root();
    if let Some(rs) = q.roots.as_ref().filter(|r| !r.is_empty()) { return Ok(rs.iter().map(|r| crate::paths::resolve(ws.as_deref(), r)).collect()); }
    ws.map(|r| vec![r]).ok_or_else(CaretError::no_workspace)
}

#[tauri::command]
pub async fn search_text(app: AppHandle, st: State<'_, SearchState>, query: SearchQuery, on_event: Channel<SearchStreamEvent>) -> Result<String> {
    let roots = roots_for(&app, &query)?;
    let id = uuid::Uuid::new_v4().to_string();
    let cancel = Arc::new(AtomicBool::new(false));
    st.active.insert(id.clone(), cancel.clone());
    let active = st.active.clone();
    let id2 = id.clone();
    tokio::task::spawn_blocking(move || { run_search(roots, query, on_event, cancel); active.remove(&id2); });
    Ok(id)
}

#[tauri::command]
pub fn search_cancel(st: State<'_, SearchState>, search_id: String) -> Result<()> {
    if let Some((_, c)) = st.active.remove(&search_id) { c.store(true, Ordering::Relaxed); }
    Ok(())
}

fn build_regex(q: &SearchQuery) -> Result<regex::Regex> {
    let mut pattern = if q.is_regex { q.query.clone() } else { regex::escape(&q.query) };
    if q.whole_word { pattern = format!(r"\b(?:{pattern})\b"); }
    regex::RegexBuilder::new(&pattern).case_insensitive(!q.case_sensitive).multi_line(true).build().map_err(|e| CaretError::invalid(format!("invalid pattern: {e}")))
}

pub fn replace_in_files(roots: &[PathBuf], req: &ReplaceRequest, restrict: Option<&[PathBuf]>) -> Result<ReplaceResult> {
    let re = build_regex(&req.query)?;
    let replacement = if req.query.is_regex { req.replacement.clone() } else { req.replacement.replace('$', "$$") };
    let mut files = 0usize; let mut count = 0usize;
    let mut apply = |p: &Path| -> Result<()> {
        let md = std::fs::metadata(p)?;
        if !md.is_file() || md.len() > MAX_FILE_SIZE { return Ok(()); }
        let bytes = std::fs::read(p)?;
        let (text, _, bin) = crate::core::fs::decode_bytes(&bytes);
        if bin { return Ok(()); }
        let n = re.find_iter(&text).count();
        if n == 0 { return Ok(()); }
        let out = re.replace_all(&text, replacement.as_str());
        crate::core::fs::write_atomic(p, out.as_bytes())?;
        files += 1; count += n;
        Ok(())
    };
    if let Some(list) = restrict { for p in list { apply(p)?; } }
    else {
        let wb = build_walker(roots, &req.query)?;
        for e in wb.build().flatten() { if e.file_type().map(|t| t.is_file()).unwrap_or(false) { apply(e.path())?; } }
    }
    Ok(ReplaceResult { files, replacements: count })
}

#[tauri::command]
pub async fn search_replace(app: AppHandle, req: ReplaceRequest) -> Result<ReplaceResult> {
    let roots = roots_for(&app, &req.query)?;
    let ws = app.state::<WorkspaceState>().root();
    let restrict: Option<Vec<PathBuf>> = req.paths.as_ref().map(|ps| ps.iter().map(|p| crate::paths::resolve(ws.as_deref(), p)).collect());
    tokio::task::spawn_blocking(move || replace_in_files(&roots, &req, restrict.as_deref())).await.map_err(|e| CaretError::internal(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("src")).unwrap();
        std::fs::create_dir_all(d.path().join("node_modules/pkg")).unwrap();
        std::fs::write(d.path().join("src/a.ts"), "const Foo = 1;\nfoo();\nlet foobar = Foo + foo;\n").unwrap();
        std::fs::write(d.path().join("src/b.rs"), "fn foo() {}\n// FOO bar\n").unwrap();
        std::fs::write(d.path().join("node_modules/pkg/i.js"), "foo foo foo\n").unwrap();
        std::fs::write(d.path().join(".gitignore"), "node_modules\n").unwrap();
        d
    }
    fn collect(roots: Vec<PathBuf>, q: SearchQuery) -> Vec<SearchMatch> {
        let matcher = build_matcher(&q).unwrap();
        let wb = build_walker(&roots, &q).unwrap();
        let total = AtomicUsize::new(0); let cancel = AtomicBool::new(false);
        let mut out = vec![];
        let mut searcher = SearcherBuilder::new().line_number(true).before_context(q.context_lines.unwrap_or(0)).after_context(q.context_lines.unwrap_or(0)).build();
        for e in wb.build().flatten() {
            if !e.file_type().map(|t| t.is_file()).unwrap_or(false) { continue; }
            let mut sink = FileSink { matcher: &matcher, path: e.path().to_string_lossy().to_string(), ctx: q.context_lines.unwrap_or(0), before: VecDeque::new(), matches: vec![], total: &total, max: 1000, cancel: &cancel };
            searcher.search_path(&matcher, e.path(), &mut sink).unwrap();
            out.extend(sink.matches);
        }
        out.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)).then(a.match_start.cmp(&b.match_start)));
        out
    }
    #[test]
    fn literal_case_insensitive_respects_ignore() {
        let d = fixture();
        let m = collect(vec![d.path().to_path_buf()], SearchQuery { query: "foo".into(), ..Default::default() });
        assert!(m.iter().all(|x| !x.path.contains("node_modules")));
        // a.ts: Foo | foo | foobar, Foo, foo = 5 ; b.rs: foo + FOO = 2
        assert_eq!(m.len(), 7, "{:?}", m.iter().map(|x| (&x.path, x.line, x.match_start)).collect::<Vec<_>>());
        let first = &m[0]; assert_eq!(first.line, 1); assert_eq!(first.column, 7); assert_eq!(first.match_start, 6); assert_eq!(first.match_end, 9);
    }
    #[test]
    fn case_sensitive_and_whole_word_and_regex() {
        let d = fixture();
        let r = d.path().to_path_buf();
        assert_eq!(collect(vec![r.clone()], SearchQuery { query: "Foo".into(), case_sensitive: true, ..Default::default() }).len(), 2);
        // whole word, case-insensitive: Foo, foo, Foo, foo (a.ts; "foobar" excluded) + foo, FOO (b.rs)
        assert_eq!(collect(vec![r.clone()], SearchQuery { query: "foo".into(), whole_word: true, ..Default::default() }).len(), 6);
        let rx = collect(vec![r.clone()], SearchQuery { query: r"fo+\(\)".into(), is_regex: true, ..Default::default() });
        assert_eq!(rx.len(), 2);
        let inc = collect(vec![r.clone()], SearchQuery { query: "foo".into(), includes: vec!["*.rs".into()], ..Default::default() });
        assert!(inc.iter().all(|x| x.path.ends_with(".rs")) && inc.len() == 2);
        let exc = collect(vec![r], SearchQuery { query: "foo".into(), excludes: vec!["*.rs".into()], ..Default::default() });
        assert!(exc.iter().all(|x| x.path.ends_with(".ts")));
    }
    #[test]
    fn context_lines_attach() {
        let d = fixture();
        let m = collect(vec![d.path().to_path_buf()], SearchQuery { query: "foobar".into(), context_lines: Some(1), ..Default::default() });
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].before.as_deref(), Some(&["foo();".to_string()][..]));
        assert_eq!(m[0].after.as_deref(), Some(&[][..]));
    }
    #[test]
    fn replace_counts_and_writes() {
        let d = fixture();
        let req = ReplaceRequest { query: SearchQuery { query: "foo".into(), whole_word: true, case_sensitive: true, ..Default::default() }, replacement: "qux".into(), paths: None };
        let r = replace_in_files(&[d.path().to_path_buf()], &req, None).unwrap();
        assert_eq!((r.files, r.replacements), (2, 3));
        assert_eq!(std::fs::read_to_string(d.path().join("src/a.ts")).unwrap(), "const Foo = 1;\nqux();\nlet foobar = Foo + qux;\n");
        let req2 = ReplaceRequest { query: SearchQuery { query: r"(F)oo".into(), is_regex: true, case_sensitive: true, ..Default::default() }, replacement: "${1}um".into(), paths: None };
        let r2 = replace_in_files(&[d.path().to_path_buf()], &req2, None).unwrap();
        assert_eq!(r2.replacements, 2);
        assert!(std::fs::read_to_string(d.path().join("src/a.ts")).unwrap().contains("const Fum = 1;"));
    }
}

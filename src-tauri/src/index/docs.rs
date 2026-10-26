//! @Docs: crawl documentation sites (same host, bounded) into per-source BM25 indexes.
use crate::error::{CaretError, Result};
use crate::events;
use crate::index::chunker::Chunk;
use crate::index::store::Store;
use crate::paths;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::AppHandle;
use url::Url;

pub const DOCS_CHANGED: &str = "caret://docs/changed";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocSource {
    pub id: String, pub name: String, pub url: String, pub pages: u32, pub chunks: u32, pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub error: Option<String>,
    pub added_at: u64, pub max_pages: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocSearchHit { pub doc_id: String, pub url: String, pub title: String, pub snippet: String, pub score: f32 }

#[derive(Serialize, Deserialize)]
struct ChunkLine { url: String, idx: u32, title: String, text: String }

struct DocIndex { store: Arc<Store>, chunks: Vec<ChunkLine> }

pub struct DocsInner {
    app: AppHandle,
    sources: Mutex<Vec<DocSource>>,
    open: Mutex<lru::LruCache<String, Arc<DocIndex>>>,
    crawling: Mutex<HashSet<String>>,
}

#[derive(Clone)]
pub struct DocsManager { inner: Arc<DocsInner> }

fn docs_dir() -> PathBuf { paths::sub_dir("docs") }
fn sources_path() -> PathBuf { docs_dir().join("sources.json") }
fn now_ms() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) }

static RE_TITLE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").unwrap());
static RE_HREF: Lazy<Regex> = Lazy::new(|| Regex::new(r#"(?i)href\s*=\s*["']([^"'>]+)["']"#).unwrap());
static RE_STRIP: Lazy<Vec<Regex>> = Lazy::new(|| ["script", "style", "nav", "header", "footer", "aside", "noscript", "svg", "form", "iframe"].iter().map(|t| Regex::new(&format!(r"(?is)<{t}\b[^>]*>.*?</{t}\s*>")).unwrap()).collect());
static RE_COMMENT: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)<!--.*?-->").unwrap());
static RE_BLANKS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n{3,}").unwrap());
const SKIP_EXT: &[&str] = &["png", "jpg", "jpeg", "gif", "svg", "webp", "ico", "pdf", "zip", "gz", "tgz", "tar", "css", "js", "mjs", "json", "xml", "rss", "atom", "woff", "woff2", "ttf", "mp4", "mp3", "wasm", "map"];

/// HTML → plain text (boilerplate stripped), title.
pub fn html_to_text(html: &str) -> (String, String) {
    let title = RE_TITLE.captures(html).and_then(|c| c.get(1)).map(|m| decode_entities(m.as_str().trim())).unwrap_or_default();
    let cleaned = RE_COMMENT.replace_all(html, "");
    let mut cleaned = cleaned.to_string();
    for re in RE_STRIP.iter() { cleaned = re.replace_all(&cleaned, "\n").to_string(); }
    let text = html2text::config::plain().allow_width_overflow().string_from_read(cleaned.as_bytes(), 2000)
        .or_else(|_| html2text::from_read(cleaned.as_bytes(), 2000))
        .unwrap_or_default();
    let text = text.lines().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n");
    (title, RE_BLANKS.replace_all(&text, "\n\n").trim().to_string())
}

fn decode_entities(s: &str) -> String { s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&nbsp;", " ") }

/// Split text into ~`target` char chunks on paragraph boundaries.
pub fn chunk_text(text: &str, target: usize) -> Vec<String> {
    let mut out = vec![]; let mut cur = String::new();
    for para in text.split("\n\n") {
        let para = para.trim(); if para.is_empty() { continue; }
        if !cur.is_empty() && cur.len() + para.len() + 2 > target { out.push(std::mem::take(&mut cur)); }
        if para.len() > target * 2 { // very long paragraph: hard split
            let mut s = para; while s.len() > target { let cut = s.char_indices().nth(target).map(|(i, _)| i).unwrap_or(s.len()); let (a, b) = s.split_at(cut); if !cur.is_empty() { out.push(std::mem::take(&mut cur)); } out.push(a.to_string()); s = b; }
            cur.push_str(s);
        } else { if !cur.is_empty() { cur.push_str("\n\n"); } cur.push_str(para); }
    }
    if !cur.trim().is_empty() { out.push(cur); }
    out
}

/// Extract same-host links (optionally under `prefix`) from HTML.
pub fn extract_links(base: &Url, html: &str, prefix: Option<&str>) -> Vec<Url> {
    let mut out = vec![]; let mut seen = HashSet::new();
    for c in RE_HREF.captures_iter(html) {
        let href = c[1].split('#').next().unwrap_or("");
        if href.is_empty() || href.starts_with("mailto:") || href.starts_with("javascript:") || href.starts_with("tel:") { continue; }
        let Ok(mut u) = base.join(href) else { continue };
        u.set_fragment(None);
        if u.scheme() != "http" && u.scheme() != "https" { continue; }
        if u.host_str() != base.host_str() { continue; }
        if let Some(p) = prefix { if !u.path().starts_with(p) { continue; } }
        if let Some(ext) = u.path().rsplit('.').next() { if u.path().contains('.') && SKIP_EXT.contains(&ext.to_ascii_lowercase().as_str()) { continue; } }
        let s = u.to_string();
        if seen.insert(s) { out.push(u); }
    }
    out
}

fn parse_robots(txt: &str) -> Vec<String> {
    let mut disallow = vec![]; let mut applies = false;
    for line in txt.lines() {
        let l = line.split('#').next().unwrap_or("").trim();
        if let Some(ua) = l.strip_prefix("User-agent:").or_else(|| l.strip_prefix("user-agent:")) { let ua = ua.trim(); applies = ua == "*" || ua.eq_ignore_ascii_case("caret"); }
        else if applies { if let Some(d) = l.strip_prefix("Disallow:").or_else(|| l.strip_prefix("disallow:")) { let d = d.trim(); if !d.is_empty() { disallow.push(d.to_string()); } } }
    }
    disallow
}

impl DocsManager {
    pub fn new(app: AppHandle) -> Self {
        let sources: Vec<DocSource> = std::fs::read(sources_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let mut sources = sources;
        for s in &mut sources { if s.status == "crawling" || s.status == "queued" { s.status = if s.pages > 0 { "ready".into() } else { "error".into() }; if s.pages == 0 { s.error = Some("Crawl interrupted".into()); } } }
        Self { inner: Arc::new(DocsInner { app, sources: Mutex::new(sources), open: Mutex::new(lru::LruCache::new(std::num::NonZeroUsize::new(4).unwrap())), crawling: Mutex::new(HashSet::new()) }) }
    }
    fn save(&self) { let s = self.inner.sources.lock(); let _ = std::fs::create_dir_all(docs_dir()); let _ = std::fs::write(sources_path(), serde_json::to_vec_pretty(&*s).unwrap_or_default()); drop(s); events::emit(&self.inner.app, DOCS_CHANGED, self.list()); }
    pub fn list(&self) -> Vec<DocSource> { self.inner.sources.lock().clone() }
    fn update<F: FnOnce(&mut DocSource)>(&self, id: &str, f: F) { if let Some(s) = self.inner.sources.lock().iter_mut().find(|s| s.id == id) { f(s); } self.save(); }

    fn open_index(&self, id: &str) -> Result<Arc<DocIndex>> {
        if let Some(d) = self.inner.open.lock().get(id) { return Ok(d.clone()); }
        let dir = docs_dir().join(id);
        let store = Arc::new(Store::open(&dir.join("tantivy"))?);
        let chunks: Vec<ChunkLine> = std::fs::read_to_string(dir.join("chunks.jsonl")).map(|s| s.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()).unwrap_or_default();
        let d = Arc::new(DocIndex { store, chunks });
        self.inner.open.lock().put(id.to_string(), d.clone());
        Ok(d)
    }

    pub fn add(&self, url: &str, name: Option<&str>, max_pages: Option<u32>, prefix: Option<&str>) -> Result<DocSource> {
        let parsed = Url::parse(url).map_err(|e| CaretError::invalid(format!("Invalid URL: {e}")))?;
        let id = format!("doc-{}", &blake3::hash(url.as_bytes()).to_hex()[..12]);
        let name = name.map(String::from).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| parsed.host_str().unwrap_or("docs").to_string());
        let src = DocSource { id: id.clone(), name, url: url.to_string(), pages: 0, chunks: 0, status: "queued".into(), error: None, added_at: now_ms(), max_pages: max_pages.unwrap_or(50).clamp(1, 500), prefix: prefix.map(String::from) };
        { let mut s = self.inner.sources.lock(); s.retain(|x| x.id != id); s.push(src.clone()); }
        self.save();
        self.spawn_crawl(id);
        Ok(src)
    }
    pub fn remove(&self, id: &str) -> Result<()> {
        { let mut s = self.inner.sources.lock(); s.retain(|x| x.id != id); }
        self.inner.open.lock().pop(id);
        let _ = std::fs::remove_dir_all(docs_dir().join(id));
        self.save(); Ok(())
    }
    pub fn refresh(&self, id: &str) -> Result<()> {
        if !self.inner.sources.lock().iter().any(|s| s.id == id) { return Err(CaretError::not_found(format!("doc {id}"))); }
        self.inner.open.lock().pop(id);
        let _ = std::fs::remove_dir_all(docs_dir().join(id));
        self.update(id, |s| { s.status = "queued".into(); s.pages = 0; s.chunks = 0; s.error = None; });
        self.spawn_crawl(id.to_string()); Ok(())
    }

    fn spawn_crawl(&self, id: String) {
        if !self.inner.crawling.lock().insert(id.clone()) { return; }
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let res = me.crawl(&id).await;
            me.inner.crawling.lock().remove(&id);
            if let Err(e) = res { me.update(&id, |s| { s.status = "error".into(); s.error = Some(e.message.clone()); }); }
        });
    }

    async fn crawl(&self, id: &str) -> Result<()> {
        let Some(src) = self.list().into_iter().find(|s| s.id == id) else { return Ok(()) };
        self.update(id, |s| { s.status = "crawling".into(); s.error = None; });
        let start = Url::parse(&src.url).map_err(|e| CaretError::invalid(e.to_string()))?;
        let client = reqwest::Client::builder().user_agent("Caret-Docs/0.1 (+https://caret.dev)").timeout(std::time::Duration::from_secs(10)).redirect(reqwest::redirect::Policy::limited(5)).build()?;
        let disallow = match client.get(format!("{}://{}/robots.txt", start.scheme(), start.host_str().unwrap_or(""))).send().await { Ok(r) if r.status().is_success() => parse_robots(&r.text().await.unwrap_or_default()), _ => vec![] };
        let allowed = |u: &Url| !disallow.iter().any(|d| u.path().starts_with(d.as_str()));
        let dir = docs_dir().join(id);
        std::fs::create_dir_all(&dir)?;
        let store = Store::open(&dir.join("tantivy"))?;
        let mut chunks_file = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("chunks.jsonl"))?;
        let mut queue: VecDeque<Url> = VecDeque::from([start.clone()]);
        let mut visited: HashSet<String> = HashSet::from([start.to_string()]);
        let (mut pages, mut nchunks) = (0u32, 0u32);
        let prefix = src.prefix.as_deref().filter(|p| !p.is_empty());
        while pages < src.max_pages && !queue.is_empty() {
            let mut batch = vec![]; while batch.len() < 2 { if let Some(u) = queue.pop_front() { if allowed(&u) { batch.push(u); } } else { break; } }
            let fetches = batch.iter().map(|u| { let c = client.clone(); let u = u.clone(); async move { let r = c.get(u.clone()).send().await; (u, r) } });
            let results = futures::future::join_all(fetches).await;
            for (u, r) in results {
                let Ok(resp) = r else { continue };
                if !resp.status().is_success() { continue; }
                let ct = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("text/html").to_string();
                if !(ct.contains("html") || ct.contains("text/plain") || ct.contains("markdown")) { continue; }
                let Ok(bytes) = resp.bytes().await else { continue };
                let bytes = if bytes.len() > 2 * 1024 * 1024 { bytes.slice(0..2 * 1024 * 1024) } else { bytes };
                let html = String::from_utf8_lossy(&bytes).to_string();
                let (title, text) = if ct.contains("html") { html_to_text(&html) } else { (u.path().to_string(), html.clone()) };
                if text.len() < 40 { continue; }
                let title = if title.is_empty() { u.to_string() } else { title };
                let chunks: Vec<Chunk> = chunk_text(&text, 1500).into_iter().enumerate().map(|(i, t)| Chunk { start_line: i as u32, end_line: i as u32, text: format!("{title}\n{t}"), name: Some(title.clone()), kind: Some("doc".into()) }).collect();
                for (i, c) in chunks.iter().enumerate() {
                    use std::io::Write;
                    let line = ChunkLine { url: u.to_string(), idx: i as u32, title: title.clone(), text: c.text.splitn(2, '\n').nth(1).unwrap_or("").to_string() };
                    let _ = writeln!(chunks_file, "{}", serde_json::to_string(&line).unwrap_or_default());
                }
                let hash = blake3::hash(bytes.as_ref()).to_hex()[..16].to_string();
                store.add_chunks(u.as_str(), &hash, &chunks)?;
                pages += 1; nchunks += chunks.len() as u32;
                if ct.contains("html") { for l in extract_links(&u, &html, prefix) { if visited.len() < 5000 && visited.insert(l.to_string()) { queue.push_back(l); } } }
                if pages >= src.max_pages { break; }
            }
            if pages % 10 == 0 { store.commit()?; self.update(id, |s| { s.pages = pages; s.chunks = nchunks; }); }
            tokio::task::yield_now().await;
        }
        store.commit()?;
        self.inner.open.lock().pop(id);
        if pages == 0 { self.update(id, |s| { s.status = "error".into(); s.error = Some("No readable pages found".into()); }); }
        else { self.update(id, |s| { s.status = "ready".into(); s.pages = pages; s.chunks = nchunks; s.error = None; }); }
        Ok(())
    }

    pub async fn search(&self, query: &str, doc_ids: Option<&[String]>, limit: usize) -> Result<Vec<DocSearchHit>> {
        let sources: Vec<DocSource> = self.list().into_iter().filter(|s| s.status == "ready" && doc_ids.map(|ids| ids.iter().any(|i| i == &s.id)).unwrap_or(true)).collect();
        let mut out: Vec<DocSearchHit> = vec![];
        for s in sources {
            let Ok(idx) = self.open_index(&s.id) else { continue };
            let hits = { let st = idx.store.clone(); let q = query.to_string(); tokio::task::spawn_blocking(move || st.search(&q, limit, None)).await.map_err(|e| CaretError::internal(e.to_string()))?? };
            for h in hits {
                let snippet = idx.chunks.iter().find(|c| c.url == h.path && c.idx == h.start_line).map(|c| c.text.chars().take(1200).collect::<String>()).unwrap_or_default();
                out.push(DocSearchHit { doc_id: s.id.clone(), url: h.path, title: h.name.unwrap_or_else(|| s.name.clone()), snippet, score: h.score });
            }
        }
        out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        out.truncate(limit.max(1));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn html_to_text_strips_boilerplate() {
        let html = "<html><head><title>Guide &amp; Ref</title><style>.x{}</style></head><body><nav><a href='/a'>nav</a></nav><main><h1>Install</h1><p>Run <code>npm i</code> to install.</p><script>alert(1)</script></main><footer>foot</footer></body></html>";
        let (title, text) = html_to_text(html);
        assert_eq!(title, "Guide & Ref");
        assert!(text.contains("Install")); assert!(text.contains("npm i"));
        assert!(!text.contains("alert")); assert!(!text.contains("foot")); assert!(!text.contains("nav"));
    }
    #[test]
    fn links_same_host_and_prefix() {
        let base = Url::parse("https://docs.example.com/guide/intro").unwrap();
        let html = r#"<a href="/guide/a">a</a> <a href="b#frag">b</a> <a href="https://other.com/x">x</a> <a href="/api/c">c</a> <a href="/img.png">img</a> <a href="mailto:x@y">m</a>"#;
        let links: Vec<String> = extract_links(&base, html, Some("/guide")).iter().map(|u| u.to_string()).collect();
        assert_eq!(links, vec!["https://docs.example.com/guide/a", "https://docs.example.com/guide/b"]);
        assert_eq!(extract_links(&base, html, None).len(), 3);
    }
    #[test]
    fn chunks_by_paragraph() {
        let text = (0..20).map(|i| format!("Paragraph {i} {}", "word ".repeat(40))).collect::<Vec<_>>().join("\n\n");
        let c = chunk_text(&text, 1500);
        assert!(c.len() >= 3);
        assert!(c.iter().all(|x| x.len() <= 1600));
        assert!(c[0].starts_with("Paragraph 0"));
    }
    #[test]
    fn robots_parse() {
        let d = parse_robots("User-agent: Googlebot\nDisallow: /g\n\nUser-agent: *\nDisallow: /private\nDisallow: /tmp # x\n");
        assert_eq!(d, vec!["/private", "/tmp"]);
    }
}

//! Web tools: web_search (DuckDuckGo HTML / SearXNG / Brave / Tavily) and fetch_url.
use super::{arg_str, arg_u64, cap_output, s_int, s_str, schema, Tool, ToolContext, ToolOutput};
use crate::ai::agent::policy::ToolEffect;
use crate::error::{CaretError, Result};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn tools() -> Vec<Arc<dyn Tool>> { vec![Arc::new(WebSearch), Arc::new(FetchUrl)] }

const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15 Caret/0.1";

pub fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder().user_agent(UA).timeout(std::time::Duration::from_secs(25)).redirect(reqwest::redirect::Policy::limited(6)).build().map_err(|e| CaretError::internal(e.to_string()))
}

#[derive(Debug, Clone)]
pub struct WebResult { pub title: String, pub url: String, pub snippet: String }

fn html_unescape(s: &str) -> String { s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#x27;", "'").replace("&#39;", "'").replace("&nbsp;", " ") }
fn strip_tags(s: &str) -> String { let re = regex::Regex::new(r"<[^>]+>").unwrap(); html_unescape(&re.replace_all(s, "")).split_whitespace().collect::<Vec<_>>().join(" ") }

pub fn parse_ddg(html: &str, max: usize) -> Vec<WebResult> {
    // Segment on each result anchor; each segment holds its title/href and (usually) its snippet.
    let anchor = regex::Regex::new(r#"(?s)<a[^>]*class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#).unwrap();
    let snippet_re = regex::Regex::new(r#"(?s)class="result__snippet"[^>]*>(.*?)</(?:a|td|div|span)>"#).unwrap();
    let mut out = vec![];
    let positions: Vec<(usize, usize)> = anchor.find_iter(html).map(|m| (m.start(), m.end())).collect();
    for (i, (start, end)) in positions.iter().enumerate() {
        let seg_end = positions.get(i + 1).map(|p| p.0).unwrap_or(html.len());
        let Some(c) = anchor.captures(&html[*start..*end]) else { continue };
        let mut url = html_unescape(&c[1]);
        if let Some(pos) = url.find("uddg=") { let enc = &url[pos + 5..]; let enc = enc.split('&').next().unwrap_or(enc); url = percent_decode(enc); }
        if url.starts_with("//") { url = format!("https:{url}"); }
        let title = strip_tags(&c[2]);
        let snippet = snippet_re.captures(&html[*end..seg_end]).map(|m| strip_tags(&m[1])).unwrap_or_default();
        if url.starts_with("http") && !title.is_empty() && !url.contains("duckduckgo.com/y.js") { out.push(WebResult { title, url, snippet }); }
        if out.len() >= max { break; }
    }
    out
}
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes(); let mut out = Vec::with_capacity(bytes.len()); let mut i = 0;
    while i < bytes.len() { if bytes[i] == b'%' && i + 2 < bytes.len() { if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) { out.push(v); i += 3; continue; } } out.push(bytes[i]); i += 1; }
    String::from_utf8_lossy(&out).to_string()
}

pub async fn search(settings: &crate::core::settings::Settings, secret: impl Fn(&str) -> Option<String>, query: &str, max: usize) -> Result<Vec<WebResult>> {
    let c = client()?;
    match settings.agent.web_search_provider.as_str() {
        "searxng" => {
            let ep = settings.agent.web_search_endpoint.clone().ok_or_else(|| CaretError::config("SearXNG endpoint is not configured (Settings → Agent → Web search)"))?;
            let u = url::Url::parse_with_params(&format!("{}/search", ep.trim_end_matches('/')), &[("q", query), ("format", "json")]).map_err(|e| CaretError::config(e.to_string()))?;
            let v: Value = c.get(u).send().await?.error_for_status()?.json().await?;
            Ok(v["results"].as_array().map(|a| a.iter().take(max).map(|r| WebResult { title: r["title"].as_str().unwrap_or("").into(), url: r["url"].as_str().unwrap_or("").into(), snippet: r["content"].as_str().unwrap_or("").into() }).collect()).unwrap_or_default())
        }
        "brave" => {
            let key = settings.agent.web_search_api_key_ref.as_deref().and_then(|r| secret(r)).or_else(|| secret("brave")).ok_or_else(|| CaretError::config("Brave Search API key missing (secret id `brave`)"))?;
            let u = url::Url::parse_with_params("https://api.search.brave.com/res/v1/web/search", &[("q", query), ("count", max.to_string().as_str())]).map_err(|e| CaretError::config(e.to_string()))?;
            let v: Value = c.get(u).header("X-Subscription-Token", key).header("Accept", "application/json").send().await?.error_for_status()?.json().await?;
            Ok(v["web"]["results"].as_array().map(|a| a.iter().take(max).map(|r| WebResult { title: r["title"].as_str().unwrap_or("").into(), url: r["url"].as_str().unwrap_or("").into(), snippet: r["description"].as_str().unwrap_or("").into() }).collect()).unwrap_or_default())
        }
        "tavily" => {
            let key = settings.agent.web_search_api_key_ref.as_deref().and_then(|r| secret(r)).or_else(|| secret("tavily")).ok_or_else(|| CaretError::config("Tavily API key missing (secret id `tavily`)"))?;
            let v: Value = c.post("https://api.tavily.com/search").json(&json!({ "api_key": key, "query": query, "max_results": max })).send().await?.error_for_status()?.json().await?;
            Ok(v["results"].as_array().map(|a| a.iter().take(max).map(|r| WebResult { title: r["title"].as_str().unwrap_or("").into(), url: r["url"].as_str().unwrap_or("").into(), snippet: r["content"].as_str().unwrap_or("").into() }).collect()).unwrap_or_default())
        }
        _ => {
            let u = url::Url::parse_with_params("https://html.duckduckgo.com/html/", &[("q", query)]).map_err(|e| CaretError::config(e.to_string()))?;
            let html = c.get(u).send().await?.error_for_status()?.text().await?;
            Ok(parse_ddg(&html, max))
        }
    }
}

pub fn format_results(results: &[WebResult]) -> String {
    if results.is_empty() { return "No results.".into(); }
    results.iter().enumerate().map(|(i, r)| format!("{}. {}\n   {}\n   {}", i + 1, r.title, r.url, r.snippet)).collect::<Vec<_>>().join("\n\n")
}

pub struct WebSearch;
#[async_trait::async_trait]
impl Tool for WebSearch {
    fn name(&self) -> &str { "web_search" }
    fn display_name(&self) -> &str { "Web search" }
    fn description(&self) -> String { "Search the web for up-to-date information (library docs, error messages, release notes). Returns titles, URLs and snippets; follow up with fetch_url to read a page.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "query": s_str("Search query"), "max_results": s_int("Number of results (default 6, max 10)") }), &["query"]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "web" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::Web }
    fn label(&self, i: &Value) -> Option<String> { arg_str(i, &["query", "search_term"]).map(String::from) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let q = arg_str(&input, &["query", "search_term"]).ok_or_else(|| CaretError::invalid("query is required"))?;
        let max = arg_u64(&input, &["max_results"]).unwrap_or(6).clamp(1, 10) as usize;
        let app = ctx.app.clone();
        let secrets = move |id: &str| { use tauri::Manager; app.as_ref().and_then(|a| a.state::<crate::core::settings::SettingsState>().secret(id)) };
        let results = match search(&ctx.settings, secrets, q, max).await { Ok(r) => r, Err(e) => return Ok(ToolOutput::error(format!("Web search failed: {}", e.message))) };
        let (text, _) = cap_output(&format_results(&results), ctx.max_output());
        Ok(ToolOutput::text(text).with_summary(format!("{} result{}", results.len(), if results.len() == 1 { "" } else { "s" })))
    }
}

pub async fn fetch_text(url: &str, max_chars: usize) -> Result<(String, String, bool)> {
    let c = client()?;
    let resp = c.get(url).header("Accept", "text/html,application/xhtml+xml,application/json,text/plain,*/*;q=0.8").send().await?;
    let status = resp.status();
    let ct = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let bytes = resp.bytes().await?;
    if !status.is_success() { return Err(CaretError::new("http", format!("HTTP {} for {url}", status.as_u16()))); }
    let bytes = if bytes.len() > 5 * 1024 * 1024 { bytes.slice(..5 * 1024 * 1024) } else { bytes };
    let body = String::from_utf8_lossy(&bytes).to_string();
    let text = if ct.contains("html") || body.trim_start().starts_with('<') { html2text::from_read(body.as_bytes(), 100).unwrap_or_else(|_| body.clone()) } else { body };
    let (capped, truncated) = cap_output(&text, max_chars);
    Ok((capped, ct, truncated))
}

pub struct FetchUrl;
#[async_trait::async_trait]
impl Tool for FetchUrl {
    fn name(&self) -> &str { "fetch_url" }
    fn display_name(&self) -> &str { "Fetch URL" }
    fn description(&self) -> String { "Fetch a web page or API URL and return its readable text (HTML is converted to plain text). Use after web_search or when the user shares a link.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "url": s_str("Absolute http(s) URL"), "max_chars": s_int("Maximum characters to return (default 20000)") }), &["url"]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "web" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::Web }
    fn label(&self, i: &Value) -> Option<String> { arg_str(i, &["url"]).map(String::from) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let url = arg_str(&input, &["url"]).ok_or_else(|| CaretError::invalid("url is required"))?;
        if !url.starts_with("http://") && !url.starts_with("https://") { return Ok(ToolOutput::error("Only http(s) URLs are supported")); }
        let max = arg_u64(&input, &["max_chars"]).unwrap_or(20_000).clamp(500, ctx.max_output() as u64 * 2) as usize;
        match fetch_text(url, max).await { Ok((t, ct, truncated)) => Ok(ToolOutput::text(format!("{url} ({ct}){}\n\n{t}", if truncated { " [truncated]" } else { "" })).with_summary(format!("Fetched {}", url.chars().take(60).collect::<String>()))), Err(e) => Ok(ToolOutput::error(format!("Fetch failed: {}", e.message))) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_ddg_html() {
        let html = r#"<a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fdocs&amp;rut=x">Example <b>Docs</b></a> <a class="result__snippet" href="x">Some &amp; snippet</a>"#;
        let r = parse_ddg(html, 5);
        assert_eq!(r.len(), 1); assert_eq!(r[0].url, "https://example.com/docs"); assert_eq!(r[0].title, "Example Docs"); assert_eq!(r[0].snippet, "Some & snippet");
    }
}

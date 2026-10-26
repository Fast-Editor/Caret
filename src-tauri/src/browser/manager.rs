//! Browser windows for the agent. Each tab is a Tauri window labelled `browser-<id>`; the helper
//! script (snapshot.js) runs in every page and reports results via intercepted `caret-result:` navigations.
use crate::core::ports::{self, BrowserElement, BrowserPort, BrowserSnapshot};
use crate::error::{CaretError, Result};
use dashmap::DashMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;

pub const HELPER_JS: &str = include_str!("snapshot.js");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTab { pub id: String, pub url: String, pub title: String, pub label: String }

struct Pending { tx: Option<oneshot::Sender<(bool, String)>>, parts: BTreeMap<u32, String>, total: u32 }

pub struct BrowserInner {
    app: AppHandle,
    tabs: DashMap<String, BrowserTab>,
    pending: Arc<DashMap<String, Mutex<Pending>>>,
}

#[derive(Clone)]
pub struct BrowserManager { pub inner: Arc<BrowserInner> }

#[derive(Deserialize)]
struct JsResult { ok: bool, value: serde_json::Value }

impl BrowserManager {
    pub fn new(app: AppHandle) -> Self { Self { inner: Arc::new(BrowserInner { app, tabs: DashMap::new(), pending: Arc::new(DashMap::new()) }) } }
    pub fn install(&self) { ports::set_browser(Arc::new(BrowserPortImpl(self.inner.clone()))); }
}

impl BrowserInner {
    fn window(&self, tab: &str) -> Result<tauri::WebviewWindow> {
        let label = self.tabs.get(tab).map(|t| t.label.clone()).ok_or_else(|| CaretError::not_found(format!("browser tab {tab}")))?;
        self.app.get_webview_window(&label).ok_or_else(|| { self.tabs.remove(tab); CaretError::not_found(format!("browser window {label}")) })
    }

    /// Handle one `caret-result://r/?id=&seq=&total=&data=` navigation; returns true when consumed.
    fn on_result_nav(pending: &DashMap<String, Mutex<Pending>>, url: &url::Url) -> bool {
        if url.scheme() != "caret-result" { return false; }
        let q: BTreeMap<String, String> = url.query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let (Some(id), Some(seq), Some(total), Some(data)) = (q.get("id"), q.get("seq"), q.get("total"), q.get("data")) else { return true };
        let (Ok(seq), Ok(total)) = (seq.parse::<u32>(), total.parse::<u32>()) else { return true };
        if let Some(entry) = pending.get(id) {
            let mut p = entry.lock();
            p.total = total.max(1);
            p.parts.insert(seq, data.clone());
            if p.parts.len() as u32 >= p.total {
                let b64: String = p.parts.values().cloned().collect();
                use base64::Engine;
                let decoded = base64::engine::general_purpose::STANDARD.decode(b64.as_bytes()).ok().and_then(|b| String::from_utf8(b).ok()).unwrap_or_default();
                let parsed: std::result::Result<JsResult, _> = serde_json::from_str(&decoded);
                let out = match parsed { Ok(r) => (r.ok, if r.value.is_string() { r.value.as_str().unwrap_or("").to_string() } else { r.value.to_string() }), Err(e) => (false, format!("bad result payload: {e}")) };
                if let Some(tx) = p.tx.take() { let _ = tx.send(out); }
            }
        }
        true
    }

    pub async fn open(&self, url: &str, label_hint: Option<&str>) -> Result<BrowserTab> {
        let parsed = url::Url::parse(url).map_err(|e| CaretError::invalid(format!("invalid url: {e}")))?;
        if parsed.scheme() != "http" && parsed.scheme() != "https" { return Err(CaretError::invalid("only http(s) URLs can be opened")); }
        let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let label = format!("browser-{id}");
        let pending = self.pending.clone();
        let app = self.app.clone();
        let label2 = label.clone();
        let title = label_hint.map(|s| format!("Caret Browser — {s}")).unwrap_or_else(|| "Caret Browser".to_string());
        // Window creation must happen on the main thread.
        let (tx, rx) = oneshot::channel::<Result<()>>();
        let url2 = parsed.clone();
        self.app.run_on_main_thread(move || {
            let r = WebviewWindowBuilder::new(&app, &label2, WebviewUrl::External(url2))
                .title(&title)
                .inner_size(1200.0, 800.0)
                .initialization_script(HELPER_JS)
                .on_navigation(move |u| !BrowserInner::on_result_nav(&pending, u))
                .build()
                .map(|_| ())
                .map_err(|e| CaretError::internal(format!("create browser window: {e}")));
            let _ = tx.send(r);
        }).map_err(|e| CaretError::internal(e.to_string()))?;
        rx.await.map_err(|_| CaretError::internal("window creation cancelled"))??;
        let tab = BrowserTab { id: id.clone(), url: url.to_string(), title: String::new(), label };
        self.tabs.insert(id, tab.clone());
        Ok(tab)
    }

    /// Evaluate `js` (an expression or async function body) inside the tab and return its JSON result.
    pub async fn eval_in(&self, tab: &str, js_fn_body: &str) -> Result<String> {
        let w = self.window(tab)?;
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.insert(id.clone(), Mutex::new(Pending { tx: Some(tx), parts: BTreeMap::new(), total: 1 }));
        let script = format!("(function(){{ if(!window.__caret){{ {HELPER} }} window.__caret.run({id:?}, async function(){{ {body}\n }}); }})();", HELPER = HELPER_JS, id = id, body = js_fn_body);
        if let Err(e) = w.eval(&script) { self.pending.remove(&id); return Err(CaretError::internal(format!("eval failed: {e}"))); }
        let res = tokio::time::timeout(Duration::from_secs(20), rx).await;
        self.pending.remove(&id);
        match res {
            Ok(Ok((true, v))) => Ok(v),
            Ok(Ok((false, e))) => Err(CaretError::new("browser_js", e)),
            Ok(Err(_)) => Err(CaretError::internal("browser result channel closed")),
            Err(_) => Err(CaretError::new("timeout", "browser script timed out (page may still be loading)")),
        }
    }

    pub async fn snapshot(&self, tab: &str) -> Result<BrowserSnapshot> {
        let v = self.eval_in(tab, "return window.__caret.snapshot();").await?;
        let snap: BrowserSnapshot = serde_json::from_str(&v).map_err(|e| CaretError::internal(format!("bad snapshot: {e}")))?;
        if let Some(mut t) = self.tabs.get_mut(tab) { t.url = snap.url.clone(); t.title = snap.title.clone(); }
        Ok(snap)
    }
    pub async fn navigate(&self, tab: &str, url: &str) -> Result<()> {
        let u = url::Url::parse(url).map_err(|e| CaretError::invalid(format!("invalid url: {e}")))?;
        let w = self.window(tab)?;
        w.navigate(u).map_err(|e| CaretError::internal(e.to_string()))?;
        if let Some(mut t) = self.tabs.get_mut(tab) { t.url = url.to_string(); }
        tokio::time::sleep(Duration::from_millis(800)).await;
        Ok(())
    }
    pub async fn click(&self, tab: &str, r: &str) -> Result<()> { self.eval_in(tab, &format!("return window.__caret.click({:?});", r)).await?; tokio::time::sleep(Duration::from_millis(400)).await; Ok(()) }
    pub async fn type_text(&self, tab: &str, r: &str, text: &str, submit: bool) -> Result<()> { self.eval_in(tab, &format!("return window.__caret.type({:?}, {:?}, {});", r, text, submit)).await?; if submit { tokio::time::sleep(Duration::from_millis(600)).await; } Ok(()) }
    pub async fn back(&self, tab: &str) -> Result<()> { let _ = self.eval_in(tab, "history.back(); return true;").await?; tokio::time::sleep(Duration::from_millis(500)).await; Ok(()) }
    pub async fn forward(&self, tab: &str) -> Result<()> { let _ = self.eval_in(tab, "history.forward(); return true;").await?; tokio::time::sleep(Duration::from_millis(500)).await; Ok(()) }
    pub fn close(&self, tab: &str) -> Result<()> { if let Ok(w) = self.window(tab) { let _ = w.close(); } self.tabs.remove(tab); Ok(()) }
    pub fn list(&self) -> Vec<BrowserTab> {
        // prune windows the user closed
        let dead: Vec<String> = self.tabs.iter().filter(|t| self.app.get_webview_window(&t.label).is_none()).map(|t| t.id.clone()).collect();
        for d in dead { self.tabs.remove(&d); }
        let mut v: Vec<BrowserTab> = self.tabs.iter().map(|t| t.clone()).collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }
}

pub struct BrowserPortImpl(pub Arc<BrowserInner>);

#[async_trait::async_trait]
impl BrowserPort for BrowserPortImpl {
    async fn open(&self, url: &str) -> Result<String> { Ok(self.0.open(url, Some("agent")).await?.id) }
    async fn navigate(&self, tab: &str, url: &str) -> Result<()> { self.0.navigate(tab, url).await }
    async fn snapshot(&self, tab: &str) -> Result<BrowserSnapshot> { self.0.snapshot(tab).await }
    async fn click(&self, tab: &str, r: &str) -> Result<()> { self.0.click(tab, r).await }
    async fn type_text(&self, tab: &str, r: &str, text: &str, submit: bool) -> Result<()> { self.0.type_text(tab, r, text, submit).await }
    async fn eval(&self, tab: &str, js: &str) -> Result<String> { self.0.eval_in(tab, js).await }
    async fn back(&self, tab: &str) -> Result<()> { self.0.back(tab).await }
    async fn close(&self, tab: &str) -> Result<()> { self.0.close(tab) }
    async fn list(&self) -> Vec<(String, String, String)> { self.0.list().into_iter().map(|t| (t.id, t.url, t.title)).collect() }
}

#[allow(dead_code)]
fn _assert_types(_: BrowserElement) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reassembles_chunked_results() {
        let pending: DashMap<String, Mutex<Pending>> = DashMap::new();
        let (tx, mut rx) = oneshot::channel();
        pending.insert("abc".into(), Mutex::new(Pending { tx: Some(tx), parts: BTreeMap::new(), total: 1 }));
        use base64::Engine;
        let payload = serde_json::json!({"ok": true, "value": {"title": "T", "n": 1}}).to_string();
        let b64 = base64::engine::general_purpose::STANDARD.encode(payload.as_bytes());
        let (a, b) = b64.split_at(b64.len() / 2);
        let mk = |seq: u32, data: &str| url::Url::parse(&format!("caret-result://r/?id=abc&seq={seq}&total=2&data={}", urlencoding(data))).unwrap();
        assert!(BrowserInner::on_result_nav(&pending, &mk(1, b)));
        assert!(rx.try_recv().is_err());
        assert!(BrowserInner::on_result_nav(&pending, &mk(0, a)));
        let (ok, v) = rx.try_recv().unwrap();
        assert!(ok); assert!(v.contains("\"title\":\"T\""));
        assert!(!BrowserInner::on_result_nav(&pending, &url::Url::parse("https://example.com").unwrap()));
    }
    fn urlencoding(s: &str) -> String { s.replace('+', "%2B").replace('/', "%2F").replace('=', "%3D") }
}

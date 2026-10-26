//! Dev-only automation bridge (enabled with `CARET_AUTOMATION=1`): a tiny localhost HTTP server
//! used by scripted testing to eval JS in the main webview and capture screenshots.
//! Endpoints: GET /health · POST /eval (body = JS, may `return` a value) · GET /screenshot
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

static PENDING: Lazy<DashMap<String, oneshot::Sender<(bool, String)>>> = Lazy::new(DashMap::new);

#[tauri::command]
pub fn automation_result(id: String, ok: bool, value: String) {
    if let Some((_, tx)) = PENDING.remove(&id) { let _ = tx.send((ok, value)); }
}

pub fn start(app: AppHandle) {
    if std::env::var("CARET_AUTOMATION").ok().as_deref() != Some("1") { return; }
    let port: u16 = std::env::var("CARET_AUTOMATION_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(17420);
    tauri::async_runtime::spawn(async move {
        let listener = match TcpListener::bind(("127.0.0.1", port)).await { Ok(l) => l, Err(e) => { tracing::error!("automation bind failed: {e}"); return; } };
        tracing::info!("automation bridge listening on http://127.0.0.1:{port}");
        let app = Arc::new(app);
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { continue };
            let app = app.clone();
            tokio::spawn(async move {
                let mut buf = Vec::with_capacity(8192);
                let mut tmp = [0u8; 4096];
                let (head_end, mut content_len) = loop {
                    let n = match sock.read(&mut tmp).await { Ok(0) | Err(_) => return, Ok(n) => n };
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..pos]).to_string();
                        let cl = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
                        break (pos + 4, cl);
                    }
                    if buf.len() > 1 << 20 { return; }
                };
                while buf.len() < head_end + content_len { let n = match sock.read(&mut tmp).await { Ok(0) | Err(_) => break, Ok(n) => n }; buf.extend_from_slice(&tmp[..n]); }
                content_len = content_len.min(buf.len() - head_end);
                let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                let body = String::from_utf8_lossy(&buf[head_end..head_end + content_len]).to_string();
                let line = head.lines().next().unwrap_or("");
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or("GET");
                let path = parts.next().unwrap_or("/");
                let (status, resp) = handle(&app, method, path, body).await;
                let _ = sock.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp}", resp.len()).as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
}

async fn handle(app: &AppHandle, method: &str, path: &str, body: String) -> (&'static str, String) {
    match (method, path) {
        ("GET", "/health") => ("200 OK", r#"{"ok":true}"#.into()),
        ("POST", "/eval") => {
            let Some(w) = app.get_webview_window("main") else { return ("500 Internal Server Error", r#"{"ok":false,"error":"no main window"}"#.into()) };
            let id = uuid::Uuid::new_v4().to_string();
            let (tx, rx) = oneshot::channel();
            PENDING.insert(id.clone(), tx);
            let js = format!(
                "(async()=>{{const __send=(ok,v)=>window.__TAURI_INTERNALS__.invoke('automation_result',{{id:'{id}',ok,value:String(v)}});try{{const r=await (async()=>{{{body}\n}})();__send(true,JSON.stringify(r===undefined?null:r));}}catch(e){{__send(false,((e&&e.message)||String(e))+'\\n'+((e&&e.stack)||''));}}}})();"
            );
            if let Err(e) = w.eval(&js) { PENDING.remove(&id); return ("500 Internal Server Error", format!(r#"{{"ok":false,"error":{}}}"#, serde_json::json!(e.to_string()))); }
            match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
                Ok(Ok((ok, value))) => ("200 OK", serde_json::json!({"ok": ok, "value": value}).to_string()),
                _ => { PENDING.remove(&id); ("504 Gateway Timeout", r#"{"ok":false,"error":"timeout"}"#.into()) }
            }
        }
        ("GET", p) if p.starts_with("/screenshot") => {
            let Some(w) = app.get_webview_window("main") else { return ("500 Internal Server Error", r#"{"ok":false,"error":"no main window"}"#.into()) };
            let _ = w.show(); let _ = w.set_focus(); let _ = w.unminimize();
            tokio::time::sleep(std::time::Duration::from_millis(450)).await;
            let scale = w.scale_factor().unwrap_or(2.0);
            let pos = w.outer_position().map(|p| (p.x as f64 / scale, p.y as f64 / scale)).unwrap_or((0.0, 0.0));
            let size = w.outer_size().map(|s| (s.width as f64 / scale, s.height as f64 / scale)).unwrap_or((1440.0, 920.0));
            let out = std::env::temp_dir().join(format!("caret-shot-{}.png", chrono::Utc::now().format("%H%M%S%3f")));
            let region = format!("{},{},{},{}", pos.0 as i64, pos.1 as i64, size.0 as i64, size.1 as i64);
            let status = tokio::process::Command::new("screencapture").args(["-x", "-R", &region, out.to_str().unwrap_or("")]).status().await;
            match status { Ok(s) if s.success() => ("200 OK", serde_json::json!({"ok": true, "path": out, "region": region}).to_string()), other => ("500 Internal Server Error", serde_json::json!({"ok": false, "error": format!("{other:?}")}).to_string()) }
        }
        _ => ("404 Not Found", r#"{"ok":false,"error":"not found"}"#.into()),
    }
}

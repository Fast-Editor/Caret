//! One DAP connection (stdio or TCP): writes framed requests, correlates responses by `request_seq`, and forwards
//! adapter events and reverse requests to the owning session.
use super::protocol::{encode, request, response, FrameParser};
use crate::error::{CaretError, Result};
use dashmap::DashMap;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, Mutex};

pub type Reader = Box<dyn AsyncRead + Unpin + Send>;
pub type Writer = Box<dyn AsyncWrite + Unpin + Send>;

/// What a connection reports upward.
#[derive(Debug)]
pub enum Incoming {
    Event { event: String, body: Value },
    /// Adapter → client request (`runInTerminal`, `startDebugging`); answer with `respond`.
    ReverseRequest { seq: i64, command: String, arguments: Value },
    Closed,
}

pub struct DapConnection {
    pub id: usize,
    seq: AtomicI64,
    pending: Arc<DashMap<i64, oneshot::Sender<Value>>>,
    writer: Mutex<Writer>,
    closed: Arc<AtomicBool>,
}

impl DapConnection {
    /// Start reading `reader`; events/reverse requests go to `tx` tagged with this connection's id.
    pub fn start(id: usize, reader: Reader, writer: Writer, tx: mpsc::UnboundedSender<(usize, Incoming)>) -> Arc<Self> {
        let conn = Arc::new(Self { id, seq: AtomicI64::new(1), pending: Arc::new(DashMap::new()), writer: Mutex::new(writer), closed: Arc::new(AtomicBool::new(false)) });
        let pending = conn.pending.clone(); let closed = conn.closed.clone();
        tokio::spawn(async move {
            let mut reader = reader; let mut parser = FrameParser::new(); let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = match reader.read(&mut buf).await { Ok(0) | Err(_) => break, Ok(n) => n };
                parser.feed(&buf[..n]);
                while let Some(msg) = parser.next() {
                    match msg.get("type").and_then(|t| t.as_str()) {
                        Some("response") => { if let Some(rs) = msg.get("request_seq").and_then(|v| v.as_i64()) { if let Some((_, s)) = pending.remove(&rs) { let _ = s.send(msg); } } }
                        Some("event") => { let _ = tx.send((id, Incoming::Event { event: msg.get("event").and_then(|e| e.as_str()).unwrap_or("").to_string(), body: msg.get("body").cloned().unwrap_or(Value::Null) })); }
                        Some("request") => { let _ = tx.send((id, Incoming::ReverseRequest { seq: msg.get("seq").and_then(|v| v.as_i64()).unwrap_or(0), command: msg.get("command").and_then(|c| c.as_str()).unwrap_or("").to_string(), arguments: msg.get("arguments").cloned().unwrap_or(Value::Null) })); }
                        _ => {}
                    }
                }
            }
            closed.store(true, Ordering::SeqCst);
            pending.clear();
            let _ = tx.send((id, Incoming::Closed));
        });
        conn
    }

    pub fn is_closed(&self) -> bool { self.closed.load(Ordering::SeqCst) }

    async fn write(&self, msg: &Value) -> Result<()> {
        let mut w = self.writer.lock().await;
        w.write_all(&encode(msg)).await.map_err(|e| CaretError::io(format!("debug adapter write failed: {e}")))?;
        w.flush().await.map_err(|e| CaretError::io(format!("debug adapter flush failed: {e}")))
    }

    /// Send a request and wait for its response body (`Err` when the adapter reports `success: false`).
    pub async fn request(&self, command: &str, arguments: Value) -> Result<Value> {
        self.request_timeout(command, arguments, std::time::Duration::from_secs(60)).await
    }

    pub async fn request_timeout(&self, command: &str, arguments: Value, timeout: std::time::Duration) -> Result<Value> {
        if self.is_closed() { return Err(CaretError::provider("debug adapter connection is closed")); }
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.insert(seq, tx);
        self.write(&request(seq, command, arguments)).await?;
        let msg = match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(m)) => m,
            Ok(Err(_)) => return Err(CaretError::provider(format!("debug adapter closed before answering `{command}`"))),
            Err(_) => { self.pending.remove(&seq); return Err(CaretError::provider(format!("debug adapter did not answer `{command}` within {}s", timeout.as_secs()))); }
        };
        if msg.get("success").and_then(|s| s.as_bool()).unwrap_or(false) { Ok(msg.get("body").cloned().unwrap_or(Value::Null)) }
        else {
            let m = msg.get("message").and_then(|m| m.as_str()).map(|s| s.to_string())
                .or_else(|| msg.pointer("/body/error/format").and_then(|f| f.as_str()).map(|s| s.to_string()))
                .unwrap_or_else(|| format!("`{command}` failed"));
            Err(CaretError::provider(m))
        }
    }

    /// Answer a reverse request.
    pub async fn respond(&self, request_seq: i64, command: &str, success: bool, body: Value, message: Option<&str>) -> Result<()> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        self.write(&response(seq, request_seq, command, success, body, message)).await
    }
}

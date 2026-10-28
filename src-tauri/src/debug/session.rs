//! One debug session: adapter process + one or more DAP connections (js-debug spawns child sessions through the
//! `startDebugging` reverse request), the configure-on-`initialized` dance, event forwarding and request routing.
use super::adapters::{self, Transport};
use super::client::{DapConnection, Incoming};
use super::{Breakpoint, BreakpointStore, DapThread, DebugEvent, EvaluateResult, EventSink, Logger, Scope, StackFrame};
use crate::error::{CaretError, Result};
use dashmap::DashMap;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

const ID_SHIFT: u32 = 24;
/// Connection-qualified id: adapters number threads / frames / variable references per connection.
pub fn gid(conn: usize, local: i64) -> i64 { if local < 0 { local } else { ((conn as i64) << ID_SHIFT) | (local & ((1 << ID_SHIFT) - 1)) } }
pub fn split_gid(g: i64) -> (usize, i64) { if g < 0 { (0, g) } else { ((g >> ID_SHIFT) as usize, g & ((1 << ID_SHIFT) - 1)) } }

pub struct DebugSession {
    pub id: String,
    pub name: String,
    pub adapter: String,
    pub kind: String,
    pub request: String,
    pub config: Value,
    inner: Arc<Inner>,
}

struct Inner {
    id: String,
    tcp_port: Option<u16>,
    conns: DashMap<usize, Arc<DapConnection>>,
    next_conn: AtomicUsize,
    active_conn: Mutex<Option<usize>>,
    capabilities: Mutex<Value>,
    state: Mutex<String>,
    tx: mpsc::UnboundedSender<(usize, Incoming)>,
    sink: EventSink,
    log: Logger,
    breakpoints: Arc<BreakpointStore>,
    child: Mutex<Option<tokio::process::Child>>,
    terminated: AtomicBool,
    kind: String,
}

impl Inner {
    fn set_state(&self, s: &str) { *self.state.lock() = s.into(); (self.sink)(DebugEvent::State { session_id: self.id.clone(), state: s.into() }); }
    fn conn(&self, id: usize) -> Result<Arc<DapConnection>> { self.conns.get(&id).map(|c| c.clone()).filter(|c| !c.is_closed()).ok_or_else(|| CaretError::provider("debug connection is gone")) }
    /// The connection to use when the caller has no id: the one that stopped last, else the newest live one.
    fn best_conn(&self) -> Result<Arc<DapConnection>> {
        if let Some(a) = *self.active_conn.lock() { if let Ok(c) = self.conn(a) { return Ok(c); } }
        let mut ids: Vec<usize> = self.conns.iter().filter(|e| !e.value().is_closed()).map(|e| *e.key()).collect(); ids.sort_unstable();
        ids.last().copied().map(|i| self.conn(i)).unwrap_or_else(|| Err(CaretError::provider("debug session has no live connection")))
    }

    async fn add_connection(self: &Arc<Self>, reader: super::client::Reader, writer: super::client::Writer) -> Arc<DapConnection> {
        let id = self.next_conn.fetch_add(1, Ordering::SeqCst);
        let c = DapConnection::start(id, reader, writer, self.tx.clone());
        self.conns.insert(id, c.clone());
        c
    }

    async fn initialize(&self, c: &DapConnection) -> Result<Value> {
        let caps = c.request("initialize", json!({
            "clientID": "caret", "clientName": "Caret", "adapterID": self.kind, "locale": "en", "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path",
            "supportsVariableType": true, "supportsVariablePaging": false, "supportsRunInTerminalRequest": false, "supportsProgressReporting": false, "supportsInvalidatedEvent": false,
            "supportsMemoryReferences": false, "supportsArgsCanBeInterpretedByShell": false, "supportsStartDebuggingRequest": true
        })).await?;
        Ok(caps)
    }

    /// `initialized` → breakpoints, exception breakpoints, configurationDone.
    async fn configure(&self, c: &DapConnection) {
        for (file, lines) in self.breakpoints.all() {
            let bps: Vec<Value> = lines.iter().map(|l| json!({ "line": l })).collect();
            if let Err(e) = c.request("setBreakpoints", json!({ "source": { "path": file, "name": std::path::Path::new(&file).file_name().map(|n| n.to_string_lossy().to_string()) }, "breakpoints": bps, "lines": lines })).await { (self.log)(format!("[debug] setBreakpoints {file}: {}", e.message)); }
        }
        let _ = c.request("setExceptionBreakpoints", json!({ "filters": [] })).await;
        let supports_done = self.capabilities.lock().get("supportsConfigurationDoneRequest").and_then(|v| v.as_bool()).unwrap_or(true);
        if supports_done {
            if let Err(e) = c.request("configurationDone", Value::Null).await { (self.log)(format!("[debug] configurationDone: {}", e.message)); }
        }
    }

    async fn start_child(self: &Arc<Self>, arguments: Value) -> Result<()> {
        let port = self.tcp_port.ok_or_else(|| CaretError::provider("startDebugging on a stdio adapter is not supported"))?;
        let stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.map_err(|e| CaretError::io(format!("child session connect: {e}")))?;
        let (r, w) = tokio::io::split(stream);
        let c = self.add_connection(Box::new(r), Box::new(w)).await;
        self.initialize(&c).await?;
        let req = arguments.get("request").and_then(|r| r.as_str()).unwrap_or("launch").to_string();
        let mut cfg = arguments.get("configuration").cloned().unwrap_or(json!({}));
        if let Some(t) = cfg.get("type").and_then(|t| t.as_str()).map(|s| s.to_string()) { cfg["type"] = Value::String(adapters::normalize_type(&t)); }
        let me = self.clone(); let cc = c.clone();
        tokio::spawn(async move { if let Err(e) = cc.request_timeout(&req, cfg, std::time::Duration::from_secs(300)).await { (me.log)(format!("[debug] child {req} failed: {}", e.message)); } });
        (self.log)(format!("[debug] child session connection #{} started", c.id));
        Ok(())
    }

    async fn handle(self: &Arc<Self>, conn: usize, msg: Incoming) {
        let sid = self.id.clone();
        match msg {
            Incoming::Event { event, body } => match event.as_str() {
                "initialized" => { if let Ok(c) = self.conn(conn) { self.configure(&c).await; } }
                "stopped" => {
                    *self.active_conn.lock() = Some(conn);
                    *self.state.lock() = "stopped".into();
                    let tid = body.get("threadId").and_then(|t| t.as_i64()).map(|t| gid(conn, t)).unwrap_or(gid(conn, 0));
                    (self.sink)(DebugEvent::Stopped { session_id: sid, thread_id: tid, reason: body.get("reason").and_then(|r| r.as_str()).unwrap_or("pause").into(), description: body.get("description").and_then(|d| d.as_str()).map(String::from), text: body.get("text").and_then(|d| d.as_str()).map(String::from), all_threads_stopped: body.get("allThreadsStopped").and_then(|a| a.as_bool()).unwrap_or(false), hit_breakpoint_ids: body.get("hitBreakpointIds").and_then(|h| h.as_array()).map(|a| a.iter().filter_map(|x| x.as_i64()).collect()).unwrap_or_default() });
                }
                "continued" => { *self.state.lock() = "running".into(); (self.sink)(DebugEvent::Continued { session_id: sid, thread_id: body.get("threadId").and_then(|t| t.as_i64()).map(|t| gid(conn, t)).unwrap_or(0), all_threads_continued: body.get("allThreadsContinued").and_then(|a| a.as_bool()).unwrap_or(true) }); }
                "output" => { (self.sink)(DebugEvent::Output { session_id: sid, category: body.get("category").and_then(|c| c.as_str()).unwrap_or("console").into(), output: body.get("output").and_then(|o| o.as_str()).unwrap_or("").into(), source: body.pointer("/source/path").and_then(|p| p.as_str()).map(String::from), line: body.get("line").and_then(|l| l.as_i64()) }); }
                "breakpoint" => { (self.sink)(DebugEvent::Breakpoint { session_id: sid, reason: body.get("reason").and_then(|r| r.as_str()).unwrap_or("changed").into(), breakpoint: bp_from(body.get("breakpoint").unwrap_or(&Value::Null), None) }); }
                "thread" => { (self.sink)(DebugEvent::Thread { session_id: sid, reason: body.get("reason").and_then(|r| r.as_str()).unwrap_or("").into(), thread_id: body.get("threadId").and_then(|t| t.as_i64()).map(|t| gid(conn, t)).unwrap_or(0) }); }
                "exited" => { (self.sink)(DebugEvent::Exited { session_id: sid, exit_code: body.get("exitCode").and_then(|c| c.as_i64()).unwrap_or(0) }); }
                "terminated" => {
                    let all_done = conn == 0 || self.conns.iter().all(|e| *e.key() == conn || e.value().is_closed());
                    if all_done { self.finish(false).await; }
                    else { (self.log)(format!("[debug] connection #{conn} terminated")); let mut a = self.active_conn.lock(); if *a == Some(conn) { *a = None; } }
                }
                "loadedSource" | "module" | "process" | "capabilities" | "progressStart" | "progressUpdate" | "progressEnd" | "invalidated" | "memory" => {}
                other => { (self.log)(format!("[debug] event {other} {}", body.to_string().chars().take(200).collect::<String>())); }
            },
            Incoming::ReverseRequest { seq, command, arguments } => {
                let Ok(c) = self.conn(conn) else { return };
                match command.as_str() {
                    "startDebugging" => {
                        let _ = c.respond(seq, &command, true, json!({}), None).await;
                        if let Err(e) = self.start_child(arguments).await { (self.sink)(DebugEvent::Error { session_id: sid, message: format!("could not start child debug session: {}", e.message) }); }
                    }
                    "runInTerminal" => { let _ = c.respond(seq, &command, false, Value::Null, Some("Caret does not support runInTerminal yet — use \"console\": \"internalConsole\" in the launch configuration")).await; }
                    other => { let _ = c.respond(seq, other, false, Value::Null, Some(&format!("unsupported reverse request {other}"))).await; }
                }
            }
            Incoming::Closed => {
                (self.log)(format!("[debug] connection #{conn} closed"));
                let all_closed = conn == 0 || self.conns.iter().all(|e| e.value().is_closed());
                if all_closed { self.finish(true).await; }
            }
        }
    }

    /// Mark the session terminated and tell the UI. `kill_now` kills the adapter immediately (user stop); otherwise
    /// the adapter gets a grace period so trailing `exited`/`output` events still arrive before it is killed.
    async fn finish(self: &Arc<Self>, kill_now: bool) {
        if self.terminated.swap(true, Ordering::SeqCst) { return; }
        self.set_state("terminated");
        (self.sink)(DebugEvent::Terminated { session_id: self.id.clone() });
        if kill_now { self.kill_adapter().await; }
        else { let me = self.clone(); tokio::spawn(async move { tokio::time::sleep(std::time::Duration::from_millis(1500)).await; me.kill_adapter().await; }); }
    }
    async fn kill_adapter(&self) {
        let child = self.child.lock().take(); // guard dropped before the await (parking_lot guards are !Send)
        if let Some(mut child) = child { let pid = child.id(); let _ = child.start_kill(); if let Some(p) = pid { crate::acp::process::kill_pid(p).await; } }
    }
}

fn bp_from(v: &Value, path: Option<&str>) -> Breakpoint {
    Breakpoint { id: v.get("id").and_then(|i| i.as_i64()), verified: v.get("verified").and_then(|b| b.as_bool()).unwrap_or(false), line: v.get("line").and_then(|l| l.as_i64()), message: v.get("message").and_then(|m| m.as_str()).map(String::from), path: v.pointer("/source/path").and_then(|p| p.as_str()).map(String::from).or_else(|| path.map(String::from)) }
}

fn map_frames(conn: usize, body: &Value) -> Vec<StackFrame> {
    body.get("stackFrames").and_then(|f| f.as_array()).map(|a| a.iter().map(|f| StackFrame {
        id: gid(conn, f.get("id").and_then(|i| i.as_i64()).unwrap_or(0)), name: f.get("name").and_then(|n| n.as_str()).unwrap_or("").into(),
        path: f.pointer("/source/path").and_then(|p| p.as_str()).map(String::from), source_name: f.pointer("/source/name").and_then(|p| p.as_str()).map(String::from),
        line: f.get("line").and_then(|l| l.as_i64()).unwrap_or(0), column: f.get("column").and_then(|c| c.as_i64()).unwrap_or(0), presentation_hint: f.get("presentationHint").and_then(|p| p.as_str()).map(String::from),
    }).collect()).unwrap_or_default()
}
fn map_vars(conn: usize, body: &Value) -> Vec<Variable> {
    body.get("variables").and_then(|v| v.as_array()).map(|a| a.iter().map(|v| Variable {
        name: v.get("name").and_then(|n| n.as_str()).unwrap_or("").into(), value: v.get("value").and_then(|n| n.as_str()).unwrap_or("").into(), kind: v.get("type").and_then(|t| t.as_str()).map(String::from),
        variables_reference: v.get("variablesReference").and_then(|r| r.as_i64()).map(|r| if r > 0 { gid(conn, r) } else { 0 }).unwrap_or(0),
        evaluate_name: v.get("evaluateName").and_then(|e| e.as_str()).map(String::from), indexed_variables: v.get("indexedVariables").and_then(|i| i.as_i64()), named_variables: v.get("namedVariables").and_then(|i| i.as_i64()),
    }).collect()).unwrap_or_default()
}
use super::Variable;

impl DebugSession {
    /// Start a session with a managed adapter chosen from the config `type`, or a custom stdio adapter command.
    pub async fn start(id: String, adapter: &str, config: Value, cwd: &std::path::Path, breakpoints: Arc<BreakpointStore>, sink: EventSink, log: Logger, custom: Option<(String, Vec<String>)>) -> Result<Arc<DebugSession>> {
        let launched = match &custom { Some((cmd, args)) => adapters::launch_custom(cmd, args, cwd, log.clone()).await?, None => adapters::launch(adapter, cwd, log.clone()).await? };
        let (tx, rx) = mpsc::unbounded_channel();
        let kind_raw = config.get("type").and_then(|t| t.as_str()).unwrap_or(adapter).to_string();
        let request = config.get("request").and_then(|r| r.as_str()).unwrap_or("launch").to_string();
        let name = config.get("name").and_then(|n| n.as_str()).unwrap_or("Debug").to_string();
        let tcp_port = match &launched.transport { Transport::Tcp { port } => Some(*port), _ => None };
        let inner = Arc::new(Inner { id: id.clone(), tcp_port, conns: DashMap::new(), next_conn: AtomicUsize::new(0), active_conn: Mutex::new(None), capabilities: Mutex::new(Value::Null), state: Mutex::new("starting".into()), tx, sink: sink.clone(), log: log.clone(), breakpoints, child: Mutex::new(launched.child), terminated: AtomicBool::new(false), kind: adapters::normalize_type(&kind_raw) });
        let conn = match launched.transport {
            Transport::Stdio { stdin, stdout } => inner.add_connection(Box::new(stdout), Box::new(stdin)).await,
            Transport::Tcp { port } => { let s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.map_err(|e| CaretError::io(format!("adapter connect: {e}")))?; let (r, w) = tokio::io::split(s); inner.add_connection(Box::new(r), Box::new(w)).await }
        };
        // event loop
        { let me = inner.clone(); let mut rx = rx; tokio::spawn(async move { while let Some((c, m)) = rx.recv().await { me.handle(c, m).await; if me.terminated.load(Ordering::SeqCst) && me.conns.iter().all(|e| e.value().is_closed()) { break; } } }); }
        sink(DebugEvent::SessionStarted { session_id: id.clone(), name: name.clone(), adapter: adapter.into(), kind: kind_raw.clone() });
        inner.set_state("starting");
        let caps = match inner.initialize(&conn).await { Ok(c) => c, Err(e) => { inner.finish(true).await; return Err(e); } };
        *inner.capabilities.lock() = caps.clone();
        sink(DebugEvent::Initialized { session_id: id.clone(), capabilities: caps });
        // launch / attach runs concurrently with the initialized → configurationDone handshake
        let mut cfg = config.clone(); cfg["type"] = Value::String(inner.kind.clone());
        if cfg.get("__caretSession").is_none() { cfg["__caretSession"] = Value::String(id.clone()); }
        { let me = inner.clone(); let c = conn.clone(); let req = request.clone();
          tokio::spawn(async move {
              match c.request_timeout(&req, cfg, std::time::Duration::from_secs(300)).await {
                  Ok(_) => { let starting = *me.state.lock() == "starting"; if starting { me.set_state("running"); } }
                  Err(e) => { (me.sink)(DebugEvent::Error { session_id: me.id.clone(), message: format!("{req} failed: {}", e.message) }); me.finish(true).await; }
              }
          }); }
        Ok(Arc::new(DebugSession { id, name, adapter: adapter.into(), kind: kind_raw, request, config, inner }))
    }

    pub fn state(&self) -> String { self.inner.state.lock().clone() }
    pub fn capabilities(&self) -> Value { self.inner.capabilities.lock().clone() }
    pub fn is_terminated(&self) -> bool { self.inner.terminated.load(Ordering::SeqCst) }

    pub async fn threads(&self) -> Result<Vec<DapThread>> {
        let mut out = vec![];
        let ids: Vec<usize> = self.inner.conns.iter().filter(|e| !e.value().is_closed()).map(|e| *e.key()).collect();
        for cid in ids { if let Ok(c) = self.inner.conn(cid) { if let Ok(b) = c.request_timeout("threads", Value::Null, std::time::Duration::from_secs(10)).await {
            for t in b.get("threads").and_then(|t| t.as_array()).cloned().unwrap_or_default() { out.push(DapThread { id: gid(cid, t.get("id").and_then(|i| i.as_i64()).unwrap_or(0)), name: t.get("name").and_then(|n| n.as_str()).unwrap_or("thread").into() }); }
        } } }
        Ok(out)
    }
    pub async fn stack_trace(&self, thread: i64) -> Result<Vec<StackFrame>> {
        let (cid, tid) = split_gid(thread); let c = self.inner.conn(cid)?;
        let b = c.request("stackTrace", json!({ "threadId": tid, "startFrame": 0, "levels": 60 })).await?;
        Ok(map_frames(cid, &b))
    }
    pub async fn scopes(&self, frame: i64) -> Result<Vec<Scope>> {
        let (cid, fid) = split_gid(frame); let c = self.inner.conn(cid)?;
        let b = c.request("scopes", json!({ "frameId": fid })).await?;
        Ok(b.get("scopes").and_then(|s| s.as_array()).map(|a| a.iter().map(|s| Scope { name: s.get("name").and_then(|n| n.as_str()).unwrap_or("").into(), variables_reference: s.get("variablesReference").and_then(|r| r.as_i64()).map(|r| if r > 0 { gid(cid, r) } else { 0 }).unwrap_or(0), expensive: s.get("expensive").and_then(|e| e.as_bool()).unwrap_or(false), presentation_hint: s.get("presentationHint").and_then(|p| p.as_str()).map(String::from) }).collect()).unwrap_or_default())
    }
    pub async fn variables(&self, reference: i64) -> Result<Vec<Variable>> {
        let (cid, r) = split_gid(reference); let c = self.inner.conn(cid)?;
        let b = c.request("variables", json!({ "variablesReference": r })).await?;
        Ok(map_vars(cid, &b))
    }
    pub async fn evaluate(&self, expression: &str, frame: Option<i64>, context: &str) -> Result<EvaluateResult> {
        let (c, fid) = match frame { Some(f) => { let (cid, fid) = split_gid(f); (self.inner.conn(cid)?, Some(fid)) } None => (self.inner.best_conn()?, None) };
        let cid = c.id;
        let mut args = json!({ "expression": expression, "context": context }); if let Some(f) = fid { args["frameId"] = json!(f); }
        let b = c.request("evaluate", args).await?;
        Ok(EvaluateResult { result: b.get("result").and_then(|r| r.as_str()).unwrap_or("").into(), kind: b.get("type").and_then(|t| t.as_str()).map(String::from), variables_reference: b.get("variablesReference").and_then(|r| r.as_i64()).map(|r| if r > 0 { gid(cid, r) } else { 0 }).unwrap_or(0) })
    }
    async fn thread_cmd(&self, cmd: &str, thread: i64) -> Result<()> {
        let (cid, tid) = split_gid(thread); let c = self.inner.conn(cid)?;
        c.request(cmd, json!({ "threadId": tid })).await?;
        if cmd != "pause" { *self.inner.state.lock() = "running".into(); (self.inner.sink)(DebugEvent::Continued { session_id: self.id.clone(), thread_id: thread, all_threads_continued: true }); }
        Ok(())
    }
    pub async fn continue_(&self, thread: i64) -> Result<()> { self.thread_cmd("continue", thread).await }
    pub async fn next(&self, thread: i64) -> Result<()> { self.thread_cmd("next", thread).await }
    pub async fn step_in(&self, thread: i64) -> Result<()> { self.thread_cmd("stepIn", thread).await }
    pub async fn step_out(&self, thread: i64) -> Result<()> { self.thread_cmd("stepOut", thread).await }
    pub async fn pause(&self, thread: i64) -> Result<()> { self.thread_cmd("pause", thread).await }
    pub async fn restart(&self) -> Result<()> { let c = self.inner.conn(0)?; c.request("restart", json!({ "arguments": self.config })).await.map(|_| ()) }

    /// Push the current breakpoint set for `file` to every live connection; returns the adapter's verification.
    pub async fn set_breakpoints(&self, file: &str, lines: &[u32]) -> Vec<Breakpoint> {
        let bps: Vec<Value> = lines.iter().map(|l| json!({ "line": l })).collect();
        let args = json!({ "source": { "path": file, "name": std::path::Path::new(file).file_name().map(|n| n.to_string_lossy().to_string()) }, "breakpoints": bps, "lines": lines });
        let mut best: Option<Vec<Breakpoint>> = None;
        let ids: Vec<usize> = self.inner.conns.iter().filter(|e| !e.value().is_closed()).map(|e| *e.key()).collect();
        for cid in ids { if let Ok(c) = self.inner.conn(cid) { if let Ok(b) = c.request_timeout("setBreakpoints", args.clone(), std::time::Duration::from_secs(10)).await {
            let list: Vec<Breakpoint> = b.get("breakpoints").and_then(|x| x.as_array()).map(|a| a.iter().map(|v| bp_from(v, Some(file))).collect()).unwrap_or_default();
            if best.as_ref().map(|p| p.iter().filter(|x| x.verified).count() < list.iter().filter(|x| x.verified).count()).unwrap_or(true) { best = Some(list); }
        } } }
        best.unwrap_or_else(|| lines.iter().map(|l| Breakpoint { id: None, verified: false, line: Some(*l as i64), message: None, path: Some(file.into()) }).collect())
    }

    /// Disconnect (terminating the debuggee) and kill the adapter.
    pub async fn stop(&self) {
        if let Ok(c) = self.inner.conn(0) { let _ = c.request_timeout("disconnect", json!({ "terminateDebuggee": true }), std::time::Duration::from_secs(3)).await; }
        self.inner.finish(true).await;
    }
}

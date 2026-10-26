//! Integrated terminal: portable-pty sessions streamed over Channels, bounded scrollback,
//! OSC title/cwd tracking, and a one-shot command runner.
use crate::core::ports;
use crate::core::settings::SettingsState;
use crate::core::workspace::WorkspaceState;
use crate::error::{CaretError, Result};
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PtySpawnOptions {
    pub cwd: Option<String>,
    pub shell: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<BTreeMap<String, String>>,
    pub cols: u16,
    pub rows: u16,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyInfo { pub id: String, pub pid: u32, pub title: String, pub cwd: String, pub shell: String, pub created_at: u64 }

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PtyEvent {
    Data { data: String },
    Exit { code: Option<i32> },
    Title { title: String },
    Cwd { cwd: String },
    /// OSC 133 semantic block started (command known when the shell integration announced it first).
    #[serde(rename = "block-start")] BlockStart { block: TerminalBlock },
    #[serde(rename = "block-command", rename_all = "camelCase")] BlockCommand { block_id: String, command: String },
    #[serde(rename = "block-end", rename_all = "camelCase")] BlockEnd { block_id: String, exit_code: Option<i32>, ended_at: u64, end_line: u64 },
    /// OSC 7501 program status (working / waiting / done / failed / idle).
    Status { status: String, #[serde(skip_serializing_if = "Option::is_none")] title: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] detail: Option<String> },
}

/// One shell command and its output, delimited by OSC 133 markers from the shell integration.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalBlock {
    pub id: String,
    pub index: u64,
    #[serde(skip_serializing_if = "Option::is_none")] pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub exit_code: Option<i32>,
    pub started_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")] pub ended_at: Option<u64>,
    /// Scrollback line offsets (newline count since the terminal started).
    pub start_line: u64,
    #[serde(skip_serializing_if = "Option::is_none")] pub end_line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub output_preview: Option<String>,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramStatusInfo { pub status: String, #[serde(skip_serializing_if = "Option::is_none")] pub title: Option<String>, #[serde(skip_serializing_if = "Option::is_none")] pub detail: Option<String> }

pub const BLOCK_OUTPUT_CAP: usize = 64 * 1024;
pub const BLOCK_TOTAL_CAP: usize = 2 * 1024 * 1024;

struct BlockRec { block: TerminalBlock, output: Vec<u8>, truncated: bool }

/// Per-terminal OSC 133 / 7501 bookkeeping. Pure state machine (no I/O) so it is unit-testable.
pub struct BlockTracker {
    blocks: VecDeque<BlockRec>,
    max_blocks: usize,
    total_bytes: usize,
    pub line_count: u64,
    next_index: u64,
    running: Option<String>,
    pending_command: Option<String>,
    cwd: Option<String>,
    track_blocks: bool,
    track_status: bool,
    pub status: Option<ProgramStatusInfo>,
    last_marker: Option<(char, u64)>,
}

pub enum BlockAction { Start(TerminalBlock), Command(String, String), End { block_id: String, exit_code: Option<i32>, ended_at: u64, end_line: u64 }, Status(ProgramStatusInfo) }

impl BlockTracker {
    pub fn new(max_blocks: usize, track_blocks: bool, track_status: bool) -> Self {
        Self { blocks: VecDeque::new(), max_blocks: max_blocks.clamp(1, 5000), total_bytes: 0, line_count: 0, next_index: 0, running: None, pending_command: None, cwd: None, track_blocks, track_status, status: None, last_marker: None }
    }
    pub fn set_cwd(&mut self, cwd: &str) { self.cwd = Some(cwd.to_string()); }
    /// Account plain output text: newline count + capture into the running block.
    pub fn on_text(&mut self, text: &str) {
        let nl = text.bytes().filter(|b| *b == b'\n').count() as u64;
        self.line_count += nl;
        if !self.track_blocks { return; }
        if let Some(id) = self.running.clone() {
            if let Some(rec) = self.blocks.iter_mut().find(|r| r.block.id == id) {
                if rec.output.len() < BLOCK_OUTPUT_CAP {
                    let room = BLOCK_OUTPUT_CAP - rec.output.len();
                    let take = text.len().min(room);
                    rec.output.extend_from_slice(&text.as_bytes()[..take]);
                    self.total_bytes += take;
                    if take < text.len() { rec.truncated = true; }
                } else { rec.truncated = true; }
            }
            self.enforce_total_cap();
        }
    }
    fn enforce_total_cap(&mut self) {
        if self.total_bytes <= BLOCK_TOTAL_CAP { return; }
        let running = self.running.clone();
        for rec in self.blocks.iter_mut() {
            if self.total_bytes <= BLOCK_TOTAL_CAP { break; }
            if Some(&rec.block.id) == running.as_ref() { continue; }
            if !rec.output.is_empty() { self.total_bytes -= rec.output.len(); rec.output = Vec::new(); rec.truncated = true; }
        }
    }
    pub fn on_command(&mut self, cmd: String) -> Option<BlockAction> {
        if !self.track_blocks { return None; }
        if let Some(id) = self.running.clone() {
            if let Some(rec) = self.blocks.iter_mut().find(|r| r.block.id == id) { if rec.block.command.is_none() { rec.block.command = Some(cmd.clone()); return Some(BlockAction::Command(id, cmd)); } }
            return None;
        }
        self.pending_command = Some(cmd);
        None
    }
    /// OSC 133 marker. `kind` ∈ A|B|C|D. Returns actions to emit (in order).
    pub fn on_prompt(&mut self, kind: char, exit: Option<i32>, now: u64) -> Vec<BlockAction> {
        if !self.track_blocks { return vec![]; }
        // dedupe identical consecutive markers (user shell + ours both emitting)
        if let Some((k, line)) = self.last_marker { if k == kind && line == self.line_count && kind != 'D' { return vec![]; } }
        self.last_marker = Some((kind, self.line_count));
        let mut out = vec![];
        match kind {
            'A' => { if let Some(a) = self.close_running(None, now) { out.push(a); } }
            'B' | 'C' => {
                if self.running.is_none() { out.push(self.start_block(now)); }
            }
            'D' => { if let Some(a) = self.close_running(exit, now) { out.push(a); } }
            _ => {}
        }
        out
    }
    fn start_block(&mut self, now: u64) -> BlockAction {
        let id = uuid::Uuid::new_v4().to_string();
        let block = TerminalBlock { id: id.clone(), index: self.next_index, command: self.pending_command.take(), cwd: self.cwd.clone(), exit_code: None, started_at: now, ended_at: None, start_line: self.line_count, end_line: None, output_preview: None, running: true };
        self.next_index += 1;
        self.blocks.push_back(BlockRec { block: block.clone(), output: Vec::new(), truncated: false });
        while self.blocks.len() > self.max_blocks { if let Some(old) = self.blocks.pop_front() { self.total_bytes = self.total_bytes.saturating_sub(old.output.len()); } }
        self.running = Some(id);
        BlockAction::Start(block)
    }
    fn close_running(&mut self, exit: Option<i32>, now: u64) -> Option<BlockAction> {
        let id = self.running.take()?;
        let line = self.line_count;
        if let Some(rec) = self.blocks.iter_mut().find(|r| r.block.id == id) {
            rec.block.exit_code = exit; rec.block.ended_at = Some(now); rec.block.end_line = Some(line); rec.block.running = false;
        }
        Some(BlockAction::End { block_id: id, exit_code: exit, ended_at: now, end_line: line })
    }
    pub fn on_status(&mut self, status: String, title: Option<String>, detail: Option<String>) -> Option<BlockAction> {
        if !self.track_status { return None; }
        let info = ProgramStatusInfo { status, title, detail };
        self.status = Some(info.clone());
        Some(BlockAction::Status(info))
    }
    pub fn blocks(&self) -> Vec<TerminalBlock> {
        self.blocks.iter().map(|r| { let mut b = r.block.clone(); let plain = strip_ansi(&String::from_utf8_lossy(&r.output)); let preview: String = plain.trim_start_matches(|c| c == '\r' || c == '\n').chars().take(200).collect(); b.output_preview = if preview.is_empty() { None } else { Some(preview) }; b }).collect()
    }
    pub fn output(&self, block_id: &str) -> Option<(String, bool)> {
        self.blocks.iter().find(|r| r.block.id == block_id).map(|r| (strip_ansi(&String::from_utf8_lossy(&r.output)), r.truncated))
    }
    pub fn last_output(&self) -> Option<String> { self.blocks.back().map(|r| strip_ansi(&String::from_utf8_lossy(&r.output))) }
    pub fn len(&self) -> usize { self.blocks.len() }
    pub fn is_empty(&self) -> bool { self.blocks.is_empty() }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCommandResult { pub stdout: String, pub stderr: String, pub code: Option<i32>, pub timed_out: bool }

const SCROLLBACK_BYTES: usize = 256 * 1024;
const COALESCE: Duration = Duration::from_millis(16);
const FLUSH_BYTES: usize = 64 * 1024;

pub struct RingBuf { buf: VecDeque<u8>, cap: usize }
impl RingBuf {
    pub fn new(cap: usize) -> Self { Self { buf: VecDeque::with_capacity(cap.min(64 * 1024)), cap } }
    pub fn push(&mut self, bytes: &[u8]) {
        if bytes.len() >= self.cap { self.buf.clear(); self.buf.extend(&bytes[bytes.len() - self.cap..]); return; }
        let overflow = (self.buf.len() + bytes.len()).saturating_sub(self.cap);
        if overflow > 0 { self.buf.drain(..overflow); }
        self.buf.extend(bytes);
    }
    pub fn text(&self) -> String { let (a, b) = self.buf.as_slices(); let mut v = Vec::with_capacity(self.buf.len()); v.extend_from_slice(a); v.extend_from_slice(b); String::from_utf8_lossy(&v).to_string() }
}

static ANSI_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r"(?s)\x1b\[[0-9;?<=>]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[()][A-Za-z0-9]|\x1b[=>78ME]|\x1bP.*?\x1b\\|\r").unwrap());

/// Strip ANSI/OSC/DCS control sequences for plain-text consumers (agent tools, @Terminal).
pub fn strip_ansi(s: &str) -> String { ANSI_RE.replace_all(s, "").to_string() }

pub fn tail_lines(text: &str, lines: usize) -> String {
    let plain = strip_ansi(text);
    let v: Vec<&str> = plain.lines().collect();
    let start = v.len().saturating_sub(lines);
    v[start..].join("\n")
}

pub struct PtySession {
    pub info: RwLock<PtyInfo>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    buffer: Mutex<RingBuf>,
    channel: Channel<PtyEvent>,
    alive: AtomicBool,
    pub blocks: Mutex<BlockTracker>,
}

#[derive(Default)]
pub struct PtyState { pub sessions: Arc<DashMap<String, Arc<PtySession>>> }

struct PtyPort(Arc<DashMap<String, Arc<PtySession>>>);
impl ports::TerminalPort for PtyPort {
    fn list(&self) -> Vec<(String, String)> { self.0.iter().map(|e| (e.key().clone(), e.value().info.read().title.clone())).collect() }
    fn read_buffer(&self, id: &str, lines: usize) -> Option<String> { self.0.get(id).map(|s| tail_lines(&s.buffer.lock().text(), lines)) }
}

pub fn init(app: &AppHandle) {
    let st = app.state::<PtyState>();
    ports::set_terminal(Arc::new(PtyPort(st.sessions.clone())));
}

fn now_ms() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) }

pub fn default_shell(settings: &SettingsState) -> String {
    settings.effective().general.default_shell.filter(|s| !s.is_empty())
        .or_else(|| std::env::var("SHELL").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| if cfg!(windows) { "powershell.exe".into() } else { "/bin/zsh".into() })
}

/// Split off an incomplete trailing UTF-8 sequence so chunk boundaries never corrupt characters.
fn split_utf8_tail(bytes: &[u8]) -> (usize, usize) {
    match std::str::from_utf8(bytes) {
        Ok(_) => (bytes.len(), 0),
        Err(e) => { let valid = e.valid_up_to(); match e.error_len() { None => (valid, bytes.len() - valid), Some(_) => (bytes.len(), 0) } }
    }
}

/// What the OSC scanner found, in stream order.
#[derive(Debug, Clone, PartialEq)]
pub enum OscEvent {
    Title(String),
    Cwd(String),
    /// OSC 133 ; A|B|C|D[;exit]
    Prompt(char, Option<i32>),
    /// Private `OSC 7770;cmd;<base64 command>` emitted by Caret's shell integration.
    Command(String),
    /// OSC 7501 program status: (state, app/title, decoded msg)
    Status { status: String, title: Option<String>, detail: Option<String> },
}
#[derive(Debug, Clone, PartialEq)]
pub enum Piece { Text(String), Event(OscEvent) }
#[derive(Debug, Default)]
pub struct Scan { pub pieces: Vec<Piece>, pub carry: String }

fn map_7501_state(state: &str) -> &'static str {
    match state { "working" => "working", "blocked" | "waiting" => "waiting", "done" => "done", "error" | "failed" => "failed", "idle" | "clear" => "idle", _ => "idle" }
}
fn b64_decode(s: &str) -> Option<String> {
    use base64::Engine as _;
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD.decode(cleaned.as_bytes()).ok().or_else(|| base64::engine::general_purpose::STANDARD_NO_PAD.decode(cleaned.as_bytes()).ok()).map(|b| String::from_utf8_lossy(&b).to_string())
}

/// Scan terminal output for OSC sequences we understand (0/2 title, 7 cwd, 133 prompts, 7501 status, 7770 command).
/// Text pieces keep every sequence xterm may want (incl. 133) but DROP the private 7501/7770 ones.
/// An unterminated trailing sequence is returned in `carry` (and withheld from the text) up to 4 KiB.
pub fn scan_osc(s: &str) -> Scan {
    let mut out = Scan::default();
    let mut text = String::new();
    let mut rest = s;
    while let Some(pos) = rest.find("\u{1b}]") {
        text.push_str(&rest[..pos]);
        let after = &rest[pos + 2..];
        let end = after.find('\u{7}').map(|i| (i, 1)).or_else(|| after.find("\u{1b}\\").map(|i| (i, 2)));
        let Some((end_i, term_len)) = end else {
            if rest.len() - pos <= 4096 { out.carry = rest[pos..].to_string(); rest = ""; } else { text.push_str(&rest[pos..]); rest = ""; }
            break;
        };
        let body = &after[..end_i];
        let raw_seq = &rest[pos..pos + 2 + end_i + term_len];
        let (code, payload) = body.split_once(';').map(|(c, p)| (c, p)).unwrap_or((body, ""));
        let mut keep = true;
        let mut ev: Option<OscEvent> = None;
        match code {
            "0" | "2" => ev = Some(OscEvent::Title(payload.to_string())),
            "7" => { let p = payload.strip_prefix("file://").map(|u| u.find('/').map(|i| &u[i..]).unwrap_or(u)).unwrap_or(payload); ev = Some(OscEvent::Cwd(percent_decode(p))); }
            "133" => {
                let mut it = payload.splitn(2, ';');
                let kind = it.next().unwrap_or("").chars().next().unwrap_or(' ');
                let exit = it.next().and_then(|e| e.split(';').next()).and_then(|e| e.trim().parse::<i32>().ok());
                if matches!(kind, 'A' | 'B' | 'C' | 'D') { ev = Some(OscEvent::Prompt(kind, exit)); }
            }
            "7501" => {
                keep = false;
                let mut state = None; let mut title = None; let mut detail = None;
                for kv in payload.split(':') {
                    if let Some((k, v)) = kv.split_once('=') {
                        match k.trim() { "state" => state = Some(v.trim().to_string()), "app" => title = Some(v.trim().to_string()), "msg" => detail = b64_decode(v.trim()).or_else(|| Some(v.trim().to_string())), _ => {} }
                    }
                }
                if let Some(st) = state { ev = Some(OscEvent::Status { status: map_7501_state(&st).to_string(), title, detail }); }
            }
            "7770" => {
                keep = false;
                if let Some(b64) = payload.strip_prefix("cmd;") { if let Some(cmd) = b64_decode(b64) { ev = Some(OscEvent::Command(cmd.trim_end_matches(['\n', '\r']).to_string())); } }
            }
            _ => {}
        }
        if keep { text.push_str(raw_seq); }
        if let Some(e) = ev {
            if !text.is_empty() { out.pieces.push(Piece::Text(std::mem::take(&mut text))); }
            out.pieces.push(Piece::Event(e));
        }
        rest = &after[end_i + term_len..];
    }
    text.push_str(rest);
    if !text.is_empty() { out.pieces.push(Piece::Text(text)); }
    out
}

/// Compatibility helper: (title, cwd, unterminated tail).
pub fn parse_osc(s: &str) -> (Option<String>, Option<String>, String) {
    let scan = scan_osc(s);
    let mut title = None; let mut cwd = None;
    for p in scan.pieces { if let Piece::Event(e) = p { match e { OscEvent::Title(t) => title = Some(t), OscEvent::Cwd(c) => cwd = Some(c), _ => {} } } }
    (title, cwd, scan.carry)
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes(); let mut out = Vec::with_capacity(b.len()); let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 + 0 { if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) { out.push(v); i += 3; continue; } }
        out.push(b[i]); i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Spawn a shell in a new PTY and wire reader/flusher/waiter threads. Blocking; call from spawn_blocking.
pub fn spawn_session(sessions: Arc<DashMap<String, Arc<PtySession>>>, shell: String, args: Vec<String>, cwd: std::path::PathBuf, env_extra: BTreeMap<String, String>, cols: u16, rows: u16, title: String, on_event: Channel<PtyEvent>) -> Result<Arc<PtySession>> {
    spawn_session_with(sessions, shell, args, cwd, env_extra, cols, rows, title, on_event, BlockTracker::new(200, true, true))
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_session_with(sessions: Arc<DashMap<String, Arc<PtySession>>>, shell: String, args: Vec<String>, cwd: std::path::PathBuf, env_extra: BTreeMap<String, String>, cols: u16, rows: u16, title: String, on_event: Channel<PtyEvent>, mut tracker: BlockTracker) -> Result<Arc<PtySession>> {
    let id = uuid::Uuid::new_v4().to_string();
    let pty = native_pty_system();
    let pair = pty.openpty(PtySize { rows: rows.max(1), cols: cols.max(2), pixel_width: 0, pixel_height: 0 }).map_err(|e| CaretError::io(format!("openpty failed: {e}")))?;
    let mut cmd = CommandBuilder::new(&shell);
    cmd.args(&args);
    cmd.cwd(&cwd);
    cmd.env("TERM", "xterm-256color"); cmd.env("COLORTERM", "truecolor"); cmd.env("TERM_PROGRAM", "Caret"); cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
    if std::env::var("LANG").map(|l| l.is_empty()).unwrap_or(true) { cmd.env("LANG", "en_US.UTF-8"); }
    for (k, v) in &env_extra { cmd.env(k, v); }
    let mut child = pair.slave.spawn_command(cmd).map_err(|e| CaretError::io(format!("failed to spawn {shell}: {e}")))?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| CaretError::io(e.to_string()))?;
    let writer = pair.master.take_writer().map_err(|e| CaretError::io(e.to_string()))?;
    let killer = child.clone_killer();
    let pid = child.process_id().unwrap_or(0);
    let info = PtyInfo { id: id.clone(), pid, title, cwd: cwd.to_string_lossy().to_string(), shell: shell.clone(), created_at: now_ms() };
    tracker.set_cwd(&cwd.to_string_lossy());
    let session = Arc::new(PtySession { info: RwLock::new(info), writer: Mutex::new(writer), master: Mutex::new(pair.master), killer: Mutex::new(killer), buffer: Mutex::new(RingBuf::new(SCROLLBACK_BYTES)), channel: on_event, alive: AtomicBool::new(true), blocks: Mutex::new(tracker) });
    sessions.insert(id.clone(), session.clone());

    // reader → chunks
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::Builder::new().name(format!("pty-read-{}", &id[..8])).spawn(move || {
        let mut buf = [0u8; 8192];
        loop { match reader.read(&mut buf) { Ok(0) | Err(_) => break, Ok(n) => { if tx.send(buf[..n].to_vec()).is_err() { break; } } } }
    }).map_err(|e| CaretError::io(e.to_string()))?;
    // flusher: coalesce bursts, fix utf-8 boundaries, track OSC, push to channel + scrollback
    let s2 = session.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    std::thread::Builder::new().name(format!("pty-flush-{}", &id[..8])).spawn(move || {
        let mut carry: Vec<u8> = Vec::new();
        let mut osc_carry = String::new();
        while let Ok(first) = rx.recv() {
            let start = Instant::now();
            let mut pending = std::mem::take(&mut carry);
            pending.extend_from_slice(&first);
            while pending.len() < FLUSH_BYTES {
                let elapsed = start.elapsed(); if elapsed >= COALESCE { break; }
                match rx.recv_timeout(COALESCE - elapsed) { Ok(more) => pending.extend_from_slice(&more), Err(_) => break }
            }
            let (valid, incomplete) = split_utf8_tail(&pending);
            if incomplete > 0 { carry = pending[valid..].to_vec(); }
            let text = String::from_utf8_lossy(&pending[..valid]).to_string();
            if text.is_empty() { continue; }
            let scan = scan_osc(&(std::mem::take(&mut osc_carry) + &text));
            osc_carry = scan.carry;
            let mut out = String::new();
            let mut title: Option<String> = None;
            let flush_out = |out: &mut String, s2: &PtySession| { if !out.is_empty() { s2.buffer.lock().push(out.as_bytes()); let _ = s2.channel.send(PtyEvent::Data { data: std::mem::take(out) }); } };
            for piece in scan.pieces {
                match piece {
                    Piece::Text(t) => { s2.blocks.lock().on_text(&t); out.push_str(&t); }
                    Piece::Event(OscEvent::Title(t)) => { if !t.is_empty() { title = Some(t); } }
                    Piece::Event(OscEvent::Cwd(c)) => { s2.blocks.lock().set_cwd(&c); let changed = { let mut i = s2.info.write(); if i.cwd != c { i.cwd = c.clone(); true } else { false } }; if changed { let _ = s2.channel.send(PtyEvent::Cwd { cwd: c }); } }
                    Piece::Event(OscEvent::Command(cmd)) => { let act = s2.blocks.lock().on_command(cmd); if let Some(BlockAction::Command(id, c)) = act { let _ = s2.channel.send(PtyEvent::BlockCommand { block_id: id, command: c }); } }
                    Piece::Event(OscEvent::Prompt(kind, exit)) => {
                        let actions = s2.blocks.lock().on_prompt(kind, exit, now_ms());
                        if !actions.is_empty() { flush_out(&mut out, &s2); }
                        for a in actions { match a {
                            BlockAction::Start(b) => { let _ = s2.channel.send(PtyEvent::BlockStart { block: b }); }
                            BlockAction::End { block_id, exit_code, ended_at, end_line } => { let _ = s2.channel.send(PtyEvent::BlockEnd { block_id, exit_code, ended_at, end_line }); }
                            BlockAction::Command(id, c) => { let _ = s2.channel.send(PtyEvent::BlockCommand { block_id: id, command: c }); }
                            BlockAction::Status(st) => { let _ = s2.channel.send(PtyEvent::Status { status: st.status, title: st.title, detail: st.detail }); }
                        } }
                    }
                    Piece::Event(OscEvent::Status { status, title: t, detail }) => { let act = s2.blocks.lock().on_status(status, t, detail); if let Some(BlockAction::Status(st)) = act { let _ = s2.channel.send(PtyEvent::Status { status: st.status, title: st.title, detail: st.detail }); } }
                }
            }
            flush_out(&mut out, &s2);
            if let Some(t) = title { s2.info.write().title = t.clone(); let _ = s2.channel.send(PtyEvent::Title { title: t }); }
        }
        let _ = done_tx.send(());
    }).map_err(|e| CaretError::io(e.to_string()))?;
    // waiter: reap child, then emit Exit (after output drains, bounded)
    let s3 = session.clone(); let sessions3 = sessions.clone(); let id3 = id.clone();
    std::thread::Builder::new().name(format!("pty-wait-{}", &id[..8])).spawn(move || {
        let code = child.wait().ok().map(|s| s.exit_code() as i32);
        let _ = done_rx.recv_timeout(Duration::from_millis(800));
        s3.alive.store(false, Ordering::Release);
        let _ = s3.channel.send(PtyEvent::Exit { code });
        sessions3.remove(&id3);
    }).map_err(|e| CaretError::io(e.to_string()))?;
    Ok(session)
}

#[tauri::command]
pub async fn pty_spawn(app: AppHandle, st: State<'_, PtyState>, options: PtySpawnOptions, on_event: Channel<PtyEvent>) -> Result<PtyInfo> {
    let settings = app.state::<SettingsState>();
    let shell = options.shell.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| default_shell(&settings));
    let root = app.state::<WorkspaceState>().root();
    let cwd = options.cwd.clone().filter(|c| !c.is_empty()).map(std::path::PathBuf::from).or(root).unwrap_or_else(crate::paths::home_dir);
    let cwd = if cwd.is_dir() { cwd } else { crate::paths::home_dir() };
    let shell_name = std::path::Path::new(&shell).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| shell.clone());
    let user_args = options.args.clone().filter(|a| !a.is_empty());
    let login = user_args.is_none() && cfg!(unix) && matches!(shell_name.as_str(), "zsh" | "bash" | "fish" | "sh");
    let mut args = user_args.clone().unwrap_or_else(|| if login { vec!["-l".into()] } else { vec![] });
    let title = options.name.clone().unwrap_or_else(|| shell_name.clone());
    let mut env_extra = options.env.clone().unwrap_or_default();
    let term_settings = settings.effective().terminal;
    if term_settings.shell_integration && user_args.is_none() {
        match crate::core::shell_integration::ensure_installed() {
            Ok(root) => if let Some(integ) = crate::core::shell_integration::for_shell(&shell_name, login, &root, &crate::paths::home_dir()) {
                for (k, v) in integ.env { env_extra.entry(k).or_insert(v); }
                if let Some(a) = integ.args { args = a; }
            },
            Err(e) => tracing::warn!(error = %e, "shell integration install failed; continuing without it"),
        }
    }
    let tracker = BlockTracker::new(term_settings.max_blocks as usize, term_settings.blocks, term_settings.program_status);
    let (cols, rows) = (options.cols, options.rows);
    let sessions = st.sessions.clone();
    let shell2 = shell.clone();
    let session = tokio::task::spawn_blocking(move || spawn_session_with(sessions, shell2, args, cwd, env_extra, cols, rows, title, on_event, tracker)).await.map_err(|e| CaretError::internal(e.to_string()))??;
    let info = session.info.read().clone();
    tracing::info!(id = %info.id, pid = info.pid, shell = %shell, "pty spawned");
    Ok(info)
}

fn get(st: &PtyState, id: &str) -> Result<Arc<PtySession>> { st.sessions.get(id).map(|s| s.clone()).ok_or_else(|| CaretError::not_found(format!("terminal {id}"))) }

#[tauri::command]
pub fn pty_write(st: State<'_, PtyState>, id: String, data: String) -> Result<()> {
    let s = get(&st, &id)?;
    let mut w = s.writer.lock();
    let r = w.write_all(data.as_bytes()).and_then(|_| w.flush()).map_err(|e| CaretError::io(e.to_string()));
    drop(w);
    r
}
#[tauri::command]
pub fn pty_resize(st: State<'_, PtyState>, id: String, cols: u16, rows: u16) -> Result<()> {
    let s = get(&st, &id)?;
    let r = s.master.lock().resize(PtySize { rows: rows.max(1), cols: cols.max(2), pixel_width: 0, pixel_height: 0 }).map_err(|e| CaretError::io(e.to_string()));
    r
}
#[tauri::command]
pub fn pty_kill(st: State<'_, PtyState>, id: String) -> Result<()> {
    if let Some(s) = st.sessions.get(&id).map(|s| s.clone()) { let _ = s.killer.lock().kill(); }
    Ok(())
}
#[tauri::command]
pub fn pty_list(st: State<'_, PtyState>) -> Result<Vec<PtyInfo>> {
    let mut v: Vec<PtyInfo> = st.sessions.iter().filter(|e| e.value().alive.load(Ordering::Acquire)).map(|e| e.value().info.read().clone()).collect();
    v.sort_by_key(|i| i.created_at);
    Ok(v)
}
#[tauri::command]
pub fn pty_read_buffer(st: State<'_, PtyState>, id: String, lines: Option<usize>) -> Result<String> {
    let s = get(&st, &id)?;
    let text = s.buffer.lock().text();
    Ok(tail_lines(&text, lines.unwrap_or(200).min(5000)))
}

#[tauri::command]
pub fn pty_blocks(st: State<'_, PtyState>, id: String) -> Result<Vec<TerminalBlock>> { let s = get(&st, &id)?; let v = s.blocks.lock().blocks(); Ok(v) }

#[tauri::command]
pub fn pty_block_output(st: State<'_, PtyState>, id: String, block_id: String, max_chars: Option<usize>) -> Result<String> {
    let s = get(&st, &id)?;
    let (text, truncated) = s.blocks.lock().output(&block_id).ok_or_else(|| CaretError::not_found(format!("block {block_id}")))?;
    let cap = max_chars.unwrap_or(20_000).clamp(200, 200_000);
    let mut out: String = if text.chars().count() > cap { let head: String = text.chars().take(cap * 2 / 3).collect(); let tail: String = text.chars().rev().take(cap / 3).collect::<Vec<_>>().into_iter().rev().collect(); format!("{head}\n…[{} chars omitted]…\n{tail}", text.chars().count() - cap) } else { text };
    if truncated { out.push_str("\n…[output capture truncated]"); }
    Ok(out)
}

#[tauri::command]
pub fn pty_status(st: State<'_, PtyState>, id: String) -> Result<Option<ProgramStatusInfo>> { let s = get(&st, &id)?; let v = s.blocks.lock().status.clone(); Ok(v) }

/// Plain-text output of the most recent block (for agent tools / @Terminal).
pub fn last_block_output(st: &PtyState, id: &str) -> Option<String> { st.sessions.get(id).and_then(|s| s.blocks.lock().last_output()) }

/// Non-interactive helper for UI features (e.g. running a formatter, `which`, version checks).
#[tauri::command]
pub async fn pty_run_command(app: AppHandle, command: String, cwd: Option<String>, timeout_ms: Option<u64>) -> Result<RunCommandResult> {
    let settings = app.state::<SettingsState>();
    let shell = default_shell(&settings);
    let root = app.state::<WorkspaceState>().root();
    let dir = cwd.map(std::path::PathBuf::from).or(root).filter(|d| d.is_dir()).unwrap_or_else(crate::paths::home_dir);
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(60_000).clamp(100, 30 * 60_000));
    let fut = tokio::process::Command::new(&shell).arg("-lc").arg(&command).current_dir(&dir).env("TERM", "dumb").env("CARET", "1")
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true).output();
    const CAP: usize = 1024 * 1024;
    let clip = |mut v: Vec<u8>| { let t = v.len() > CAP; if t { v.truncate(CAP); } let mut s = String::from_utf8_lossy(&v).to_string(); if t { s.push_str("\n…[truncated]"); } s };
    match tokio::time::timeout(timeout, fut).await {
        Ok(Ok(out)) => Ok(RunCommandResult { stdout: clip(out.stdout), stderr: clip(out.stderr), code: out.status.code(), timed_out: false }),
        Ok(Err(e)) => Err(CaretError::io(format!("failed to run command: {e}"))),
        Err(_) => Ok(RunCommandResult { stdout: String::new(), stderr: format!("timed out after {} ms", timeout.as_millis()), code: None, timed_out: true }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ring_buffer_bounds() {
        let mut r = RingBuf::new(10);
        r.push(b"hello"); r.push(b"world!!"); assert_eq!(r.text(), "lloworld!!"); assert!(r.buf.len() <= 10);
        r.push(b"0123456789abcdef"); assert_eq!(r.text(), "6789abcdef");
    }
    #[test]
    fn utf8_tail_split() {
        let s = "héllo".as_bytes(); let cut = &s[..2]; // 'h' + first byte of 'é'
        assert_eq!(split_utf8_tail(cut), (1, 1));
        assert_eq!(split_utf8_tail(s), (s.len(), 0));
        assert_eq!(split_utf8_tail(&[0xff, b'a']), (2, 0));
    }
    #[test]
    fn osc_parse_title_cwd_and_tail() {
        let (t, c, tail) = parse_osc("abc\u{1b}]0;my title\u{7}xyz\u{1b}]7;file://host/Users/me/pro%20ject\u{1b}\\rest\u{1b}]2;partial");
        assert_eq!(t.as_deref(), Some("my title")); assert_eq!(c.as_deref(), Some("/Users/me/pro ject")); assert!(tail.starts_with("\u{1b}]2;partial"));
    }
    #[test]
    fn spawns_real_shell_and_reports_exit() {
        if !std::path::Path::new("/bin/sh").exists() { return; }
        let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
        let ev2 = events.clone();
        let ch: Channel<PtyEvent> = Channel::new(move |body| { if let tauri::ipc::InvokeResponseBody::Json(s) = body { ev2.lock().push(s); } Ok(()) });
        let sessions: Arc<DashMap<String, Arc<PtySession>>> = Arc::new(DashMap::new());
        let s = spawn_session(sessions.clone(), "/bin/sh".into(), vec!["-c".into(), "printf 'hello caret\\n'; printf '\\033]0;ttl\\007'; exit 3".into()], std::env::temp_dir(), BTreeMap::new(), 80, 24, "sh".into(), ch).unwrap();
        let id = s.info.read().id.clone();
        assert!(sessions.contains_key(&id));
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline { if events.lock().iter().any(|e| e.contains("\"exit\"")) { break; } std::thread::sleep(Duration::from_millis(20)); }
        let evs = events.lock().clone();
        let all = evs.join("\n");
        assert!(all.contains("hello caret"), "data events: {all}");
        assert!(all.contains("\"type\":\"exit\"") && all.contains("\"code\":3"), "exit event: {all}");
        assert!(all.contains("\"type\":\"title\"") && all.contains("ttl"), "title event: {all}");
        assert!(tail_lines(&s.buffer.lock().text(), 5).contains("hello caret"));
        assert!(!sessions.contains_key(&id), "session removed after exit");
    }
    #[test]
    fn ansi_strip_and_tail() {
        let s = "\u{1b}[32mok\u{1b}[0m\r\nline2\u{1b}]0;t\u{7}\nline3\n";
        assert_eq!(strip_ansi(s), "ok\nline2\nline3\n");
        assert_eq!(tail_lines(s, 2), "line2\nline3");
    }

    fn b64(s: &str) -> String { use base64::Engine as _; base64::engine::general_purpose::STANDARD.encode(s.as_bytes()) }
    #[test]
    fn scanner_prompt_markers_command_and_status_are_ordered_and_private_oscs_stripped() {
        let input = format!("$ \u{1b}]7770;cmd;{}\u{7}\u{1b}]133;B\u{7}\u{1b}]133;C\u{7}hello\r\n\u{1b}]133;D;0\u{7}\u{1b}]7501;state=working:app=cargo:msg={}\u{7}prompt> \u{1b}]133;A\u{7}", b64("echo hello"), b64("building 3/10"));
        let scan = scan_osc(&input);
        assert!(scan.carry.is_empty());
        let events: Vec<&OscEvent> = scan.pieces.iter().filter_map(|p| if let Piece::Event(e) = p { Some(e) } else { None }).collect();
        assert_eq!(events[0], &OscEvent::Command("echo hello".into()));
        assert_eq!(events[1], &OscEvent::Prompt('B', None));
        assert_eq!(events[2], &OscEvent::Prompt('C', None));
        assert_eq!(events[3], &OscEvent::Prompt('D', Some(0)));
        assert_eq!(events[4], &OscEvent::Status { status: "working".into(), title: Some("cargo".into()), detail: Some("building 3/10".into()) });
        assert_eq!(events[5], &OscEvent::Prompt('A', None));
        let text: String = scan.pieces.iter().filter_map(|p| if let Piece::Text(t) = p { Some(t.as_str()) } else { None }).collect();
        assert!(!text.contains("7770") && !text.contains("7501"), "private OSCs must not reach xterm: {text:?}");
        assert!(text.contains("\u{1b}]133;B\u{7}"), "133 markers are kept for xterm: {text:?}");
        assert!(text.contains("hello\r\n") && text.contains("prompt> "));
    }
    #[test]
    fn scanner_carries_unterminated_private_sequence() {
        let scan = scan_osc("out\u{1b}]7501;state=wor");
        assert_eq!(scan.pieces, vec![Piece::Text("out".into())]);
        assert_eq!(scan.carry, "\u{1b}]7501;state=wor");
        let scan2 = scan_osc(&(scan.carry + "king\u{1b}\\more"));
        assert!(matches!(scan2.pieces[0], Piece::Event(OscEvent::Status { ref status, .. }) if status == "working"));
        assert_eq!(scan2.pieces[1], Piece::Text("more".into()));
    }
    #[test]
    fn scanner_maps_7501_states_and_ignores_unknown_osc() {
        for (raw, want) in [("blocked", "waiting"), ("error", "failed"), ("done", "done"), ("clear", "idle"), ("bogus", "idle")] {
            let scan = scan_osc(&format!("\u{1b}]7501;state={raw}\u{1b}\\"));
            assert!(matches!(&scan.pieces[0], Piece::Event(OscEvent::Status { status, .. }) if status == want), "{raw} -> {want}");
        }
        let scan = scan_osc("a\u{1b}]52;c;Zm9v\u{7}b");
        assert_eq!(scan.pieces, vec![Piece::Text("a\u{1b}]52;c;Zm9v\u{7}b".into())]);
    }
    #[test]
    fn block_tracker_lifecycle_and_caps() {
        let mut t = BlockTracker::new(2, true, true);
        t.set_cwd("/w");
        t.on_text("prompt$ "); // no block yet
        assert!(t.on_command("ls -la".into()).is_none(), "command before B is pended");
        let acts = t.on_prompt('B', None, 100);
        assert!(matches!(&acts[0], BlockAction::Start(b) if b.command.as_deref() == Some("ls -la") && b.cwd.as_deref() == Some("/w") && b.start_line == 0 && b.running));
        assert!(t.on_prompt('C', None, 101).is_empty(), "C after B does not start a second block");
        t.on_text("file1\nfile2\n");
        let acts = t.on_prompt('D', Some(0), 200);
        assert!(matches!(&acts[0], BlockAction::End { exit_code: Some(0), end_line: 2, ended_at: 200, .. }));
        let blocks = t.blocks();
        assert_eq!(blocks.len(), 1); assert_eq!(blocks[0].exit_code, Some(0)); assert!(!blocks[0].running); assert_eq!(blocks[0].output_preview.as_deref(), Some("file1\nfile2\n"));
        let id0 = blocks[0].id.clone();
        assert_eq!(t.output(&id0).unwrap().0, "file1\nfile2\n");
        // command arriving after start (bash DEBUG-trap ordering) attaches to running block
        t.on_prompt('A', None, 250); // prompt start, nothing running → no actions
        t.on_prompt('B', None, 300);
        assert!(matches!(t.on_command("make".into()), Some(BlockAction::Command(_, c)) if c == "make"));
        assert!(t.on_command("dup".into()).is_none(), "second command for same block ignored");
        t.on_text("x\n"); t.on_prompt('D', Some(2), 400);
        t.on_prompt('B', None, 500); t.on_prompt('D', None, 600); // third block → oldest evicted (max 2)
        assert_eq!(t.len(), 2);
        assert!(t.output(&id0).is_none(), "evicted block gone");
        assert_eq!(t.blocks()[0].command.as_deref(), Some("make"));
        assert_eq!(t.blocks()[0].exit_code, Some(2));
        assert_eq!(t.last_output().as_deref(), Some(""));
        // status
        assert!(matches!(t.on_status("waiting".into(), Some("claude".into()), None), Some(BlockAction::Status(ref s)) if s.status == "waiting"));
        assert_eq!(t.status.as_ref().unwrap().title.as_deref(), Some("claude"));
    }
    #[test]
    fn block_tracker_per_block_cap_and_disabled_modes() {
        let mut t = BlockTracker::new(10, true, true);
        t.on_prompt('B', None, 1);
        let chunk = "y".repeat(BLOCK_OUTPUT_CAP + 10);
        t.on_text(&chunk);
        let (out, truncated) = t.output(&t.blocks()[0].id).unwrap();
        assert_eq!(out.len(), BLOCK_OUTPUT_CAP); assert!(truncated);
        let mut off = BlockTracker::new(10, false, false);
        assert!(off.on_prompt('B', None, 1).is_empty()); assert!(off.on_status("working".into(), None, None).is_none()); assert!(off.is_empty());
        off.on_text("a\nb\n"); assert_eq!(off.line_count, 2, "line counting still works when blocks are off");
    }
    #[test]
    fn block_tracker_dedupes_double_markers() {
        let mut t = BlockTracker::new(10, true, true);
        assert_eq!(t.on_prompt('B', None, 1).len(), 1);
        assert_eq!(t.on_prompt('B', None, 1).len(), 0, "same marker at same line is ignored");
        t.on_text("o\n");
        assert_eq!(t.on_prompt('D', Some(0), 2).len(), 1);
        assert_eq!(t.on_prompt('D', Some(0), 2).len(), 0, "no running block → no second end");
    }
    #[test]
    fn sh_session_emits_block_events_from_osc_133() {
        if !std::path::Path::new("/bin/sh").exists() { return; }
        let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
        let ev2 = events.clone();
        let ch: Channel<PtyEvent> = Channel::new(move |body| { if let tauri::ipc::InvokeResponseBody::Json(s) = body { ev2.lock().push(s); } Ok(()) });
        let sessions: Arc<DashMap<String, Arc<PtySession>>> = Arc::new(DashMap::new());
        let script = format!("printf '\\033]7770;cmd;{}\\007\\033]133;B\\007\\033]133;C\\007'; printf 'out1\\n'; printf '\\033]133;D;7\\007\\033]7501;state=done:app=sh\\007'; exit 0", b64("true"));
        let s = spawn_session(sessions.clone(), "/bin/sh".into(), vec!["-c".into(), script], std::env::temp_dir(), BTreeMap::new(), 80, 24, "sh".into(), ch).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline { if events.lock().iter().any(|e| e.contains("\"exit\"")) { break; } std::thread::sleep(Duration::from_millis(20)); }
        let all = events.lock().join("\n");
        assert!(all.contains("\"type\":\"block-start\"") && all.contains("\"command\":\"true\""), "{all}");
        assert!(all.contains("\"type\":\"block-end\"") && all.contains("\"exitCode\":7"), "{all}");
        assert!(all.contains("\"type\":\"status\"") && all.contains("\"status\":\"done\""), "{all}");
        assert!(!all.contains("7770") && !all.contains("7501"), "private OSCs stripped from data: {all}");
        let blocks = s.blocks.lock().blocks();
        assert_eq!(blocks.len(), 1); assert_eq!(blocks[0].exit_code, Some(7));
        assert_eq!(s.blocks.lock().output(&blocks[0].id).unwrap().0.trim(), "out1");
    }
}

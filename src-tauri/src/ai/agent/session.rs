//! Session model + every serde type shared with the UI (mirrors src/types.ts chat/agent section).
use crate::ai::llm::{self, ContentBlock, Message as LlmMessage};
use crate::tokens;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub fn now_ms() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) }
pub fn new_id() -> String { uuid::Uuid::new_v4().to_string() }
pub fn short_id() -> String { uuid::Uuid::new_v4().simple().to_string()[..8].to_string() }

// ───────────────────────── context / attachments ─────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Range1 { pub start_line: u32, pub end_line: u32 }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ContextItem {
    pub id: String,
    pub kind: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub range: Option<Range1>,
    #[serde(skip_serializing_if = "Option::is_none")] pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub data: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub mime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub terminal_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub rule_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub server_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub base: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub doc_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ImageAttachment {
    pub id: String,
    pub name: String,
    pub mime: String,
    pub data_base64: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")] pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CursorSelection { pub path: String, pub range: Range1, pub text: String }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SendMessageRequest {
    pub session_id: String,
    pub text: String,
    pub context: Vec<ContextItem>,
    pub images: Vec<ImageAttachment>,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub use_worktree: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub queue: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub active_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub open_files: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")] pub cursor_selection: Option<CursorSelection>,
    /// 'caret' (built-in) or an ACP agent id; None = session default.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub review_comments: Option<Vec<ReviewComment>>,
}

// ───────────────────────── review comments (review-on-diff) ─────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewComment {
    pub id: String,
    pub path: String,
    pub line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub end_line: Option<u32>,
    pub side: String, // before | after
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub change_id: Option<String>,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub resolved: Option<bool>,
    pub author: String, // user | bot
    #[serde(default, skip_serializing_if = "Option::is_none")] pub finding_id: Option<String>,
}

// ───────────────────────── tool calls / changes / approvals ─────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolStatus { Pending, AwaitingApproval, Running, Done, Error, Rejected, Cancelled }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalOption { Allow, AllowSession, AllowAlways, Deny, DenyAlways }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalKind { Command, Mcp, FileDelete, FileEdit, OutsideWorkspace, Web, Subagent, Browser }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub approval_id: String,
    pub session_id: String,
    pub tool_call_id: String,
    pub kind: ApprovalKind,
    pub title: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub cwd: Option<String>,
    pub options: Vec<ApprovalOption>,
    #[serde(skip_serializing_if = "Option::is_none")] pub allow_always_pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub classification: Option<crate::sandbox::CommandClassification>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub risk_tier: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub sandboxed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentQuestion {
    pub question_id: String,
    pub question: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub options: Option<Vec<String>>,
    pub allow_free_text: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind { Create, Modify, Delete, Rename }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeStatus { Pending, Accepted, Rejected }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChangeSummary {
    pub change_id: String,
    pub path: String,
    pub kind: ChangeKind,
    pub additions: u32,
    pub deletions: u32,
    pub status: ChangeStatus,
    #[serde(skip_serializing_if = "Option::is_none")] pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub old_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingChange {
    pub change_id: String,
    pub session_id: String,
    pub path: String,
    pub kind: ChangeKind,
    pub before: Option<String>,
    pub after: Option<String>,
    pub diff: String,
    pub status: ChangeStatus,
    #[serde(skip_serializing_if = "Option::is_none")] pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub tool_call_id: Option<String>,
    #[serde(default)] pub additions: u32,
    #[serde(default)] pub deletions: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub old_path: Option<String>,
}
impl PendingChange {
    pub fn summary(&self) -> FileChangeSummary {
        FileChangeSummary { change_id: self.change_id.clone(), path: self.path.clone(), kind: self.kind, additions: self.additions, deletions: self.deletions, status: self.status, tool_call_id: self.tool_call_id.clone(), turn_id: self.turn_id.clone(), old_path: self.old_path.clone() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub id: String,
    pub session_id: String,
    pub message_id: String,
    pub created_at: u64,
    pub files: Vec<String>,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub input: Value,
    #[serde(skip_serializing_if = "Option::is_none")] pub input_text: Option<String>,
    pub status: ToolStatus,
    #[serde(skip_serializing_if = "Option::is_none")] pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub output_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub started_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub finished_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub file_changes: Option<Vec<FileChangeSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")] pub mcp_server_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")] pub is_background: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub diff: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub approval: Option<ApprovalRequest>,
    #[serde(skip_serializing_if = "Option::is_none")] pub subagent_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub sandboxed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub classification: Option<crate::sandbox::CommandClassification>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub locations: Option<Vec<ToolLocation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub raw_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolLocation { pub path: String, #[serde(default, skip_serializing_if = "Option::is_none")] pub line: Option<u32> }
impl ToolCallRecord {
    pub fn new(id: &str, name: &str, display_name: &str) -> Self {
        Self { id: id.into(), name: name.into(), display_name: display_name.into(), input: Value::Null, input_text: None, status: ToolStatus::Pending, output: None, output_truncated: None, error: None, started_at: Some(now_ms()), finished_at: None, file_changes: None, mcp_server_id: None, command: None, cwd: None, exit_code: None, is_background: None, diff: None, approval: None, subagent_session_id: None, summary: None, sandboxed: None, classification: None, locations: None, raw_kind: None }
    }
}

// ───────────────────────── messages ─────────────────────────
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole { User, Assistant, System }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum MessagePart {
    Text { text: String },
    Thinking { text: String, #[serde(skip_serializing_if = "Option::is_none")] duration_ms: Option<u64>, #[serde(skip_serializing_if = "Option::is_none")] redacted: Option<bool> },
    Tool { call: ToolCallRecord },
    Image { image: ImageAttachment },
    Context { items: Vec<ContextItem> },
    Error { message: String },
    Plan { path: String, title: String },
    Question { question: AgentQuestion, #[serde(skip_serializing_if = "Option::is_none")] answer: Option<String> },
    Summary { text: String },
    Checkpoint { checkpoint_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus { Streaming, Done, Error, Cancelled }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")] pub thinking_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub requests: Option<u32>,
}
impl Usage {
    pub fn from_delta(d: &llm::UsageDelta, cost: f64) -> Self {
        Self { input_tokens: d.input_tokens, output_tokens: d.output_tokens, cache_read_tokens: d.cache_read_tokens, cache_write_tokens: d.cache_write_tokens, thinking_tokens: if d.thinking_tokens > 0 { Some(d.thinking_tokens) } else { None }, cost_usd: Some(cost), requests: Some(1) }
    }
    pub fn add(&mut self, o: &Usage) {
        self.input_tokens += o.input_tokens; self.output_tokens += o.output_tokens; self.cache_read_tokens += o.cache_read_tokens; self.cache_write_tokens += o.cache_write_tokens;
        if o.thinking_tokens.is_some() { self.thinking_tokens = Some(self.thinking_tokens.unwrap_or(0) + o.thinking_tokens.unwrap_or(0)); }
        if o.cost_usd.is_some() { self.cost_usd = Some(self.cost_usd.unwrap_or(0.0) + o.cost_usd.unwrap_or(0.0)); }
        self.requests = Some(self.requests.unwrap_or(0) + o.requests.unwrap_or(1));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: String,
    pub role: MessageRole,
    pub parts: Vec<MessagePart>,
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")] pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub usage: Option<Usage>,
    #[serde(skip_serializing_if = "Option::is_none")] pub status: Option<MessageStatus>,
    #[serde(skip_serializing_if = "Option::is_none")] pub checkpoint_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub duration_ms: Option<u64>,
    /// Auto-routing decision for this assistant message (served model, tier).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub routing: Option<llm::RoutingInfo>,
}
impl ChatMessage {
    pub fn new(role: MessageRole) -> Self { Self { id: new_id(), role, parts: vec![], created_at: now_ms(), model_id: None, mode: None, usage: None, status: None, checkpoint_id: None, turn_id: None, duration_ms: None, routing: None } }
    pub fn text(&self) -> String { self.parts.iter().filter_map(|p| if let MessagePart::Text { text } = p { Some(text.as_str()) } else { None }).collect::<Vec<_>>().join("\n") }
    pub fn preview(&self, n: usize) -> String { let t = self.text(); let t = t.trim(); let s: String = t.chars().take(n).collect(); if t.chars().count() > n { format!("{s}…") } else { s } }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem { pub id: String, pub content: String, pub status: String }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct QueuedMessage { pub id: String, pub text: String, pub context: Vec<ContextItem>, pub images: Vec<ImageAttachment>, #[serde(skip_serializing_if = "Option::is_none")] pub mode: Option<String>, #[serde(skip)] pub request: Option<SendMessageRequest> }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionStatus { Idle, Running, AwaitingApproval, AwaitingInput, Error, Summarizing }

// ───────────────────────── events ─────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum AgentEvent {
    SessionStatus { session_id: String, status: SessionStatus },
    MessageStart { session_id: String, message: ChatMessage },
    TextDelta { session_id: String, message_id: String, text: String },
    ThinkingDelta { session_id: String, message_id: String, text: String },
    ToolStart { session_id: String, message_id: String, call: ToolCallRecord },
    ToolInputDelta { session_id: String, message_id: String, tool_call_id: String, text: String },
    ToolUpdate { session_id: String, message_id: String, call: ToolCallRecord },
    ApprovalRequest { session_id: String, request: ApprovalRequest },
    Question { session_id: String, message_id: String, question: AgentQuestion },
    FileChange { session_id: String, change: FileChangeSummary, #[serde(skip_serializing_if = "Option::is_none")] diff: Option<String> },
    FileChangeStatus { session_id: String, change_ids: Vec<String>, status: ChangeStatus },
    Checkpoint { session_id: String, checkpoint_id: String, message_id: String },
    Todos { session_id: String, todos: Vec<TodoItem> },
    Usage { session_id: String, message_id: String, usage: Usage, context_tokens: u64, session_usage: Usage },
    MessageEnd { session_id: String, message_id: String, status: MessageStatus, #[serde(skip_serializing_if = "Option::is_none")] error: Option<String>, duration_ms: u64 },
    Title { session_id: String, title: String },
    Queue { session_id: String, queued: Vec<QueuedMessage> },
    Summary { session_id: String, text: String },
    Plan { session_id: String, path: String, title: String },
    SessionCreated { session: SessionSummary },
    SessionDeleted { session_id: String },
    Error { session_id: String, message: String },
    /// Sent after restore/regenerate/edit so the UI reloads the full session.
    SessionReset { session_id: String },
    Verification { session_id: String, report: crate::verify::VerificationReport },
    /// Auto router decided which model serves `message_id` (emitted as soon as the stream starts).
    Routed { session_id: String, message_id: String, routing: llm::RoutingInfo },
    AgentStatus { session_id: String, agent_id: String, status: crate::acp::AcpAgentStatus },
    ReviewComments { session_id: String, comments: Vec<ReviewComment> },
}

// ───────────────────────── session ─────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String, pub title: String, pub workspace_root: String, pub created_at: u64, pub updated_at: u64, pub mode: String,
    pub status: SessionStatus, pub message_count: usize, pub last_message_preview: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub worktree_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub is_subagent: Option<bool>,
    pub pending_change_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")] pub pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub archived: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub risk_tier: Option<u8>,
}

/// UI projection of a session (= TS `ChatSession`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSessionView {
    pub id: String, pub title: String, pub workspace_root: String, pub created_at: u64, pub updated_at: u64, pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub model_id: Option<String>,
    pub messages: Vec<ChatMessage>, pub todos: Vec<TodoItem>, pub usage: Usage, pub pending_changes: Vec<FileChangeSummary>,
    #[serde(skip_serializing_if = "Option::is_none")] pub worktree_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub branch: Option<String>,
    pub status: SessionStatus, pub queued_messages: Vec<QueuedMessage>,
    #[serde(skip_serializing_if = "Option::is_none")] pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub parent_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub is_subagent: Option<bool>,
    pub context_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")] pub pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub archived: Option<bool>,
    pub checkpoints: Vec<Checkpoint>,
    #[serde(skip_serializing_if = "Option::is_none")] pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub agent_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub last_verification_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub review_comments: Option<Vec<ReviewComment>>,
}

/// Full in-memory + on-disk session (transcript included).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub workspace_root: String,
    pub cwd: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub mode: String,
    pub model_id: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub transcript: Vec<LlmMessage>,
    /// Model the transcript's thinking blocks belong to.
    pub transcript_model: Option<String>,
    pub todos: Vec<TodoItem>,
    pub usage: Usage,
    pub changes: Vec<PendingChange>,
    pub checkpoints: Vec<Checkpoint>,
    pub worktree_path: Option<String>,
    pub branch: Option<String>,
    #[serde(skip)] pub status: SessionStatus,
    pub queued: Vec<QueuedMessage>,
    pub summary: Option<String>,
    pub parent_session_id: Option<String>,
    pub is_subagent: bool,
    pub pinned: bool,
    pub archived: bool,
    pub title_generated: bool,
    /// Per-session "allow for this session" decisions: command prefixes / tool names.
    pub session_allow: Vec<String>,
    /// (turn_id, transcript length before the turn) — used by checkpoint restore.
    pub transcript_marks: Vec<(String, usize)>,
    /// Current turn id while running.
    #[serde(skip)] pub current_turn: Option<String>,
    pub context_tokens: u64,
    /// 'caret' or an ACP agent id (None = caret).
    #[serde(default)] pub agent_id: Option<String>,
    #[serde(default)] pub last_verification_id: Option<String>,
    #[serde(default)] pub review_comments: Vec<ReviewComment>,
}
impl Default for Session {
    fn default() -> Self {
        Self { id: new_id(), title: "New chat".into(), workspace_root: String::new(), cwd: String::new(), created_at: now_ms(), updated_at: now_ms(), mode: "agent".into(), model_id: None, messages: vec![], transcript: vec![], transcript_model: None, todos: vec![], usage: Usage::default(), changes: vec![], checkpoints: vec![], worktree_path: None, branch: None, status: SessionStatus::Idle, queued: vec![], summary: None, parent_session_id: None, is_subagent: false, pinned: false, archived: false, title_generated: false, session_allow: vec![], transcript_marks: vec![], current_turn: None, context_tokens: 0, agent_id: None, last_verification_id: None, review_comments: vec![] }
    }
}
impl Session {
    pub fn root(&self) -> Option<PathBuf> { if self.workspace_root.is_empty() { None } else { Some(PathBuf::from(&self.workspace_root)) } }
    pub fn cwd_path(&self) -> PathBuf { if self.cwd.is_empty() { self.root().unwrap_or_else(crate::paths::home_dir) } else { PathBuf::from(&self.cwd) } }
    pub fn pending_summaries(&self) -> Vec<FileChangeSummary> { self.changes.iter().filter(|c| c.status == ChangeStatus::Pending).map(|c| c.summary()).collect() }
    pub fn touch(&mut self) { self.updated_at = now_ms(); }
    pub fn view(&self) -> ChatSessionView {
        ChatSessionView { id: self.id.clone(), title: self.title.clone(), workspace_root: self.workspace_root.clone(), created_at: self.created_at, updated_at: self.updated_at, mode: self.mode.clone(), model_id: self.model_id.clone(), messages: self.messages.clone(), todos: self.todos.clone(), usage: self.usage.clone(), pending_changes: self.pending_summaries(), worktree_path: self.worktree_path.clone(), branch: self.branch.clone(), status: self.status, queued_messages: self.queued.clone(), summary: self.summary.clone(), parent_session_id: self.parent_session_id.clone(), is_subagent: if self.is_subagent { Some(true) } else { None }, context_tokens: self.context_tokens, pinned: if self.pinned { Some(true) } else { None }, archived: if self.archived { Some(true) } else { None }, checkpoints: self.checkpoints.clone(), agent_id: self.agent_id.clone(), agent_kind: self.agent_id.as_deref().map(|a| if a == "caret" { "caret".to_string() } else { "acp".to_string() }), last_verification_id: self.last_verification_id.clone(), review_comments: if self.review_comments.is_empty() { None } else { Some(self.review_comments.clone()) } }
    }
    pub fn summary_view(&self) -> SessionSummary {
        let last = self.messages.iter().rev().find(|m| matches!(m.role, MessageRole::User | MessageRole::Assistant)).map(|m| m.preview(120)).unwrap_or_default();
        SessionSummary { id: self.id.clone(), title: self.title.clone(), workspace_root: self.workspace_root.clone(), created_at: self.created_at, updated_at: self.updated_at, mode: self.mode.clone(), status: self.status, message_count: self.messages.len(), last_message_preview: last, worktree_path: self.worktree_path.clone(), is_subagent: if self.is_subagent { Some(true) } else { None }, pending_change_count: self.changes.iter().filter(|c| c.status == ChangeStatus::Pending).count(), pinned: if self.pinned { Some(true) } else { None }, archived: if self.archived { Some(true) } else { None }, model_id: self.model_id.clone(), parent_session_id: self.parent_session_id.clone(), agent_id: self.agent_id.clone(), risk_tier: None }
    }
    /// Rough token estimate of the transcript (text + ~1500 per image).
    pub fn estimate_context_tokens(&self) -> u64 {
        let mut n = 0usize;
        for m in &self.transcript {
            for b in &m.content {
                n += match b {
                    ContentBlock::Text { text } => tokens::estimate(text),
                    ContentBlock::Image { .. } => 1500,
                    ContentBlock::ToolUse { input, name, .. } => tokens::estimate(name) + tokens::estimate(&input.to_string()),
                    ContentBlock::ToolResult { content, .. } => content.iter().map(|c| match c { llm::ToolResultContent::Text { text } => tokens::estimate(text), _ => 1500 }).sum(),
                    ContentBlock::Thinking { text, .. } => tokens::estimate(text),
                    ContentBlock::RedactedThinking { .. } => 100,
                };
            }
            n += 4;
        }
        n as u64
    }
    /// Drop model-bound thinking blocks when switching models.
    pub fn ensure_transcript_model(&mut self, model: &str) {
        if self.transcript_model.as_deref() == Some(model) { return; }
        if self.transcript_model.is_some() {
            for m in &mut self.transcript { m.content.retain(|b| !matches!(b, ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. })); }
            self.transcript.retain(|m| !m.content.is_empty());
        }
        self.transcript_model = Some(model.to_string());
    }
    pub fn message_mut(&mut self, id: &str) -> Option<&mut ChatMessage> { self.messages.iter_mut().find(|m| m.id == id) }
    pub fn find_tool_mut(&mut self, message_id: &str, call_id: &str) -> Option<&mut ToolCallRecord> {
        self.message_mut(message_id)?.parts.iter_mut().find_map(|p| if let MessagePart::Tool { call } = p { if call.id == call_id { Some(call) } else { None } } else { None })
    }
}

// ───────────────────────── modes / misc views ─────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeView { pub id: String, pub name: String, pub description: String, #[serde(skip_serializing_if = "Option::is_none")] pub icon: Option<String>, pub builtin: bool, pub tools: Value }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCatalogEntry { pub name: String, pub display_name: String, pub description: String, pub group: String, pub readonly: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPreviewItem { pub id: String, pub tokens: u64, pub truncated: bool }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPreview { pub total_tokens: u64, pub items: Vec<ContextPreviewItem> }

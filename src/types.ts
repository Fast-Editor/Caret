/**
 * Caret shared types — the contract between the Rust core and the UI.
 * Rust serde structs use `#[serde(rename_all = "camelCase")]` and mirror these 1:1.
 */

// ───────────────────────────── common ─────────────────────────────
export interface CaretError { code: string; message: string }
export type DeepPartial<T> = { [K in keyof T]?: T[K] extends object ? DeepPartial<T[K]> : T[K] }
export interface Range1 { startLine: number; endLine: number }
export interface LspPosition { line: number; character: number }
export interface LspRange { start: LspPosition; end: LspPosition }

// ───────────────────────────── app / workspace ─────────────────────────────
export interface AppInfo {
  version: string; os: string; arch: string; configDir: string; dataDir: string
  webview: string; debug: boolean; homeDir: string
}
export interface ProcessStats { rssBytes: number; cpuPercent: number; threads: number; uptimeSecs: number; totalMemoryBytes: number }
export interface WorkspaceInfo { root: string; name: string; isGitRepo: boolean; hasCaretDir: boolean; hasCursorDir: boolean }
export interface RecentWorkspace { path: string; name: string; lastOpened: number; exists: boolean }
export interface Notification {
  id: string; level: 'info' | 'warn' | 'error' | 'success'; title: string; message?: string
  actions?: { id: string; label: string }[]; timeoutMs?: number; source?: string
}

// ───────────────────────────── fs ─────────────────────────────
export type FileKind = 'file' | 'dir' | 'symlink' | 'other'
export interface DirEntry {
  name: string; path: string; kind: FileKind; size?: number; mtime?: number
  isHidden: boolean; isIgnored: boolean; hasChildren?: boolean; isSymlink?: boolean
}
export interface FileContent {
  path: string; content: string; encoding: 'utf8' | 'utf16le' | 'latin1' | 'binary'
  size: number; mtime: number; truncated: boolean; isBinary: boolean
  lineEnding: 'lf' | 'crlf' | 'mixed'; readonly: boolean
}
// ───────────────────────────── `caret` command-line launcher ─────────────────────────────
export interface OpenRequest { path: string; line?: number; col?: number }
export interface CliStatus { installed: boolean; path?: string; target?: string; scriptPath: string; onPath: boolean; error?: string }

export interface FileStat { path: string; kind: FileKind; size: number; mtime: number; readonly: boolean; exists: boolean }
export interface WriteResult { path: string; mtime: number; size: number }
export interface FsChangeEvent { kind: 'create' | 'modify' | 'remove' | 'rename' | 'any'; paths: string[] }
export interface QuickOpenResult { path: string; relPath: string; score: number; indices?: number[] }

// ───────────────────────────── search ─────────────────────────────
export interface SearchQuery {
  query: string; isRegex: boolean; caseSensitive: boolean; wholeWord: boolean
  includes: string[]; excludes: string[]; maxResults?: number; useIgnoreFiles: boolean
  contextLines?: number; roots?: string[]
}
export interface SearchMatch {
  path: string; line: number; column: number; lineText: string
  matchStart: number; matchEnd: number; before?: string[]; after?: string[]
}
export type SearchStreamEvent =
  | { type: 'match'; match: SearchMatch }
  | { type: 'done'; total: number; files: number; limited: boolean; elapsedMs: number }
  | { type: 'error'; message: string }
export interface ReplaceRequest { query: SearchQuery; replacement: string; paths?: string[] }
export interface ReplaceResult { files: number; replacements: number }

// ───────────────────────────── git ─────────────────────────────
export type GitFileStatus =
  | 'modified' | 'added' | 'deleted' | 'renamed' | 'copied' | 'untracked' | 'ignored' | 'conflicted' | 'typechange'
export interface GitFileChange { path: string; oldPath?: string; status: GitFileStatus; staged: boolean }
export interface GitStatus {
  isRepo: boolean; branch: string | null; upstream: string | null; ahead: number; behind: number
  staged: GitFileChange[]; unstaged: GitFileChange[]; untracked: GitFileChange[]; conflicted: GitFileChange[]
  isDetached: boolean; headSha: string | null; state: 'clean' | 'merge' | 'rebase' | 'cherrypick' | 'revert' | 'bisect' | 'normal'
  stashCount: number
}
export interface GitBranch { name: string; isCurrent: boolean; isRemote: boolean; upstream?: string; sha: string; lastCommitTime?: number }
export interface GitCommit {
  sha: string; shortSha: string; summary: string; body: string; author: string; email: string
  time: number; parents: string[]; refs: string[]
}
export interface GitBlameLine { line: number; sha: string; shortSha: string; author: string; time: number; summary: string; isUncommitted: boolean }
export interface GitWorktree { path: string; branch: string | null; head: string; isMain: boolean; isLocked: boolean }
export interface GitRemoteOpResult { ok: boolean; output: string; command: string }
export interface GitDiffStat { files: number; additions: number; deletions: number }
export interface GitCommitFile { path: string; status: GitFileStatus; additions: number; deletions: number }

// ───────────────────────────── terminal (pty) ─────────────────────────────
export interface PtySpawnOptions {
  cwd?: string; shell?: string; args?: string[]; env?: Record<string, string>
  cols: number; rows: number; name?: string
}
export interface PtyInfo { id: string; pid: number; title: string; cwd: string; shell: string; createdAt: number }
export type PtyEvent =
  | { type: 'data'; data: string }
  | { type: 'exit'; code: number | null }
  | { type: 'title'; title: string }
  | { type: 'cwd'; cwd: string }

// ───────────────────────────── lsp ─────────────────────────────
export interface LspServerConfig {
  id: string; name: string; command: string; args: string[]; languages: string[]; extensions: string[]
  initializationOptions?: unknown; settings?: unknown; enabled: boolean; rootPatterns?: string[]
  env?: Record<string, string>; installHint?: string
}
export interface LspServerStatus {
  serverId: string; configId: string; name: string; state: 'starting' | 'running' | 'stopped' | 'error' | 'missing'
  pid?: number; error?: string; languages: string[]; root: string
}
export interface LspDiagnostic {
  range: LspRange; severity?: 1 | 2 | 3 | 4; code?: string | number; source?: string; message: string
  tags?: number[]; relatedInformation?: { location: { uri: string; range: LspRange }; message: string }[]
  codeDescription?: { href: string }; data?: unknown
}
export interface FileDiagnostics { uri: string; path: string; diagnostics: LspDiagnostic[]; serverId: string; version?: number }
/** Raw JSON-RPC message from a server (responses, notifications, server→client requests). */
export type LspRawMessage = { jsonrpc: '2.0'; id?: number | string; method?: string; params?: unknown; result?: unknown; error?: { code: number; message: string; data?: unknown } }

// ───────────────────────────── settings ─────────────────────────────
export type ProviderKind = 'anthropic' | 'openai' | 'openai-compatible' | 'azure-openai' | 'ollama' | 'openrouter' | 'gemini' | 'anthropic-compatible'
export interface ProviderConfig {
  id: string; kind: ProviderKind; name: string; baseUrl?: string
  /** Key stored in ~/.caret/secrets.json under this id. */
  apiKeyRef?: string; enabled: boolean; headers?: Record<string, string>
  apiVersion?: string; authStyle?: 'x-api-key' | 'bearer' | 'api-key'
  /** Anthropic: enable server-side refusal fallbacks (first-party API only). */
  enableFallbacks?: boolean
}
export interface ModelConfig {
  id: string; providerId: string; model: string; displayName: string
  contextWindow: number; maxOutput: number
  supportsTools: boolean; supportsVision: boolean; supportsThinking: boolean; supportsEffort: boolean
  inputPricePerM?: number; outputPricePerM?: number; cacheReadPricePerM?: number; cacheWritePricePerM?: number
  enabled: boolean; isDefault?: boolean; tags?: string[]; isFast?: boolean
}
export type Effort = 'low' | 'medium' | 'high' | 'xhigh' | 'max'
export type ThinkingPref = 'auto' | 'on' | 'off'
export interface TabSettings {
  enabled: boolean; modelId?: string; debounceMs: number; maxLines: number; partialAccepts: boolean
  nextEditPrediction: boolean; suggestInComments: boolean; disabledLanguages: string[]
  contextLines: number; cacheSize: number
}
export type AutoRunMode = 'off' | 'allowlist' | 'all'
export interface AgentSettings {
  defaultMode: ModeId; defaultModelId?: string; autoRun: AutoRunMode
  commandAllowlist: string[]; commandDenylist: string[]
  protectDotfiles: boolean; protectDelete: boolean; protectOutsideWorkspace: boolean
  autoFixLints: boolean; autoApplyEdits: boolean; maxIterations: number; maxToolOutputChars: number
  webSearchEnabled: boolean; webSearchProvider: 'duckduckgo' | 'searxng' | 'brave' | 'tavily'
  webSearchEndpoint?: string; webSearchApiKeyRef?: string
  subagentModelId?: string; summarizeThresholdRatio: number; chimeOnFinish: boolean
  thinking: ThinkingPref; effort: Effort; commandTimeoutSecs: number; mcpAutoRun: boolean
  autoRunMcpTools: string[]; largeContextMode: boolean; parallelToolCalls: boolean
}
export interface IndexingSettings {
  enabled: boolean; embeddingsProviderId?: string; embeddingsModel?: string; maxFileSizeKb: number
  excludeGlobs: string[]; chunkLines: number; chunkOverlap: number; autoIndexNewFolders: boolean; hybridAlpha: number
  gitGraphIndexing: boolean
}
export type ThemeId = 'caret-dark' | 'caret-midnight' | 'caret-light' | 'caret-high-contrast'
export interface UiSettings {
  theme: ThemeId; fontFamily: string; fontSize: number; lineHeight: number; tabSize: number; insertSpaces: boolean
  wordWrap: 'on' | 'off' | 'bounded'; minimap: boolean; renderWhitespace: 'none' | 'boundary' | 'selection' | 'all'
  cursorBlinking: 'blink' | 'smooth' | 'phase' | 'expand' | 'solid'; cursorStyle: 'line' | 'block' | 'underline'
  bracketPairColorization: boolean; formatOnSave: boolean; autoSave: 'off' | 'afterDelay' | 'onFocusChange'
  autoSaveDelayMs: number; chatFontSize: number; sidebarPosition: 'left' | 'right'; chatPosition: 'right' | 'left'
  terminalFontFamily: string; terminalFontSize: number; terminalScrollback: number; terminalCursorStyle: 'block' | 'bar' | 'underline'; terminalCursorBlink: boolean; showStatusBarMemory: boolean
  layout: 'editor' | 'agents'; reduceMotion: boolean; stickyScroll: boolean; lineNumbers: 'on' | 'off' | 'relative'
  smoothScrolling: boolean; fontLigatures: boolean; editorTabsMode: 'multiple' | 'single'; previewTabs: boolean
  breadcrumbs: boolean; zoomLevel: number; confirmDelete: boolean; compactFolders: boolean
}
export interface PrivacySettings { privacyMode: boolean; allowWebTools: boolean; allowRemoteEmbeddings: boolean }
export interface GeneralSettings {
  defaultShell?: string; openLastWorkspace: boolean; confirmQuitWithRunningAgents: boolean
  gitAutoFetch: boolean; gitAutoFetchIntervalSecs: number; globalIgnore: string[]; trustedWorkspaces: string[]
  showWelcome: boolean; useCursorCompatDirs: boolean
}
export interface CustomMode {
  id: string; name: string; icon?: string; description?: string; instructions: string
  tools: string[] | 'all' | 'readonly'; autoRun?: boolean; modelId?: string; keybinding?: string
}
export interface BetaSettings { browserTools: boolean; voice: boolean; nextEditPrediction: boolean; backgroundAgents: boolean; designMode: boolean }
export interface Settings {
  providers: ProviderConfig[]; models: ModelConfig[]
  tab: TabSettings; agent: AgentSettings; indexing: IndexingSettings; ui: UiSettings
  privacy: PrivacySettings; general: GeneralSettings
  userRules: string; lsp: { enabled: boolean; servers: LspServerConfig[] }
  mcp: { enabled: boolean }; customModes: CustomMode[]; beta: BetaSettings
  keybindings: Record<string, string | null>
  verify: VerifySettings; sandbox: SandboxSettings; acp: AcpSettings; terminal: TerminalSettings; router: RouterSettings
}
export interface RouterSettings { enabled: boolean; source: 'npm' | 'bundled'; overrides: Record<string, string>; excludedModels: string[]; stickySessions: boolean; routeSingleShot: boolean; tpmLimit?: number; embeddingsProviderId?: string }
export type RouterTier = 'simple' | 'medium' | 'complex' | 'reasoning'
export const ROUTER_TIERS: RouterTier[] = ['simple', 'medium', 'complex', 'reasoning']
export interface TierSlot { modelId: string; source: 'auto' | 'override'; reason?: string }
export interface RouterAssignment { slots: Partial<Record<RouterTier, TierSlot>>; candidates: number; warnings: string[]; hash: string }
export type RouterState = 'disabled' | 'not-installed' | 'installing' | 'starting' | 'healthy' | 'degraded' | 'error'
export interface RouterStatus { state: RouterState; reason?: string; port?: number; bridgePort?: number; version?: string; installed: boolean; nativeOk: boolean; assignment: RouterAssignment; warnings: string[]; pid?: number }
/** Which model served an Auto-routed assistant message. */
export interface RoutingInfo { via: string; requestedTier?: string; servedTier?: string; servedModelId: string; score?: number; method?: string; fallback: boolean }
export type SettingsPatch = DeepPartial<Settings>
export interface SettingsScopes { user: Settings; workspace: DeepPartial<Settings> | null; effective: Settings; userPath: string; workspacePath: string | null }

// ───────────────────────────── chat / agent ─────────────────────────────
export type ModeId = 'agent' | 'ask' | 'plan' | 'debug' | (string & {})
export type ContextItemKind =
  | 'file' | 'folder' | 'code' | 'selection' | 'symbol' | 'codebase' | 'git-diff' | 'git-staged' | 'git-commit' | 'git-branch-diff'
  | 'web' | 'docs' | 'terminal' | 'lint' | 'recent-changes' | 'past-chat' | 'rule' | 'link' | 'image' | 'note'
  | 'mcp-resource' | 'active-file' | 'open-files' | 'plan' | 'skill'
export interface ContextItem {
  id: string; kind: ContextItemKind; label: string; path?: string; range?: Range1; url?: string; query?: string
  data?: string; mime?: string; terminalId?: string; sessionId?: string; sha?: string; ruleId?: string
  serverId?: string; uri?: string; pinned?: boolean; base?: string; docId?: string
}
export interface ImageAttachment { id: string; name: string; mime: string; dataBase64: string; width?: number; height?: number }
export interface SendMessageRequest {
  sessionId: string; text: string; context: ContextItem[]; images: ImageAttachment[]
  mode: ModeId; modelId?: string; thinking?: ThinkingPref; effort?: Effort
  useWorktree?: boolean; queue?: boolean; activeFile?: string; openFiles?: string[]; cursorSelection?: { path: string; range: Range1; text: string }
  /** 'caret' (built-in) or an ACP agent id; defaults to the session's agent. */
  agentId?: string; reviewComments?: ReviewComment[]
}
export type MessageRole = 'user' | 'assistant' | 'system'
export type ToolStatus = 'pending' | 'awaiting-approval' | 'running' | 'done' | 'error' | 'rejected' | 'cancelled'
export type ApprovalOption = 'allow' | 'allow-session' | 'allow-always' | 'deny' | 'deny-always'
export interface ApprovalRequest {
  approvalId: string; sessionId: string; toolCallId: string
  kind: 'command' | 'mcp' | 'file-delete' | 'file-edit' | 'outside-workspace' | 'web' | 'subagent' | 'browser'
  title: string; detail: string; command?: string; cwd?: string; options: ApprovalOption[]; allowAlwaysPattern?: string
  classification?: CommandClassification; riskTier?: RiskTier; sandboxed?: boolean; agentId?: string
}
export interface AgentQuestion { questionId: string; question: string; options?: string[]; allowFreeText: boolean }
export interface FileChangeSummary {
  changeId: string; path: string; kind: 'create' | 'modify' | 'delete' | 'rename'; additions: number; deletions: number
  status: 'pending' | 'accepted' | 'rejected'; toolCallId?: string; turnId?: string; oldPath?: string
}
export interface ToolCallRecord {
  id: string; name: string; displayName: string; input: unknown; inputText?: string; status: ToolStatus
  output?: string; outputTruncated?: boolean; error?: string; startedAt?: number; finishedAt?: number
  fileChanges?: FileChangeSummary[]; mcpServerId?: string; command?: string; cwd?: string; exitCode?: number
  isBackground?: boolean; diff?: string; approval?: ApprovalRequest; subagentSessionId?: string; summary?: string
  sandboxed?: boolean; classification?: CommandClassification; locations?: { path: string; line?: number }[]; rawKind?: string
}
export type MessagePart =
  | { type: 'text'; text: string }
  | { type: 'thinking'; text: string; durationMs?: number; redacted?: boolean }
  | { type: 'tool'; call: ToolCallRecord }
  | { type: 'image'; image: ImageAttachment }
  | { type: 'context'; items: ContextItem[] }
  | { type: 'error'; message: string }
  | { type: 'plan'; path: string; title: string }
  | { type: 'question'; question: AgentQuestion; answer?: string }
  | { type: 'summary'; text: string }
  | { type: 'checkpoint'; checkpointId: string }
export interface Usage {
  inputTokens: number; outputTokens: number; cacheReadTokens: number; cacheWriteTokens: number
  thinkingTokens?: number; costUsd?: number; requests?: number
}
export interface ChatMessage {
  id: string; role: MessageRole; parts: MessagePart[]; createdAt: number; modelId?: string; mode?: ModeId
  usage?: Usage; status?: 'streaming' | 'done' | 'error' | 'cancelled'; checkpointId?: string; turnId?: string; durationMs?: number
  routing?: RoutingInfo
}
export interface TodoItem { id: string; content: string; status: 'pending' | 'in_progress' | 'completed' | 'cancelled' }
export interface QueuedMessage { id: string; text: string; context: ContextItem[]; images: ImageAttachment[]; mode?: ModeId }
export type SessionStatus = 'idle' | 'running' | 'awaiting-approval' | 'awaiting-input' | 'error' | 'summarizing'
export interface ChatSession {
  id: string; title: string; workspaceRoot: string; createdAt: number; updatedAt: number; mode: ModeId; modelId?: string
  messages: ChatMessage[]; todos: TodoItem[]; usage: Usage; pendingChanges: FileChangeSummary[]
  worktreePath?: string; branch?: string; status: SessionStatus; queuedMessages: QueuedMessage[]
  summary?: string; parentSessionId?: string; isSubagent?: boolean; contextTokens: number; pinned?: boolean; archived?: boolean
  agentId?: string; agentKind?: AgentKind; lastVerificationId?: string; reviewComments?: ReviewComment[]
}
export interface ChatSessionSummary {
  id: string; title: string; workspaceRoot: string; createdAt: number; updatedAt: number; mode: ModeId
  status: SessionStatus; messageCount: number; lastMessagePreview: string; worktreePath?: string; isSubagent?: boolean
  pendingChangeCount: number; pinned?: boolean; archived?: boolean; modelId?: string; agentId?: string; riskTier?: RiskTier
}
export type AgentEvent =
  | { type: 'session-status'; sessionId: string; status: SessionStatus }
  | { type: 'message-start'; sessionId: string; message: ChatMessage }
  | { type: 'text-delta'; sessionId: string; messageId: string; text: string }
  | { type: 'thinking-delta'; sessionId: string; messageId: string; text: string }
  | { type: 'tool-start'; sessionId: string; messageId: string; call: ToolCallRecord }
  | { type: 'tool-input-delta'; sessionId: string; messageId: string; toolCallId: string; text: string }
  | { type: 'tool-update'; sessionId: string; messageId: string; call: ToolCallRecord }
  | { type: 'approval-request'; sessionId: string; request: ApprovalRequest }
  | { type: 'question'; sessionId: string; messageId: string; question: AgentQuestion }
  | { type: 'file-change'; sessionId: string; change: FileChangeSummary; diff?: string }
  | { type: 'file-change-status'; sessionId: string; changeIds: string[]; status: 'accepted' | 'rejected' }
  | { type: 'checkpoint'; sessionId: string; checkpointId: string; messageId: string }
  | { type: 'todos'; sessionId: string; todos: TodoItem[] }
  | { type: 'usage'; sessionId: string; messageId: string; usage: Usage; contextTokens: number; sessionUsage: Usage }
  | { type: 'message-end'; sessionId: string; messageId: string; status: 'done' | 'error' | 'cancelled'; error?: string; durationMs: number }
  | { type: 'title'; sessionId: string; title: string }
  | { type: 'queue'; sessionId: string; queued: QueuedMessage[] }
  | { type: 'summary'; sessionId: string; text: string }
  | { type: 'plan'; sessionId: string; path: string; title: string }
  | { type: 'session-created'; session: ChatSessionSummary }
  | { type: 'session-deleted'; sessionId: string }
  | { type: 'session-reset'; sessionId: string }
  | { type: 'verification'; sessionId: string; report: VerificationReport }
  | { type: 'agent-status'; sessionId: string; agentId: string; status: AcpAgentStatus }
  | { type: 'routed'; sessionId: string; messageId: string; routing: RoutingInfo }
  | { type: 'review-comments'; sessionId: string; comments: ReviewComment[] }
  | { type: 'error'; sessionId: string; message: string }
export interface PendingChange {
  changeId: string; sessionId: string; path: string; kind: FileChangeSummary['kind']
  before: string | null; after: string | null; diff: string; status: FileChangeSummary['status']; turnId?: string; toolCallId?: string
}
export interface Checkpoint { id: string; sessionId: string; messageId: string; createdAt: number; files: string[]; label: string }

// ───────────────────────────── tab / inline / single-shot AI ─────────────────────────────
export interface RecentEdit { path: string; before: string; after: string; line: number; at: number }
export interface CompletionRequest {
  requestId: string; path: string; language: string; prefix: string; suffix: string
  cursorLine: number; cursorColumn: number; recentEdits?: RecentEdit[]; diagnostics?: string[]
  openFiles?: { path: string; snippet: string }[]; multiline: boolean; manual?: boolean
}
export interface CompletionResult { requestId: string; text: string; modelId?: string; latencyMs: number; cached: boolean }
export interface NextEditRequest { requestId: string; path: string; language: string; content: string; cursorLine: number; recentEdits: RecentEdit[] }
export interface NextEditResult { requestId: string; found: boolean; startLine?: number; endLine?: number; newText?: string; reason?: string }
export interface InlineEditRequest {
  requestId: string; path: string; language: string; fullText: string; startLine: number; endLine: number
  selectedText: string; instruction: string; context: ContextItem[]; mode: 'edit' | 'question' | 'generate'; modelId?: string
  history?: { instruction: string; result: string }[]
}
export type InlineEditEvent =
  | { type: 'delta'; text: string }
  | { type: 'done'; text: string; usage?: Usage }
  | { type: 'error'; message: string }
export interface TerminalCommandRequest { requestId: string; instruction: string; shell: string; cwd: string; recentOutput?: string; os: string }
export interface TerminalCommandResult { command: string; explanation?: string }
export interface ApplyRequest { path: string; code: string; language?: string; instructions?: string; sessionId?: string; modelId?: string }
export interface ApplyResult { path: string; before: string; after: string; diff: string; method: 'search-replace' | 'rewrite' | 'create' | 'heuristic'; changeId?: string }
export interface CommitMessageRequest { style?: 'conventional' | 'plain'; includeBody: boolean; modelId?: string }
export interface CommitMessageResult { summary: string; body: string }
export interface ReviewRequest { scope: 'working-tree' | 'staged' | 'branch' | 'commit' | 'files'; base?: string; files?: string[]; focus?: string; modelId?: string; sha?: string }
export interface ReviewFinding {
  id: string; path: string; line: number; endLine?: number; severity: 'critical' | 'high' | 'medium' | 'low' | 'info'
  title: string; description: string; suggestion?: string; category: string; confidence?: number
}
export interface ReviewResult {
  id: string; findings: ReviewFinding[]; summary: string; stats: GitDiffStat; createdAt: number; modelId: string; scope: ReviewRequest['scope']
}
export type ReviewStreamEvent = { type: 'status'; message: string } | { type: 'finding'; finding: ReviewFinding } | { type: 'done'; result: ReviewResult } | { type: 'error'; message: string }

// ───────────────────────────── rules / memories / skills / commands / subagents / hooks / plans ─────────────────────────────
export type RuleKind = 'always' | 'auto' | 'agent' | 'manual'
export interface RuleInfo {
  id: string; path: string; name: string; description?: string; globs: string[]; alwaysApply: boolean; kind: RuleKind
  source: 'project' | 'user' | 'legacy' | 'agents-md' | 'claude-md' | 'nested' | 'cursor-project'; enabled: boolean; sizeBytes: number
}
export interface MemoryItem { id: string; text: string; createdAt: number; source: 'agent' | 'user'; sessionId?: string; enabled: boolean; title?: string }
export interface SkillInfo { id: string; name: string; description: string; path: string; source: 'project' | 'user' | 'cursor' | 'claude' }
export interface SlashCommandInfo { name: string; description?: string; path?: string; source: 'project' | 'user' | 'builtin' | 'cursor' ; argumentHint?: string }
export interface SubagentInfo { id: string; name: string; description: string; path: string; modelId?: string; tools?: string[]; source: 'project' | 'user' | 'cursor' }
export type HookEvent =
  | 'beforeSubmitPrompt' | 'beforeShellExecution' | 'beforeMCPExecution' | 'beforeReadFile'
  | 'afterFileEdit' | 'afterAgentResponse' | 'afterAgentThought' | 'stop' | 'sessionStart' | 'sessionEnd'
export interface HookInfo { event: HookEvent; command: string; source: string; timeoutMs?: number; matcher?: string }
export interface PlanInfo { path: string; title: string; sessionId?: string; updatedAt: number; status: 'draft' | 'approved' | 'building' | 'done' }

// ───────────────────────────── index / docs ─────────────────────────────
export type IndexState = 'disabled' | 'idle' | 'scanning' | 'indexing' | 'embedding' | 'ready' | 'error' | 'paused'
export interface IndexStatus {
  state: IndexState; totalFiles: number; indexedFiles: number; totalChunks: number; embeddedChunks: number
  pendingFiles: number; lastIndexedAt?: number; error?: string; sizeBytes: number; embeddingsAvailable: boolean; root: string | null
  progress: number
}
export interface CodebaseSearchHit { path: string; startLine: number; endLine: number; score: number; snippet: string; kind?: string; name?: string; source: 'bm25' | 'vector' | 'hybrid' }
export interface DocSource { id: string; name: string; url: string; pages: number; chunks: number; status: 'crawling' | 'ready' | 'error' | 'queued'; error?: string; addedAt: number; maxPages: number }
export interface DocSearchHit { docId: string; url: string; title: string; snippet: string; score: number }

// ───────────────────────────── mcp ─────────────────────────────
export type McpTransport = 'stdio' | 'http' | 'sse'
export interface McpServerConfig {
  id: string; name: string; transport: McpTransport; command?: string; args?: string[]; env?: Record<string, string>
  url?: string; headers?: Record<string, string>; enabled: boolean; cwd?: string
  source: 'project' | 'user' | 'cursor-project' | 'cursor-user'; disabledTools?: string[]
}
export interface McpToolInfo { serverId: string; name: string; qualifiedName: string; description?: string; inputSchema: unknown; enabled: boolean }
export interface McpResourceInfo { serverId: string; uri: string; name: string; description?: string; mimeType?: string }
export interface McpPromptInfo { serverId: string; name: string; description?: string; arguments?: { name: string; description?: string; required?: boolean }[] }
export interface McpServerStatus {
  id: string; name: string; state: 'connecting' | 'connected' | 'error' | 'disabled' | 'disconnected'
  error?: string; toolCount: number; resourceCount: number; promptCount: number; transport: McpTransport; source: McpServerConfig['source']
}
export interface McpToolResult { content: { type: 'text' | 'image' | 'resource'; text?: string; data?: string; mimeType?: string; uri?: string }[]; isError: boolean }

// ───────────────────────────── browser ─────────────────────────────
export interface BrowserTab { id: string; url: string; title: string; label: string }
export interface BrowserElement { ref: string; tag: string; text: string; role?: string; href?: string; placeholder?: string; value?: string; name?: string }
export interface BrowserSnapshot { url: string; title: string; text: string; elements: BrowserElement[]; consoleLogs: string[] }

// ───────────────────────────── commands / keybindings (UI) ─────────────────────────────
export interface CommandDescriptor { id: string; title: string; category?: string; keybinding?: string; when?: string; icon?: string }
export interface KeybindingEntry { command: string; key: string; mac?: string; when?: string; source: 'default' | 'user' }

// ═══════════════════════════ Wave 1 additions (verification · sandbox · terminal blocks · ACP · review comments) ═══════════════════════════

// ───────────────────────────── verification (Verified Diff) ─────────────────────────────
export type VerifyScopeKind = 'session' | 'change-set' | 'staged' | 'working-tree' | 'files' | 'branch'
export interface VerifyScope { kind: VerifyScopeKind; sessionId?: string; changeIds?: string[]; files?: string[]; base?: string }
export type VerifyCheckKind = 'diagnostics' | 'project-check' | 'dependencies' | 'security' | 'tests' | 'mutation' | 'equivalence' | 'properties' | 'lint'
export type VerifyStatus = 'pending' | 'running' | 'pass' | 'warn' | 'fail' | 'skipped' | 'error'
export type VerifySeverity = 'critical' | 'high' | 'medium' | 'low' | 'info'
export interface VerifyFinding {
  id: string; path?: string; line?: number; endLine?: number; severity: VerifySeverity; message: string
  rule?: string; suggestion?: string; fixable?: boolean; evidence?: string; url?: string
}
export interface VerifyCheck {
  id: string; kind: VerifyCheckKind; title: string; status: VerifyStatus; summary: string
  findings: VerifyFinding[]; durationMs?: number; command?: string; output?: string; skippedReason?: string
  /** e.g. mutation: { killed, survived, total, score } tests: { passed, failed, total } */
  metrics?: Record<string, number>
}
export type RiskTier = 0 | 1 | 2
export interface VerificationReport {
  id: string; scope: VerifyScope; sessionId?: string; workspaceRoot: string; startedAt: number; finishedAt?: number
  status: VerifyStatus; checks: VerifyCheck[]; riskTier: RiskTier; riskReasons: string[]
  /** Machine-established facts vs statements the agent made about its own work. */
  facts: string[]; claims: string[]
  filesTouched: string[]; additions: number; deletions: number; summary: string
}
export type VerifyEvent =
  | { type: 'report'; report: VerificationReport }
  | { type: 'check'; reportId: string; check: VerifyCheck }
  | { type: 'done'; report: VerificationReport }
  | { type: 'error'; reportId?: string; message: string }
export interface VerifySettings {
  autoRunAfterTurn: boolean; runTests: boolean; mutationTesting: boolean; mutationMaxMutants: number
  dependencyGrounding: boolean; securityRules: boolean; projectCheckCommand?: string; testCommand?: string; preTestCommand?: string
  timeoutSecs: number; blockMergeOnFail: boolean; useSemgrepIfAvailable: boolean
}

// ───────────────────────────── sandbox & command safety ─────────────────────────────
export type CommandRisk = 'safe' | 'caution' | 'destructive' | 'blocked'
export interface CommandSegment { text: string; risk: CommandRisk; labels: string[]; program?: string }
export interface CommandClassification {
  command: string; segments: CommandSegment[]; overallRisk: CommandRisk; labels: string[]; blocked: boolean; reasons: string[]
  /** Deterministic allowlist/denylist verdict computed below the model. */
  allowlisted: boolean; denylisted: boolean
}
export interface SandboxStatus { available: boolean; backend: 'seatbelt' | 'bubblewrap' | 'landlock' | 'none'; reason?: string; networkIsolation: boolean }
export interface SandboxSettings { mode: 'off' | 'auto' | 'strict'; allowNetwork: boolean; extraWritablePaths: string[]; allowReadHome: boolean; pinMcpToolDescriptions: boolean }
export interface AcpSettings { enabled: boolean; defaultAgentId: string; agents: AcpAgentConfig[]; passMcpServers: boolean; sessionDefaults?: Record<string, Record<string, unknown>> }
export interface TerminalSettings { blocks: boolean; shellIntegration: boolean; aiEnabled: boolean; programStatus: boolean; maxBlocks: number }
export interface McpToolChange { serverId: string; tool: string; previousHash: string; currentHash: string; previousDescription?: string; currentDescription: string; detectedAt: number }

// ───────────────────────────── terminal blocks (OSC 133) & program status (OSC 7501) ─────────────────────────────
export type ProgramStatus = 'idle' | 'working' | 'waiting' | 'done' | 'failed'
export interface TerminalBlock {
  id: string; index: number; command?: string; cwd?: string; exitCode?: number | null; startedAt: number; endedAt?: number
  startLine: number; endLine?: number; outputPreview?: string; running: boolean
}
export type PtyBlockEvent =
  | { type: 'block-start'; block: TerminalBlock }
  | { type: 'block-command'; blockId: string; command: string }
  | { type: 'block-end'; blockId: string; exitCode: number | null; endedAt: number; endLine: number }
  | { type: 'status'; status: ProgramStatus; title?: string; detail?: string }
export type PtyEventExt = PtyEvent | PtyBlockEvent

// ───────────────────────────── ACP (Agent Client Protocol) ─────────────────────────────
export type AgentKind = 'caret' | 'acp'
export interface AcpAgentConfig {
  id: string; name: string; command: string; args: string[]; env?: Record<string, string>; enabled: boolean
  source: 'builtin' | 'user' | 'registry'; description?: string; icon?: string; installHint?: string; website?: string
}
export interface AcpAgentStatus {
  id: string; name: string; installed: boolean; state: 'idle' | 'starting' | 'ready' | 'error' | 'unsupported'
  version?: string; protocolVersion?: number; error?: string; authRequired?: boolean; authMethods?: { id: string; name: string; description?: string }[]
  capabilities?: { loadSession?: boolean; promptImages?: boolean; mcp?: boolean }
  modes?: { id: string; name: string; description?: string }[]; currentMode?: string
  /** ACP session config options (model, effort, permission mode …) for the session this status was emitted for. */
  configOptions?: AcpConfigOption[]
}
export interface AcpConfigOption { id: string; name: string; description?: string; category?: 'model' | 'mode' | 'thought_level' | string; type: 'select' | string; currentValue: unknown; options: { value: string; name: string; description?: string }[] }
export interface AcpRegistryEntry { id: string; name: string; description?: string; install?: { npm?: string; command?: string; args?: string[] }; website?: string; installed: boolean }

// ───────────────────────────── review comments (review-on-diff as feedback) ─────────────────────────────
export interface ReviewComment {
  id: string; path: string; line: number; endLine?: number; side: 'before' | 'after'; text: string
  changeId?: string; createdAt: number; resolved?: boolean; author: 'user' | 'bot'; findingId?: string
}
export interface ReviewGroup {
  id: string; title: string; kind: 'intent' | 'tool-call' | 'file-kind' | 'other'; turnId?: string; toolCallId?: string
  changeIds: string[]; collapsedByDefault: boolean; order: number
}

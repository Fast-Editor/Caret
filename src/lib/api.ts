/**
 * Typed command catalog. Every function maps 1:1 to a Rust `#[tauri::command]`.
 * Argument names are camelCase here and snake_case in Rust (Tauri converts).
 */
import type * as T from '@/types'
import { invoke, channel, type Channel } from './tauri'

const apiBase = {
  app: {
    info: () => invoke<T.AppInfo>('app_info'),
    processStats: () => invoke<T.ProcessStats>('app_process_stats'),
    openExternal: (url: string) => invoke<void>('app_open_external', { url }),
    showInFolder: (path: string) => invoke<void>('app_show_in_folder', { path }),
    openDevtools: () => invoke<void>('app_open_devtools'),
    log: (level: 'debug' | 'info' | 'warn' | 'error', message: string) => invoke<void>('app_log', { level, message }),
  },

  workspace: {
    open: (path: string) => invoke<T.WorkspaceInfo>('workspace_open', { path }),
    current: () => invoke<T.WorkspaceInfo | null>('workspace_current'),
    close: () => invoke<void>('workspace_close'),
    recent: () => invoke<T.RecentWorkspace[]>('workspace_recent'),
    removeRecent: (path: string) => invoke<void>('workspace_remove_recent', { path }),
    ensureProjectDir: () => invoke<string>('workspace_ensure_project_dir'),
  },

  fs: {
    readDir: (path: string, showHidden = true) => invoke<T.DirEntry[]>('fs_read_dir', { path, showHidden }),
    readFile: (path: string, maxBytes?: number) => invoke<T.FileContent>('fs_read_file', { path, maxBytes }),
    writeFile: (path: string, content: string, opts?: { createDirs?: boolean; lineEnding?: 'lf' | 'crlf' }) =>
      invoke<T.WriteResult>('fs_write_file', { path, content, createDirs: opts?.createDirs ?? true, lineEnding: opts?.lineEnding }),
    stat: (path: string) => invoke<T.FileStat>('fs_stat', { path }),
    exists: (path: string) => invoke<boolean>('fs_exists', { path }),
    create: (path: string, kind: 'file' | 'dir') => invoke<void>('fs_create', { path, kind }),
    rename: (from: string, to: string) => invoke<void>('fs_rename', { from, to }),
    delete: (paths: string[], trash = true) => invoke<void>('fs_delete', { paths, trash }),
    copy: (from: string, to: string) => invoke<void>('fs_copy', { from, to }),
    move: (from: string, to: string) => invoke<void>('fs_move', { from, to }),
    quickOpen: (query: string, limit = 50) => invoke<T.QuickOpenResult[]>('fs_quick_open', { query, limit }),
    readImage: (path: string) => invoke<{ dataBase64: string; mime: string }>('fs_read_image', { path }),
    /** All non-ignored file paths (relative) — used for @-mention pickers; capped. */
    listFiles: (limit = 20000) => invoke<string[]>('fs_list_files', { limit }),
  },

  search: {
    /** Streams results into `onEvent`; resolves with a searchId usable for cancel. */
    text: (query: T.SearchQuery, onEvent: (e: T.SearchStreamEvent) => void) =>
      invoke<string>('search_text', { query, onEvent: channel(onEvent) }),
    cancel: (searchId: string) => invoke<void>('search_cancel', { searchId }),
    replace: (req: T.ReplaceRequest) => invoke<T.ReplaceResult>('search_replace', { req }),
  },

  git: {
    status: () => invoke<T.GitStatus>('git_status'),
    diff: (opts: { path?: string; staged?: boolean; base?: string }) => invoke<string>('git_diff', opts),
    diffStat: (base?: string) => invoke<T.GitDiffStat>('git_diff_stat', { base }),
    stage: (paths: string[]) => invoke<void>('git_stage', { paths }),
    unstage: (paths: string[]) => invoke<void>('git_unstage', { paths }),
    stageAll: () => invoke<void>('git_stage_all'),
    unstageAll: () => invoke<void>('git_unstage_all'),
    discard: (paths: string[]) => invoke<void>('git_discard', { paths }),
    commit: (message: string, opts?: { amend?: boolean; signoff?: boolean }) =>
      invoke<string>('git_commit', { message, amend: opts?.amend ?? false, signoff: opts?.signoff ?? false }),
    log: (opts?: { limit?: number; skip?: number; path?: string; rev?: string }) => invoke<T.GitCommit[]>('git_log', { ...opts }),
    commitFiles: (sha: string) => invoke<T.GitCommitFile[]>('git_commit_files', { sha }),
    showFile: (path: string, rev: string) => invoke<string>('git_show_file', { path, rev }),
    blame: (path: string) => invoke<T.GitBlameLine[]>('git_blame', { path }),
    branches: () => invoke<T.GitBranch[]>('git_branches'),
    checkout: (ref: string, create = false) => invoke<void>('git_checkout', { ref, create }),
    createBranch: (name: string, checkout = true) => invoke<void>('git_create_branch', { name, checkout }),
    deleteBranch: (name: string, force = false) => invoke<void>('git_delete_branch', { name, force }),
    push: (opts?: { remote?: string; branch?: string; setUpstream?: boolean; force?: boolean }) => invoke<T.GitRemoteOpResult>('git_push', { ...opts }),
    pull: (rebase = false) => invoke<T.GitRemoteOpResult>('git_pull', { rebase }),
    fetch: () => invoke<T.GitRemoteOpResult>('git_fetch'),
    stash: (message?: string) => invoke<T.GitRemoteOpResult>('git_stash', { message }),
    stashPop: () => invoke<T.GitRemoteOpResult>('git_stash_pop'),
    worktrees: () => invoke<T.GitWorktree[]>('git_worktrees'),
    worktreeAdd: (path: string, branch: string, createBranch = true) => invoke<T.GitWorktree>('git_worktree_add', { path, branch, createBranch }),
    worktreeRemove: (path: string, force = false) => invoke<void>('git_worktree_remove', { path, force }),
    init: () => invoke<void>('git_init'),
    remotes: () => invoke<{ name: string; url: string }[]>('git_remotes'),
    defaultBranch: () => invoke<string | null>('git_default_branch'),
  },

  pty: {
    spawn: (options: T.PtySpawnOptions, onEvent: (e: T.PtyEventExt) => void) =>
      invoke<T.PtyInfo>('pty_spawn', { options, onEvent: channel(onEvent) }),
    write: (id: string, data: string) => invoke<void>('pty_write', { id, data }),
    resize: (id: string, cols: number, rows: number) => invoke<void>('pty_resize', { id, cols, rows }),
    kill: (id: string) => invoke<void>('pty_kill', { id }),
    list: () => invoke<T.PtyInfo[]>('pty_list'),
    readBuffer: (id: string, lines = 200) => invoke<string>('pty_read_buffer', { id, lines }),
    runCommand: (command: string, cwd?: string, timeoutMs = 60000) =>
      invoke<{ stdout: string; stderr: string; code: number | null; timedOut: boolean }>('pty_run_command', { command, cwd, timeoutMs }),
  },

  lsp: {
    configs: () => invoke<{ config: T.LspServerConfig; installed: boolean }[]>('lsp_configs'),
    start: (configId: string, root: string, onMessage: (m: T.LspRawMessage) => void) =>
      invoke<T.LspServerStatus>('lsp_start', { configId, root, onMessage: channel(onMessage) }),
    stop: (serverId: string) => invoke<void>('lsp_stop', { serverId }),
    send: (serverId: string, message: T.LspRawMessage) => invoke<void>('lsp_send', { serverId, message }),
    status: () => invoke<T.LspServerStatus[]>('lsp_status'),
    diagnostics: () => invoke<T.FileDiagnostics[]>('lsp_diagnostics'),
    diagnosticsFor: (path: string) => invoke<T.FileDiagnostics[]>('lsp_diagnostics_for', { path }),
    /** UI pushes diagnostics it computed (e.g. Monaco TS worker) so agent tools can read them. */
    reportDiagnostics: (diags: T.FileDiagnostics) => invoke<void>('lsp_report_diagnostics', { diags }),
  },

  settings: {
    get: () => invoke<T.SettingsScopes>('settings_get'),
    update: (patch: T.SettingsPatch, scope: 'user' | 'workspace' = 'user') => invoke<T.Settings>('settings_update', { patch, scope }),
    reset: (scope: 'user' | 'workspace') => invoke<T.Settings>('settings_reset', { scope }),
    setSecret: (id: string, value: string) => invoke<void>('settings_set_secret', { id, value }),
    deleteSecret: (id: string) => invoke<void>('settings_delete_secret', { id }),
    listSecrets: () => invoke<string[]>('settings_list_secrets'),
    path: (scope: 'user' | 'workspace' | 'keybindings') => invoke<string>('settings_path', { scope }),
    listRemoteModels: (providerId: string) => invoke<string[]>('models_list_remote', { providerId }),
    testModel: (modelId: string) => invoke<{ ok: boolean; message: string; latencyMs: number }>('models_test', { modelId }),
    importEnvKeys: () => invoke<string[]>('settings_import_env_keys'),
  },

  chat: {
    /** Register the single agent event stream. Call once at startup (re-call after HMR). */
    subscribe: (onEvent: (e: T.AgentEvent) => void) => invoke<void>('chat_subscribe', { onEvent: channel(onEvent) }),
    listSessions: (includeArchived = false) => invoke<T.ChatSessionSummary[]>('chat_list_sessions', { includeArchived }),
    createSession: (opts?: { mode?: T.ModeId; modelId?: string; title?: string; useWorktree?: boolean; agentId?: string }) => invoke<T.ChatSession>('chat_create_session', { ...opts }),
    getSession: (sessionId: string) => invoke<T.ChatSession>('chat_get_session', { sessionId }),
    deleteSession: (sessionId: string) => invoke<void>('chat_delete_session', { sessionId }),
    renameSession: (sessionId: string, title: string) => invoke<void>('chat_rename_session', { sessionId, title }),
    duplicateSession: (sessionId: string) => invoke<T.ChatSession>('chat_duplicate_session', { sessionId }),
    pinSession: (sessionId: string, pinned: boolean) => invoke<void>('chat_pin_session', { sessionId, pinned }),
    archiveSession: (sessionId: string, archived: boolean) => invoke<void>('chat_archive_session', { sessionId, archived }),
    exportSession: (sessionId: string, format: 'markdown' | 'json') => invoke<string>('chat_export_session', { sessionId, format }),
    send: (req: T.SendMessageRequest) => invoke<void>('chat_send', { req }),
    stop: (sessionId: string) => invoke<void>('chat_stop', { sessionId }),
    approve: (approvalId: string, option: T.ApprovalOption) => invoke<void>('chat_approve', { approvalId, option }),
    answerQuestion: (questionId: string, answer: string) => invoke<void>('chat_answer_question', { questionId, answer }),
    queueRemove: (sessionId: string, queuedId: string) => invoke<void>('chat_queue_remove', { sessionId, queuedId }),
    queueSendNow: (sessionId: string, queuedId: string) => invoke<void>('chat_queue_send_now', { sessionId, queuedId }),
    acceptChanges: (sessionId: string, changeIds: string[] | 'all') => invoke<void>('chat_accept_changes', { sessionId, changeIds: changeIds === 'all' ? null : changeIds }),
    rejectChanges: (sessionId: string, changeIds: string[] | 'all') => invoke<void>('chat_reject_changes', { sessionId, changeIds: changeIds === 'all' ? null : changeIds }),
    pendingChanges: (sessionId: string) => invoke<T.PendingChange[]>('chat_pending_changes', { sessionId }),
    changeDiff: (changeId: string) => invoke<T.PendingChange>('chat_change_diff', { changeId }),
    checkpoints: (sessionId: string) => invoke<T.Checkpoint[]>('chat_checkpoints', { sessionId }),
    restoreCheckpoint: (sessionId: string, checkpointId: string) => invoke<string[]>('chat_restore_checkpoint', { sessionId, checkpointId }),
    setTodos: (sessionId: string, todos: T.TodoItem[]) => invoke<void>('chat_set_todos', { sessionId, todos }),
    summarize: (sessionId: string) => invoke<void>('chat_summarize', { sessionId }),
    setMode: (sessionId: string, mode: T.ModeId) => invoke<void>('chat_set_mode', { sessionId, mode }),
    setModel: (sessionId: string, modelId: string) => invoke<void>('chat_set_model', { sessionId, modelId }),
    regenerate: (sessionId: string, messageId: string) => invoke<void>('chat_regenerate', { sessionId, messageId }),
    editMessage: (sessionId: string, messageId: string, text: string) => invoke<void>('chat_edit_message', { sessionId, messageId, text }),
    modes: () => invoke<{ id: string; name: string; description: string; icon?: string; builtin: boolean; tools: string[] | 'all' | 'readonly' }[]>('chat_list_modes'),
    contextPreview: (items: T.ContextItem[]) => invoke<{ totalTokens: number; items: { id: string; tokens: number; truncated: boolean }[] }>('chat_context_preview', { items }),
    mentionCandidates: (kind: T.ContextItemKind, query: string, limit = 30) => invoke<T.ContextItem[]>('chat_mention_candidates', { kind, query, limit }),
    toolCatalog: () => invoke<{ name: string; displayName: string; description: string; group: string; readonly: boolean }[]>('chat_tool_catalog'),
  },

  ai: {
    complete: (req: T.CompletionRequest) => invoke<T.CompletionResult>('ai_complete', { req }),
    nextEdit: (req: T.NextEditRequest) => invoke<T.NextEditResult>('ai_next_edit', { req }),
    inlineEdit: (req: T.InlineEditRequest, onEvent: (e: T.InlineEditEvent) => void) => invoke<void>('ai_inline_edit', { req, onEvent: channel(onEvent) }),
    cancel: (requestId: string) => invoke<void>('ai_cancel', { requestId }),
    terminalCommand: (req: T.TerminalCommandRequest) => invoke<T.TerminalCommandResult>('ai_terminal_command', { req }),
    apply: (req: T.ApplyRequest) => invoke<T.ApplyResult>('ai_apply', { req }),
    commitMessage: (req: T.CommitMessageRequest) => invoke<T.CommitMessageResult>('ai_commit_message', { req }),
    review: (req: T.ReviewRequest, onEvent: (e: T.ReviewStreamEvent) => void) => invoke<string>('ai_review', { req, onEvent: channel(onEvent) }),
    reviews: () => invoke<T.ReviewResult[]>('ai_review_list'),
    dismissReview: (reviewId: string) => invoke<void>('ai_review_dismiss', { reviewId }),
    dismissFinding: (reviewId: string, findingId: string) => invoke<void>('ai_review_dismiss_finding', { reviewId, findingId }),
  },

  rules: {
    list: () => invoke<T.RuleInfo[]>('rules_list'),
    read: (ruleId: string) => invoke<string>('rules_read', { ruleId }),
    create: (opts: { name: string; kind: T.RuleKind; globs?: string[]; description?: string; content: string; scope: 'project' | 'user' }) => invoke<T.RuleInfo>('rules_create', { ...opts }),
    toggle: (ruleId: string, enabled: boolean) => invoke<void>('rules_toggle', { ruleId, enabled }),
    delete: (ruleId: string) => invoke<void>('rules_delete', { ruleId }),
  },
  memories: {
    list: () => invoke<T.MemoryItem[]>('memories_list'),
    add: (text: string, title?: string) => invoke<T.MemoryItem>('memories_add', { text, title }),
    update: (memoryId: string, text: string) => invoke<void>('memories_update', { memoryId, text }),
    delete: (memoryId: string) => invoke<void>('memories_delete', { memoryId }),
    toggle: (memoryId: string, enabled: boolean) => invoke<void>('memories_toggle', { memoryId, enabled }),
  },
  skills: { list: () => invoke<T.SkillInfo[]>('skills_list'), read: (skillId: string) => invoke<string>('skills_read', { skillId }) },
  commands: { list: () => invoke<T.SlashCommandInfo[]>('commands_list') },
  subagents: { list: () => invoke<T.SubagentInfo[]>('subagents_list') },
  cli: {
    takePendingOpens: () => invoke<T.OpenRequest[]>('cli_take_pending_opens'),
    status: () => invoke<T.CliStatus>('cli_status'),
    install: () => invoke<T.CliStatus>('cli_install'),
    uninstall: () => invoke<T.CliStatus>('cli_uninstall'),
  },
  hooks: { list: () => invoke<T.HookInfo[]>('hooks_list') },
  plans: {
    list: () => invoke<T.PlanInfo[]>('plans_list'),
    read: (path: string) => invoke<string>('plans_read', { path }),
    setStatus: (path: string, status: T.PlanInfo['status']) => invoke<void>('plans_set_status', { path, status }),
    build: (path: string, opts?: { sessionId?: string; modelId?: string; useWorktree?: boolean }) => invoke<string>('plans_build', { path, ...opts }),
  },

  index: {
    status: () => invoke<T.IndexStatus>('index_status'),
    start: () => invoke<void>('index_start'),
    pause: () => invoke<void>('index_pause'),
    resume: () => invoke<void>('index_resume'),
    rebuild: () => invoke<void>('index_rebuild'),
    clear: () => invoke<void>('index_clear'),
    search: (query: string, opts?: { limit?: number; pathPrefix?: string }) => invoke<T.CodebaseSearchHit[]>('index_search', { query, ...opts }),
  },
  docs: {
    list: () => invoke<T.DocSource[]>('docs_list'),
    add: (url: string, name?: string, opts?: { maxPages?: number; prefix?: string }) => invoke<T.DocSource>('docs_add', { url, name, ...opts }),
    remove: (docId: string) => invoke<void>('docs_remove', { docId }),
    refresh: (docId: string) => invoke<void>('docs_refresh', { docId }),
    search: (query: string, docIds?: string[], limit = 10) => invoke<T.DocSearchHit[]>('docs_search', { query, docIds, limit }),
  },

  mcp: {
    servers: () => invoke<T.McpServerStatus[]>('mcp_list_servers'),
    configs: () => invoke<T.McpServerConfig[]>('mcp_list_configs'),
    add: (config: T.McpServerConfig, scope: 'project' | 'user') => invoke<void>('mcp_add_server', { config, scope }),
    update: (config: T.McpServerConfig) => invoke<void>('mcp_update_server', { config }),
    remove: (serverId: string) => invoke<void>('mcp_remove_server', { serverId }),
    toggle: (serverId: string, enabled: boolean) => invoke<void>('mcp_toggle_server', { serverId, enabled }),
    restart: (serverId: string) => invoke<void>('mcp_restart_server', { serverId }),
    tools: () => invoke<T.McpToolInfo[]>('mcp_tools'),
    toggleTool: (serverId: string, tool: string, enabled: boolean) => invoke<void>('mcp_toggle_tool', { serverId, tool, enabled }),
    resources: (serverId?: string) => invoke<T.McpResourceInfo[]>('mcp_resources', { serverId }),
    readResource: (serverId: string, uri: string) => invoke<T.McpToolResult>('mcp_read_resource', { serverId, uri }),
    prompts: () => invoke<T.McpPromptInfo[]>('mcp_prompts'),
    getPrompt: (serverId: string, name: string, args: Record<string, string>) => invoke<{ role: string; text: string }[]>('mcp_get_prompt', { serverId, name, args }),
    callTool: (serverId: string, name: string, args: unknown) => invoke<T.McpToolResult>('mcp_call_tool', { serverId, name, args }),
    logs: (serverId: string) => invoke<string[]>('mcp_logs', { serverId }),
  },

  browser: {
    open: (url: string, label?: string) => invoke<T.BrowserTab>('browser_open', { url, label }),
    list: () => invoke<T.BrowserTab[]>('browser_list'),
    navigate: (tabId: string, url: string) => invoke<void>('browser_navigate', { tabId, url }),
    snapshot: (tabId: string) => invoke<T.BrowserSnapshot>('browser_snapshot', { tabId }),
    click: (tabId: string, ref: string) => invoke<void>('browser_click', { tabId, ref }),
    type: (tabId: string, ref: string, text: string, submit = false) => invoke<void>('browser_type', { tabId, ref, text, submit }),
    eval: (tabId: string, js: string) => invoke<string>('browser_eval', { tabId, js }),
    back: (tabId: string) => invoke<void>('browser_back', { tabId }),
    forward: (tabId: string) => invoke<void>('browser_forward', { tabId }),
    close: (tabId: string) => invoke<void>('browser_close', { tabId }),
  },
}

export type { Channel }

// ═══════════════════════════ Wave 1 additions ═══════════════════════════
export const apiWave1 = {
  verify: {
    run: (scope: T.VerifyScope, onEvent: (e: T.VerifyEvent) => void) => invoke<string>('verify_run', { scope, onEvent: channel(onEvent) }),
    get: (reportId: string) => invoke<T.VerificationReport>('verify_get', { reportId }),
    list: (sessionId?: string, limit = 20) => invoke<T.VerificationReport[]>('verify_list', { sessionId, limit }),
    cancel: (reportId: string) => invoke<void>('verify_cancel', { reportId }),
    latestForSession: (sessionId: string) => invoke<T.VerificationReport | null>('verify_latest', { sessionId }),
    applyFix: (reportId: string, findingId: string) => invoke<T.PendingChange | null>('verify_apply_fix', { reportId, findingId }),
  },
  sandbox: {
    status: () => invoke<T.SandboxStatus>('sandbox_status'),
    classify: (command: string, cwd?: string) => invoke<T.CommandClassification>('sandbox_classify', { command, cwd }),
    mcpToolChanges: () => invoke<T.McpToolChange[]>('mcp_tool_changes'),
    acknowledgeMcpTool: (serverId: string, tool: string) => invoke<void>('mcp_acknowledge_tool', { serverId, tool }),
  },
  pty: {
    blocks: (id: string) => invoke<T.TerminalBlock[]>('pty_blocks', { id }),
    blockOutput: (id: string, blockId: string, maxChars = 20000) => invoke<string>('pty_block_output', { id, blockId, maxChars }),
    status: (id: string) => invoke<{ status: T.ProgramStatus; title?: string; detail?: string } | null>('pty_status', { id }),
  },
  acp: {
    agents: () => invoke<T.AcpAgentStatus[]>('acp_list_agents'),
    configs: () => invoke<T.AcpAgentConfig[]>('acp_list_configs'),
    add: (config: T.AcpAgentConfig) => invoke<T.AcpAgentConfig[]>('acp_add_agent', { config }),
    remove: (agentId: string) => invoke<T.AcpAgentConfig[]>('acp_remove_agent', { agentId }),
    toggle: (agentId: string, enabled: boolean) => invoke<T.AcpAgentConfig[]>('acp_toggle_agent', { agentId, enabled }),
    probe: (agentId: string) => invoke<T.AcpAgentStatus>('acp_probe', { agentId }),
    authenticate: (agentId: string, methodId: string) => invoke<T.AcpAgentStatus>('acp_authenticate', { agentId, methodId }),
    setMode: (sessionId: string, modeId: string) => invoke<T.AcpAgentStatus>('acp_set_mode', { sessionId, modeId }),
    /** Change a session config option (model, effort, …); remembered per agent for future sessions. */
    setConfigOption: (sessionId: string, configId: string, value: unknown) => invoke<T.AcpAgentStatus>('acp_set_config_option', { sessionId, configId, value }),
    /** Connect the agent and create its ACP session now so modes/options are available before the first message. */
    prepareSession: (sessionId: string, agentId: string) => invoke<T.AcpAgentStatus>('acp_prepare_session', { sessionId, agentId }),
    registry: () => invoke<T.AcpRegistryEntry[]>('acp_registry'),
    install: (entryId: string) => invoke<T.AcpRegistryEntry>('acp_install', { entryId }),
    logs: (agentId: string) => invoke<string[]>('acp_logs', { agentId }),
    sessionInfo: (sessionId: string) => invoke<{ agentId: string; acpSessionId: string; modes?: { currentModeId: string; availableModes: { id: string; name: string; description?: string }[] } | null; configOptions?: T.AcpConfigOption[] | null; commands: { name: string; description?: string }[] } | null>('acp_session_info', { sessionId }),
  },
  /** Auto routing (managed Lynkr sidecar). */
  router: {
    status: () => invoke<T.RouterStatus>('router_status'),
    install: () => invoke<T.RouterStatus>('router_install'),
    restart: () => invoke<T.RouterStatus>('router_restart'),
    stop: () => invoke<T.RouterStatus>('router_stop'),
    logs: () => invoke<string[]>('router_logs'),
    previewAssignment: () => invoke<T.RouterAssignment>('router_preview_assignment'),
    setOverride: (tier: T.RouterTier, modelId: string | null) => invoke<T.RouterStatus>('router_set_override', { tier, modelId }),
  },
  /** Ports panel: local listening TCP servers. */
  ports: {
    list: () => invoke<T.ListeningPort[]>('ports_list'),
    kill: (pid: number) => invoke<void>('ports_kill', { pid }),
  },
  /** Run and Debug (Debug Adapter Protocol). Ids for threads/frames/variables are opaque. */
  debug: {
    adapters: () => invoke<T.DebugAdapterStatus[]>('debug_adapters'),
    installAdapter: (id: string) => invoke<T.DebugAdapterStatus>('debug_install_adapter', { id }),
    listConfigs: (file?: string) => invoke<T.LaunchConfig[]>('debug_list_configs', { file }),
    start: (config: T.LaunchConfig) => invoke<T.DebugSessionInfo>('debug_start', { config }),
    sessions: () => invoke<T.DebugSessionInfo[]>('debug_sessions'),
    stop: (sessionId: string) => invoke<void>('debug_stop', { sessionId }),
    continue: (sessionId: string, threadId: number) => invoke<void>('debug_continue', { sessionId, threadId }),
    next: (sessionId: string, threadId: number) => invoke<void>('debug_next', { sessionId, threadId }),
    stepIn: (sessionId: string, threadId: number) => invoke<void>('debug_step_in', { sessionId, threadId }),
    stepOut: (sessionId: string, threadId: number) => invoke<void>('debug_step_out', { sessionId, threadId }),
    pause: (sessionId: string, threadId: number) => invoke<void>('debug_pause', { sessionId, threadId }),
    restart: (sessionId: string) => invoke<void>('debug_restart', { sessionId }),
    threads: (sessionId: string) => invoke<T.DapThread[]>('debug_threads', { sessionId }),
    stackTrace: (sessionId: string, threadId: number) => invoke<T.DapStackFrame[]>('debug_stack_trace', { sessionId, threadId }),
    scopes: (sessionId: string, frameId: number) => invoke<T.DapScope[]>('debug_scopes', { sessionId, frameId }),
    variables: (sessionId: string, variablesReference: number) => invoke<T.DapVariable[]>('debug_variables', { sessionId, variablesReference }),
    evaluate: (sessionId: string, expression: string, frameId?: number, context = 'repl') => invoke<T.DapEvaluateResult>('debug_evaluate', { sessionId, expression, frameId, context }),
    setBreakpoints: (file: string, lines: number[]) => invoke<T.DebugBreakpoint[]>('debug_set_breakpoints', { file, lines }),
    breakpoints: () => invoke<[string, number[]][]>('debug_breakpoints'),
    logs: () => invoke<string[]>('debug_logs'),
  },
  review: {
    comments: (sessionId: string) => invoke<T.ReviewComment[]>('chat_review_comments', { sessionId }),
    addComment: (sessionId: string, comment: Omit<T.ReviewComment, 'id' | 'createdAt' | 'author'>) => invoke<T.ReviewComment>('chat_add_review_comment', { sessionId, comment }),
    resolveComment: (sessionId: string, commentId: string, resolved = true) => invoke<void>('chat_resolve_review_comment', { sessionId, commentId, resolved }),
    deleteComment: (sessionId: string, commentId: string) => invoke<void>('chat_delete_review_comment', { sessionId, commentId }),
    /** Send all unresolved comments to the agent as a structured follow-up turn. */
    sendComments: (sessionId: string, text?: string) => invoke<void>('chat_send_review_comments', { sessionId, text }),
    groups: (sessionId: string) => invoke<T.ReviewGroup[]>('chat_review_groups', { sessionId }),
  },
}
export type ApiWave1 = typeof apiWave1
/** Full command surface: base domains plus wave-1 additions. */
export const api = { ...apiBase, ...apiWave1, pty: { ...apiBase.pty, ...apiWave1.pty } }
export type Api = typeof api

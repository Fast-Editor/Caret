/**
 * Settings field registry: drives the Settings UI forms, search, and "modified vs default" detection.
 * DEFAULTS mirror src-tauri/src/core/settings.rs — keep them in sync.
 */
import type { Settings, DeepPartial } from '@/types'

export type SettingsPageId =
  | 'general' | 'models' | 'tab' | 'agent' | 'agents' | 'rules' | 'memories' | 'indexing' | 'mcp' | 'hooks' | 'appearance' | 'beta' | 'keybindings' | 'about'

export interface SettingsPageMeta { id: SettingsPageId; title: string; icon: string; description: string }

export const SETTINGS_PAGES: SettingsPageMeta[] = [
  { id: 'general', title: 'General', icon: 'settings', description: 'Startup, workspace, git and privacy' },
  { id: 'models', title: 'Models', icon: 'brain', description: 'Providers, API keys and model catalog' },
  { id: 'tab', title: 'Tab', icon: 'zap', description: 'Inline completions and next-edit prediction' },
  { id: 'agent', title: 'Agent', icon: 'bot', description: 'Modes, auto-run, protections and tools' },
  { id: 'rules', title: 'Rules', icon: 'file-text', description: 'User and project rules for the AI' },
  { id: 'memories', title: 'Memories', icon: 'database', description: 'Facts the agent remembers per project' },
  { id: 'indexing', title: 'Indexing & Docs', icon: 'layers', description: 'Codebase index, embeddings and docs' },
  { id: 'agents', title: 'Agents', icon: 'bot', description: 'External coding agents (Claude Code, Codex, Gemini…) via the Agent Client Protocol' },
  { id: 'mcp', title: 'MCP', icon: 'server', description: 'Model Context Protocol servers and tools' },
  { id: 'hooks', title: 'Hooks & Automation', icon: 'webhook', description: 'Hooks, slash commands, skills, subagents, plans' },
  { id: 'appearance', title: 'Appearance & Editor', icon: 'palette', description: 'Theme, fonts and editor behaviour' },
  { id: 'beta', title: 'Beta', icon: 'flask', description: 'Experimental features' },
  { id: 'keybindings', title: 'Keybindings', icon: 'keyboard', description: 'Keyboard shortcuts' },
  { id: 'about', title: 'About', icon: 'info', description: 'Version, diagnostics and config folder' },
]

export type FieldType = 'boolean' | 'number' | 'string' | 'select' | 'stringList' | 'textarea' | 'model' | 'provider' | 'theme' | 'keyValue'

export interface SelectOption { value: string; label: string; description?: string }

export interface SettingField {
  /** Dot path inside Settings, e.g. `ui.fontSize`. */
  id: string
  title: string
  description?: string
  page: SettingsPageId
  type: FieldType
  options?: SelectOption[]
  min?: number
  max?: number
  step?: number
  unit?: string
  placeholder?: string
  keywords?: string[]
  /** Only show when another boolean path is true. */
  dependsOn?: string
  warning?: string
  /** Allow clearing to undefined (e.g. optional model ids). */
  optional?: boolean
  /** Render group heading within the page. */
  group?: string
}

export const DEFAULTS: Settings = {
  providers: [],
  models: [],
  tab: { enabled: true, modelId: undefined, debounceMs: 250, maxLines: 12, partialAccepts: true, nextEditPrediction: true, suggestInComments: true, disabledLanguages: ['plaintext', 'markdown', 'log'], contextLines: 80, cacheSize: 200 },
  agent: {
    defaultMode: 'agent', defaultModelId: undefined, autoRun: 'allowlist',
    commandAllowlist: ['ls', 'cat', 'pwd', 'git status', 'git diff', 'git log', 'rg', 'grep', 'find', 'echo', 'wc', 'head', 'tail', 'npm test', 'npm run', 'cargo check', 'cargo test', 'cargo build', 'pytest', 'go test', 'node', 'python3', 'tsc'],
    commandDenylist: ['rm -rf /', 'sudo', 'shutdown', 'reboot', 'mkfs', 'dd if=', 'git push --force', ':(){'],
    protectDotfiles: true, protectDelete: true, protectOutsideWorkspace: true, autoFixLints: true, autoApplyEdits: true,
    maxIterations: 60, maxToolOutputChars: 30000, webSearchEnabled: true, webSearchProvider: 'duckduckgo', webSearchEndpoint: undefined, webSearchApiKeyRef: undefined,
    subagentModelId: undefined, summarizeThresholdRatio: 0.8, chimeOnFinish: false, thinking: 'auto', effort: 'high', commandTimeoutSecs: 120,
    mcpAutoRun: false, autoRunMcpTools: [], largeContextMode: false, parallelToolCalls: true,
  },
  indexing: { enabled: true, embeddingsProviderId: undefined, embeddingsModel: undefined, maxFileSizeKb: 512, excludeGlobs: ['**/node_modules/**', '**/target/**', '**/dist/**', '**/build/**', '**/.git/**', '**/*.min.js', '**/*.lock', '**/package-lock.json', '**/*.map'], chunkLines: 60, chunkOverlap: 10, autoIndexNewFolders: true, hybridAlpha: 0.5, gitGraphIndexing: false },
  ui: {
    theme: 'caret-dark', fontFamily: "JetBrains Mono, Menlo, Monaco, 'Courier New', monospace", fontSize: 13, lineHeight: 1.6, tabSize: 2, insertSpaces: true,
    wordWrap: 'off', minimap: true, renderWhitespace: 'selection', cursorBlinking: 'smooth', cursorStyle: 'line', bracketPairColorization: true, formatOnSave: false,
    autoSave: 'afterDelay', autoSaveDelayMs: 1000, chatFontSize: 13, sidebarPosition: 'left', chatPosition: 'right', terminalFontFamily: 'JetBrains Mono, Menlo, monospace',
    terminalFontSize: 12, terminalScrollback: 5000, terminalCursorStyle: 'block', terminalCursorBlink: true, showStatusBarMemory: true, layout: 'editor', reduceMotion: false, stickyScroll: true, lineNumbers: 'on', smoothScrolling: true,
    fontLigatures: true, editorTabsMode: 'multiple', previewTabs: true, breadcrumbs: true, zoomLevel: 0, confirmDelete: true, compactFolders: true,
  },
  privacy: { privacyMode: false, allowWebTools: true, allowRemoteEmbeddings: true },
  general: { defaultShell: undefined, openLastWorkspace: true, confirmQuitWithRunningAgents: true, gitAutoFetch: false, gitAutoFetchIntervalSecs: 180, globalIgnore: ['.DS_Store', 'Thumbs.db'], trustedWorkspaces: [], showWelcome: true, useCursorCompatDirs: true },
  userRules: '',
  lsp: { enabled: true, servers: [] },
  mcp: { enabled: true },
  customModes: [],
  beta: { browserTools: true, voice: false, nextEditPrediction: true, backgroundAgents: true, designMode: false },
  keybindings: {},
  verify: { autoRunAfterTurn: true, runTests: true, mutationTesting: true, mutationMaxMutants: 12, dependencyGrounding: true, securityRules: true, projectCheckCommand: undefined, testCommand: undefined, preTestCommand: undefined, timeoutSecs: 300, blockMergeOnFail: false, useSemgrepIfAvailable: true },
  sandbox: { mode: 'auto', allowNetwork: true, extraWritablePaths: [], allowReadHome: true, pinMcpToolDescriptions: true },
  acp: { enabled: true, defaultAgentId: 'caret', agents: [], passMcpServers: true, sessionDefaults: {} },
  terminal: { blocks: true, shellIntegration: true, aiEnabled: true, programStatus: true, maxBlocks: 200 },
}

export const SETTINGS_FIELDS: SettingField[] = [
  // ── General ──
  { id: 'acp.enabled', page: 'agents', type: 'boolean', title: 'Enable external agents', description: 'Run Claude Code, Codex, Gemini CLI, OpenCode and other ACP agents inside Caret chat.', group: 'Agent Client Protocol', keywords: ['acp', 'claude code', 'codex', 'gemini'] },
  { id: 'acp.passMcpServers', page: 'agents', type: 'boolean', title: 'Share MCP servers with agents', description: 'Pass your enabled MCP servers to external agents when a session starts.', group: 'Agent Client Protocol', dependsOn: 'acp.enabled' },
  { id: 'acp.defaultAgentId', page: 'agents', type: 'string', title: 'Default agent for new chats', description: "'caret' for the built-in agent, or an agent id such as claude-code.", group: 'Agent Client Protocol', placeholder: 'caret', dependsOn: 'acp.enabled' },
  { id: 'general.openLastWorkspace', page: 'general', type: 'boolean', title: 'Open last workspace on startup', group: 'Startup' },
  { id: 'general.showWelcome', page: 'general', type: 'boolean', title: 'Show Welcome page', description: 'Show the welcome page when no editors are open.', group: 'Startup' },
  { id: 'general.confirmQuitWithRunningAgents', page: 'general', type: 'boolean', title: 'Confirm quit while agents are running', group: 'Startup' },
  { id: 'general.defaultShell', page: 'general', type: 'string', title: 'Default shell', description: 'Path to the shell used by new terminals. Empty uses your login shell.', placeholder: '/bin/zsh', optional: true, group: 'Terminal' },
  { id: 'general.gitAutoFetch', page: 'general', type: 'boolean', title: 'Git auto fetch', description: 'Periodically run git fetch in the background.', group: 'Git' },
  { id: 'general.gitAutoFetchIntervalSecs', page: 'general', type: 'number', title: 'Auto fetch interval', unit: 's', min: 30, max: 3600, step: 30, dependsOn: 'general.gitAutoFetch', group: 'Git' },
  { id: 'general.globalIgnore', page: 'general', type: 'stringList', title: 'Global ignore patterns', description: 'Files hidden from the explorer, search and the agent everywhere.', group: 'Files' },
  { id: 'general.useCursorCompatDirs', page: 'general', type: 'boolean', title: 'Read .cursor compatibility directories', description: 'Also load rules, MCP servers, hooks, commands, skills and agents from .cursor/ and .cursorrules.', group: 'Files', keywords: ['cursor', 'compat'] },
  { id: 'privacy.privacyMode', page: 'general', type: 'boolean', title: 'Privacy mode', description: 'Never send code to anything other than your configured model provider: disables web tools and remote embeddings.', group: 'Privacy', keywords: ['telemetry'] },
  { id: 'privacy.allowWebTools', page: 'general', type: 'boolean', title: 'Allow web tools', description: 'Let the agent search the web and fetch URLs.', group: 'Privacy' },
  { id: 'privacy.allowRemoteEmbeddings', page: 'general', type: 'boolean', title: 'Allow remote embeddings', description: 'Send code chunks to the embeddings provider for semantic search. Disable to use local BM25 search only.', group: 'Privacy' },

  // ── Tab ──
  { id: 'tab.enabled', page: 'tab', type: 'boolean', title: 'Enable Tab completions', description: 'Show inline ghost-text suggestions while typing. Press Tab to accept.', keywords: ['autocomplete', 'copilot', 'ghost'] },
  { id: 'tab.modelId', page: 'tab', type: 'model', title: 'Tab model', description: 'A fast model is recommended. Empty uses the default model.', optional: true },
  { id: 'tab.debounceMs', page: 'tab', type: 'number', title: 'Debounce', description: 'Wait this long after you stop typing before requesting a suggestion.', unit: 'ms', min: 50, max: 2000, step: 50 },
  { id: 'tab.maxLines', page: 'tab', type: 'number', title: 'Max suggestion lines', min: 1, max: 60 },
  { id: 'tab.partialAccepts', page: 'tab', type: 'boolean', title: 'Partial accepts', description: 'Accept one word at a time with ⌘→.' },
  { id: 'tab.nextEditPrediction', page: 'tab', type: 'boolean', title: 'Next-edit prediction', description: 'After an accepted edit, predict the next edit location and offer Tab to jump.' },
  { id: 'tab.suggestInComments', page: 'tab', type: 'boolean', title: 'Suggest in comments' },
  { id: 'tab.disabledLanguages', page: 'tab', type: 'stringList', title: 'Disabled languages', description: 'Language ids where Tab stays off.' },
  { id: 'tab.contextLines', page: 'tab', type: 'number', title: 'Context lines', description: 'Lines of prefix/suffix sent with each request.', min: 10, max: 400, step: 10 },
  { id: 'tab.cacheSize', page: 'tab', type: 'number', title: 'Suggestion cache size', min: 0, max: 2000, step: 50 },

  // ── Agent ──
  { id: 'agent.defaultMode', page: 'agent', type: 'select', title: 'Default mode', options: [{ value: 'agent', label: 'Agent' }, { value: 'ask', label: 'Ask' }, { value: 'plan', label: 'Plan' }, { value: 'debug', label: 'Debug' }], group: 'Defaults' },
  { id: 'agent.defaultModelId', page: 'agent', type: 'model', title: 'Default model', description: 'Used for new chats and by "Auto".', optional: true, group: 'Defaults' },
  { id: 'agent.subagentModelId', page: 'agent', type: 'model', title: 'Subagent model', description: 'Model for `task` subagents. Empty uses the session model.', optional: true, group: 'Defaults' },
  { id: 'agent.thinking', page: 'agent', type: 'select', title: 'Thinking', options: [{ value: 'auto', label: 'Auto' }, { value: 'on', label: 'On' }, { value: 'off', label: 'Off' }], group: 'Defaults', keywords: ['reasoning'] },
  { id: 'agent.effort', page: 'agent', type: 'select', title: 'Effort', description: 'Reasoning depth for models that support effort.', options: ['low', 'medium', 'high', 'xhigh', 'max'].map((v) => ({ value: v, label: v })), group: 'Defaults' },
  { id: 'agent.autoRun', page: 'agent', type: 'select', title: 'Auto-run commands', description: 'How terminal commands proposed by the agent are approved.', options: [{ value: 'off', label: 'Off — always ask' }, { value: 'allowlist', label: 'Allowlist — run matching commands automatically' }, { value: 'all', label: 'All — run every command (YOLO)' }], warning: 'Running every command without review can damage your system. Use allowlists where possible.', group: 'Auto-run', keywords: ['yolo'] },
  { id: 'agent.commandAllowlist', page: 'agent', type: 'stringList', title: 'Command allowlist', description: 'Prefixes that run without approval when auto-run is set to Allowlist.', group: 'Auto-run' },
  { id: 'agent.commandDenylist', page: 'agent', type: 'stringList', title: 'Command denylist', description: 'Commands containing these fragments always require explicit approval.', group: 'Auto-run' },
  { id: 'agent.commandTimeoutSecs', page: 'agent', type: 'number', title: 'Command timeout', unit: 's', min: 5, max: 3600, step: 5, group: 'Auto-run' },
  { id: 'agent.mcpAutoRun', page: 'agent', type: 'boolean', title: 'Auto-run MCP tools', description: 'Run MCP tool calls without asking.', group: 'Auto-run' },
  { id: 'agent.autoRunMcpTools', page: 'agent', type: 'stringList', title: 'Always-allowed MCP tools', description: 'Qualified tool names (server/tool) that never prompt.', group: 'Auto-run' },
  { id: 'agent.protectDotfiles', page: 'agent', type: 'boolean', title: 'Protect dotfiles', description: 'Ask before the agent edits files or folders that start with a dot.', group: 'Protections' },
  { id: 'agent.protectDelete', page: 'agent', type: 'boolean', title: 'File-delete protection', description: 'Ask before the agent deletes files.', group: 'Protections' },
  { id: 'agent.protectOutsideWorkspace', page: 'agent', type: 'boolean', title: 'Protect outside workspace', description: 'Ask before reading or writing paths outside the open folder.', group: 'Protections' },
  { id: 'sandbox.mode', page: 'agent', type: 'select', title: 'Sandbox mode', description: 'Run agent commands inside an OS sandbox (macOS Seatbelt, Linux bubblewrap): writes limited to the workspace, temp and package caches; credential folders unreadable.', options: [{ value: 'off', label: 'Off — run commands directly' }, { value: 'auto', label: 'Auto — sandbox when available' }, { value: 'strict', label: 'Strict — refuse to run without a sandbox' }], group: 'Sandbox', keywords: ['seatbelt', 'bwrap', 'isolation', 'security'] },
  { id: 'sandbox.allowNetwork', page: 'agent', type: 'boolean', title: 'Allow network in sandbox', description: 'Off cuts sandboxed commands from the network entirely (package installs will fail until re-enabled).', group: 'Sandbox' },
  { id: 'sandbox.allowReadHome', page: 'agent', type: 'boolean', title: 'Allow reading home directory', description: 'When off, sandboxed commands can only read the workspace, toolchains and caches under your home folder. Credential folders (~/.ssh, ~/.aws, …) are never readable either way.', group: 'Sandbox' },
  { id: 'sandbox.extraWritablePaths', page: 'agent', type: 'stringList', title: 'Extra writable paths', description: 'Additional folders sandboxed commands may write to (absolute or ~/ paths).', group: 'Sandbox', placeholder: '~/projects/shared-cache' },
  { id: 'sandbox.pinMcpToolDescriptions', page: 'agent', type: 'boolean', title: 'Pin MCP tool descriptions', description: 'Remember each MCP tool’s description and schema on first use; if a server changes them later (a common prompt-injection vector), the tool needs a fresh approval.', group: 'Sandbox', keywords: ['mcp', 'rug pull', 'prompt injection'] },
  { id: 'agent.autoApplyEdits', page: 'agent', type: 'boolean', title: 'Auto-apply edits', description: 'Write agent edits to disk immediately; you review and accept/reject afterwards.', group: 'Behaviour' },
  { id: 'agent.autoFixLints', page: 'agent', type: 'boolean', title: 'Iterate on lints', description: 'Feed new diagnostics back to the agent after edits so it can fix them.', group: 'Behaviour' },
  { id: 'agent.parallelToolCalls', page: 'agent', type: 'boolean', title: 'Parallel tool calls', group: 'Behaviour' },
  { id: 'agent.maxIterations', page: 'agent', type: 'number', title: 'Max tool iterations per turn', min: 1, max: 500, group: 'Behaviour' },
  { id: 'agent.maxToolOutputChars', page: 'agent', type: 'number', title: 'Max tool output', unit: 'chars', min: 2000, max: 200000, step: 1000, group: 'Behaviour' },
  { id: 'agent.summarizeThresholdRatio', page: 'agent', type: 'number', title: 'Summarize at context usage', description: 'Fraction of the context window after which older turns are summarized.', min: 0.3, max: 0.95, step: 0.05, group: 'Behaviour' },
  { id: 'agent.largeContextMode', page: 'agent', type: 'boolean', title: 'Large context mode', description: 'Include more file content per request (higher cost).', group: 'Behaviour' },
  { id: 'agent.chimeOnFinish', page: 'agent', type: 'boolean', title: 'Play a chime when the agent finishes', group: 'Behaviour' },
  { id: 'agent.webSearchEnabled', page: 'agent', type: 'boolean', title: 'Enable web search tool', group: 'Web' },
  { id: 'agent.webSearchProvider', page: 'agent', type: 'select', title: 'Web search provider', options: [{ value: 'duckduckgo', label: 'DuckDuckGo (no key)' }, { value: 'searxng', label: 'SearXNG (self-hosted)' }, { value: 'brave', label: 'Brave Search API' }, { value: 'tavily', label: 'Tavily' }], dependsOn: 'agent.webSearchEnabled', group: 'Web' },
  { id: 'agent.webSearchEndpoint', page: 'agent', type: 'string', title: 'Search endpoint', description: 'SearXNG base URL.', placeholder: 'http://localhost:8080', optional: true, dependsOn: 'agent.webSearchEnabled', group: 'Web' },

  // ── Indexing ──
  { id: 'indexing.enabled', page: 'indexing', type: 'boolean', title: 'Index codebase', description: 'Build a local search index for @Codebase and the codebase_search tool.' },
  { id: 'indexing.embeddingsProviderId', page: 'indexing', type: 'provider', title: 'Embeddings provider', description: 'OpenAI-compatible provider used for /v1/embeddings. Empty disables semantic search (BM25 still works).', optional: true },
  { id: 'indexing.embeddingsModel', page: 'indexing', type: 'string', title: 'Embeddings model', placeholder: 'text-embedding-3-small · nomic-embed-text', optional: true },
  { id: 'indexing.maxFileSizeKb', page: 'indexing', type: 'number', title: 'Max file size', unit: 'KB', min: 16, max: 10240, step: 16 },
  { id: 'indexing.excludeGlobs', page: 'indexing', type: 'stringList', title: 'Exclude globs' },
  { id: 'indexing.chunkLines', page: 'indexing', type: 'number', title: 'Chunk size', unit: 'lines', min: 10, max: 400 },
  { id: 'indexing.chunkOverlap', page: 'indexing', type: 'number', title: 'Chunk overlap', unit: 'lines', min: 0, max: 100 },
  { id: 'indexing.hybridAlpha', page: 'indexing', type: 'number', title: 'Hybrid weight', description: '0 = keyword only, 1 = vector only.', min: 0, max: 1, step: 0.05 },
  { id: 'indexing.autoIndexNewFolders', page: 'indexing', type: 'boolean', title: 'Index new folders automatically' },
  { id: 'indexing.gitGraphIndexing', page: 'indexing', type: 'boolean', title: 'Git graph indexing', description: 'Index commit history for @Git mentions.' },

  // ── Appearance ──
  { id: 'ui.theme', page: 'appearance', type: 'theme', title: 'Theme', group: 'Appearance' },
  { id: 'ui.zoomLevel', page: 'appearance', type: 'number', title: 'Zoom level', min: -3, max: 5, step: 0.5, group: 'Appearance' },
  { id: 'ui.reduceMotion', page: 'appearance', type: 'boolean', title: 'Reduce motion', group: 'Appearance' },
  { id: 'ui.layout', page: 'appearance', type: 'select', title: 'Default layout', options: [{ value: 'editor', label: 'Editor' }, { value: 'agents', label: 'Agents' }], group: 'Appearance' },
  { id: 'ui.sidebarPosition', page: 'appearance', type: 'select', title: 'Sidebar position', options: [{ value: 'left', label: 'Left' }, { value: 'right', label: 'Right' }], group: 'Appearance' },
  { id: 'ui.chatPosition', page: 'appearance', type: 'select', title: 'AI pane position', options: [{ value: 'right', label: 'Right' }, { value: 'left', label: 'Left' }], group: 'Appearance' },
  { id: 'ui.chatFontSize', page: 'appearance', type: 'number', title: 'Chat font size', unit: 'px', min: 10, max: 24, group: 'Appearance' },
  { id: 'ui.showStatusBarMemory', page: 'appearance', type: 'boolean', title: 'Show memory usage in status bar', group: 'Appearance' },
  { id: 'ui.fontFamily', page: 'appearance', type: 'string', title: 'Font family', group: 'Editor' },
  { id: 'ui.fontSize', page: 'appearance', type: 'number', title: 'Font size', unit: 'px', min: 8, max: 40, group: 'Editor' },
  { id: 'ui.lineHeight', page: 'appearance', type: 'number', title: 'Line height', min: 1, max: 3, step: 0.1, group: 'Editor' },
  { id: 'ui.fontLigatures', page: 'appearance', type: 'boolean', title: 'Font ligatures', group: 'Editor' },
  { id: 'ui.tabSize', page: 'appearance', type: 'number', title: 'Tab size', min: 1, max: 8, group: 'Editor' },
  { id: 'ui.insertSpaces', page: 'appearance', type: 'boolean', title: 'Insert spaces', group: 'Editor' },
  { id: 'ui.wordWrap', page: 'appearance', type: 'select', title: 'Word wrap', options: [{ value: 'off', label: 'Off' }, { value: 'on', label: 'On' }, { value: 'bounded', label: 'Bounded' }], group: 'Editor' },
  { id: 'ui.lineNumbers', page: 'appearance', type: 'select', title: 'Line numbers', options: [{ value: 'on', label: 'On' }, { value: 'off', label: 'Off' }, { value: 'relative', label: 'Relative' }], group: 'Editor' },
  { id: 'ui.minimap', page: 'appearance', type: 'boolean', title: 'Minimap', group: 'Editor' },
  { id: 'ui.renderWhitespace', page: 'appearance', type: 'select', title: 'Render whitespace', options: ['none', 'boundary', 'selection', 'all'].map((v) => ({ value: v, label: v })), group: 'Editor' },
  { id: 'ui.cursorStyle', page: 'appearance', type: 'select', title: 'Cursor style', options: ['line', 'block', 'underline'].map((v) => ({ value: v, label: v })), group: 'Editor' },
  { id: 'ui.cursorBlinking', page: 'appearance', type: 'select', title: 'Cursor blinking', options: ['blink', 'smooth', 'phase', 'expand', 'solid'].map((v) => ({ value: v, label: v })), group: 'Editor' },
  { id: 'ui.bracketPairColorization', page: 'appearance', type: 'boolean', title: 'Bracket pair colorization', group: 'Editor' },
  { id: 'ui.stickyScroll', page: 'appearance', type: 'boolean', title: 'Sticky scroll', group: 'Editor' },
  { id: 'ui.smoothScrolling', page: 'appearance', type: 'boolean', title: 'Smooth scrolling', group: 'Editor' },
  { id: 'ui.breadcrumbs', page: 'appearance', type: 'boolean', title: 'Breadcrumbs', group: 'Editor' },
  { id: 'ui.formatOnSave', page: 'appearance', type: 'boolean', title: 'Format on save', group: 'Files' },
  { id: 'ui.autoSave', page: 'appearance', type: 'select', title: 'Auto save', options: [{ value: 'off', label: 'Off' }, { value: 'afterDelay', label: 'After delay' }, { value: 'onFocusChange', label: 'On focus change' }], group: 'Files' },
  { id: 'ui.autoSaveDelayMs', page: 'appearance', type: 'number', title: 'Auto save delay', unit: 'ms', min: 100, max: 60000, step: 100, group: 'Files' },
  { id: 'ui.editorTabsMode', page: 'appearance', type: 'select', title: 'Editor tabs', options: [{ value: 'multiple', label: 'Multiple' }, { value: 'single', label: 'Single' }], group: 'Workbench' },
  { id: 'ui.previewTabs', page: 'appearance', type: 'boolean', title: 'Preview tabs', description: 'Single-click opens files in a reusable italic preview tab.', group: 'Workbench' },
  { id: 'ui.compactFolders', page: 'appearance', type: 'boolean', title: 'Compact folders', description: 'Collapse single-child folders in the explorer.', group: 'Workbench' },
  { id: 'ui.confirmDelete', page: 'appearance', type: 'boolean', title: 'Confirm before deleting files', group: 'Workbench' },
  { id: 'ui.terminalFontFamily', page: 'appearance', type: 'string', title: 'Terminal font family', group: 'Terminal' },
  { id: 'ui.terminalFontSize', page: 'appearance', type: 'number', title: 'Terminal font size', unit: 'px', min: 8, max: 32, group: 'Terminal' },
  { id: 'ui.terminalCursorStyle', page: 'appearance', type: 'select', title: 'Terminal cursor style', options: ['block', 'bar', 'underline'].map((v) => ({ value: v, label: v })), group: 'Terminal' },
  { id: 'ui.terminalCursorBlink', page: 'appearance', type: 'boolean', title: 'Terminal cursor blinking', group: 'Terminal' },
  { id: 'ui.terminalScrollback', page: 'appearance', type: 'number', title: 'Terminal scrollback', unit: 'lines', min: 500, max: 100000, step: 500, group: 'Terminal' },
  { id: 'terminal.shellIntegration', page: 'appearance', type: 'boolean', title: 'Shell integration', description: 'Inject Caret\'s zsh/bash/fish hooks so the terminal knows where each command starts and ends (OSC 133) and the current directory. Applies to new terminals.', group: 'Terminal', keywords: ['osc 133', 'prompt', 'zdotdir'] },
  { id: 'terminal.blocks', page: 'appearance', type: 'boolean', title: 'Command blocks', description: 'Group each command and its output into a block: gutter bars, click to select, ⌘↑/⌘↓ to jump, re-run, copy and send a single command\'s output to chat.', dependsOn: 'terminal.shellIntegration', group: 'Terminal', keywords: ['semantic terminal', 'warp'] },
  { id: 'terminal.maxBlocks', page: 'appearance', type: 'number', title: 'Blocks kept per terminal', description: 'Older blocks (and their captured output, max 64 KiB each) are dropped beyond this.', unit: 'blocks', min: 20, max: 2000, step: 20, dependsOn: 'terminal.blocks', group: 'Terminal' },
  { id: 'terminal.programStatus', page: 'appearance', type: 'boolean', title: 'Program status badges', description: 'Show when a CLI (Claude Code, Codex, Gemini CLI, builds…) reports it is working, waiting for input, done or failed via OSC 7501, and list waiting terminals in the panel.', group: 'Terminal', keywords: ['osc 7501', 'waiting for input', 'agents'] },
  { id: 'terminal.aiEnabled', page: 'appearance', type: 'boolean', title: 'Terminal AI features', description: 'Generate commands with ⌘K, explain output and add command output to chat. Turn off for a plain terminal.', group: 'Terminal', keywords: ['cmd+k', 'explain'] },

  // ── Beta ──
  { id: 'beta.browserTools', page: 'beta', type: 'boolean', title: 'Browser tools', description: 'Let the agent open a browser window, read the page and interact with it.' },
  { id: 'beta.nextEditPrediction', page: 'beta', type: 'boolean', title: 'Next-edit prediction (Tab to jump)' },
  { id: 'beta.backgroundAgents', page: 'beta', type: 'boolean', title: 'Background agents', description: 'Run several agent sessions concurrently, optionally in git worktrees.' },
  { id: 'beta.voice', page: 'beta', type: 'boolean', title: 'Voice input', description: 'Coming soon.' },
  { id: 'beta.designMode', page: 'beta', type: 'boolean', title: 'Design mode', description: 'Coming soon.' },

  // ── MCP / LSP toggles ──
  { id: 'mcp.enabled', page: 'mcp', type: 'boolean', title: 'Enable MCP', description: 'Connect to configured MCP servers and expose their tools to the agent.' },
  { id: 'lsp.enabled', page: 'appearance', type: 'boolean', title: 'Language servers', description: 'Start installed language servers for diagnostics, hover, go to definition.', group: 'Editor', keywords: ['lsp', 'intellisense'] },
]

export function getPath(obj: unknown, path: string): unknown {
  return path.split('.').reduce<unknown>((o, k) => (o && typeof o === 'object' ? (o as Record<string, unknown>)[k] : undefined), obj)
}

/** Build a nested DeepPartial patch for `path` = `value`. `undefined` is encoded as `null` (backend deletes the key). */
export function patchFor(path: string, value: unknown): DeepPartial<Settings> {
  const keys = path.split('.')
  const root: Record<string, unknown> = {}
  let cur = root
  keys.forEach((k, i) => {
    if (i === keys.length - 1) cur[k] = value === undefined ? null : value
    else { cur[k] = {}; cur = cur[k] as Record<string, unknown> }
  })
  return root as DeepPartial<Settings>
}

export function defaultFor(path: string): unknown { return getPath(DEFAULTS, path) }

export function valuesEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (a == null || b == null) return (a == null) && (b == null)
  if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((v, i) => valuesEqual(v, b[i]))
  if (typeof a === 'object' && typeof b === 'object') {
    const ak = Object.keys(a as object).sort(); const bk = Object.keys(b as object).sort()
    return ak.length === bk.length && ak.every((k, i) => k === bk[i] && valuesEqual((a as Record<string, unknown>)[k], (b as Record<string, unknown>)[k]))
  }
  return false
}

export function isModified(settings: Settings | null, field: SettingField | string): boolean {
  if (!settings) return false
  const id = typeof field === 'string' ? field : field.id
  return !valuesEqual(getPath(settings, id), defaultFor(id))
}

function norm(s: string): string { return s.toLowerCase().replace(/[^a-z0-9]+/g, ' ').trim() }

/** Rank fields by a free-text query over title, description, id, page and keywords. */
export function searchFields(query: string, fields: SettingField[] = SETTINGS_FIELDS): SettingField[] {
  const q = norm(query)
  if (!q) return fields
  const terms = q.split(' ').filter(Boolean)
  const scored = fields.map((f) => {
    const hay = {
      title: norm(f.title), desc: norm(f.description ?? ''), id: norm(f.id), page: norm(SETTINGS_PAGES.find((p) => p.id === f.page)?.title ?? f.page), kw: norm((f.keywords ?? []).join(' ')), group: norm(f.group ?? ''),
    }
    let score = 0
    for (const t of terms) {
      if (hay.title.includes(t)) score += hay.title.startsWith(t) ? 6 : 4
      else if (hay.kw.includes(t)) score += 3
      else if (hay.id.includes(t)) score += 3
      else if (hay.group.includes(t) || hay.page.includes(t)) score += 2
      else if (hay.desc.includes(t)) score += 1
      else return { f, score: 0 }
    }
    return { f, score }
  })
  return scored.filter((s) => s.score > 0).sort((a, b) => b.score - a.score).map((s) => s.f)
}

export function fieldsForPage(page: SettingsPageId): SettingField[] { return SETTINGS_FIELDS.filter((f) => f.page === page) }

/** Format a KeyboardEvent as a Caret keybinding string (`mod+shift+k`, `ctrl+alt+f5`). Returns null for pure modifier presses. */
export function formatKeyChord(e: { key: string; code?: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean }, platform: 'mac' | 'other' = 'mac'): string | null {
  const key = e.key
  if (['Meta', 'Control', 'Alt', 'Shift', 'CapsLock', 'Dead', 'Unidentified'].includes(key)) return null
  const parts: string[] = []
  const primaryIsMeta = platform === 'mac'
  if ((primaryIsMeta && e.metaKey) || (!primaryIsMeta && e.ctrlKey)) parts.push('mod')
  if (primaryIsMeta && e.ctrlKey) parts.push('ctrl')
  if (!primaryIsMeta && e.metaKey) parts.push('meta')
  if (e.altKey) parts.push('alt')
  if (e.shiftKey) parts.push('shift')
  let k = key.length === 1 ? key.toLowerCase() : key.toLowerCase()
  const map: Record<string, string> = { ' ': 'space', arrowup: 'up', arrowdown: 'down', arrowleft: 'left', arrowright: 'right', escape: 'escape', backspace: 'backspace', delete: 'delete', enter: 'enter', tab: 'tab', pageup: 'pageup', pagedown: 'pagedown', home: 'home', end: 'end' }
  k = map[k] ?? k
  // When alt/shift produce symbols on mac, prefer the physical key from `code` for letters/digits.
  if (e.code && /^(Key[A-Z]|Digit[0-9])$/.test(e.code) && k.length === 1 && !/^[a-z0-9]$/.test(k)) k = e.code.replace('Key', '').replace('Digit', '').toLowerCase()
  parts.push(k)
  return parts.join('+')
}

/** Pretty-print a keybinding string using mac glyphs. */
export function prettyKeybinding(kb: string | undefined | null, platform: 'mac' | 'other' = 'mac'): string[] {
  if (!kb) return []
  return kb.split(' ').flatMap((chord, i) => {
    const keys = chord.split('+').map((p) => {
      const m: Record<string, string> = platform === 'mac'
        ? { mod: '⌘', meta: '⌘', ctrl: '⌃', alt: '⌥', shift: '⇧', enter: '↩', backspace: '⌫', delete: '⌦', escape: '⎋', tab: '⇥', up: '↑', down: '↓', left: '←', right: '→', space: '␣' }
        : { mod: 'Ctrl', meta: 'Win', ctrl: 'Ctrl', alt: 'Alt', shift: 'Shift', enter: 'Enter', backspace: 'Backspace', delete: 'Del', escape: 'Esc', tab: 'Tab', up: '↑', down: '↓', left: '←', right: '→', space: 'Space' }
      return m[p] ?? (p.length === 1 ? p.toUpperCase() : p[0].toUpperCase() + p.slice(1))
    })
    return i === 0 ? keys : ['then', ...keys]
  })
}

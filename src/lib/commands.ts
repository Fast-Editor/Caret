/**
 * UI command registry. Every user-facing action is a command with a stable id (see docs/UI_COMMANDS.md).
 * Modules register handlers for their own domain and invoke other domains ONLY through `runCommand`,
 * which keeps editor / chat / shell modules decoupled. Keybindings and the palette map onto this registry.
 */
export interface CommandSpec {
  id: string
  title: string
  category?: string
  /** Default keybinding in Monaco/VS Code notation, e.g. "cmd+shift+p" (use "mod" for cmd/ctrl). */
  keybinding?: string
  /** Hide from palette. */
  hidden?: boolean
  icon?: string
  when?: () => boolean
  run: (args?: unknown) => unknown | Promise<unknown>
}

const registry = new Map<string, CommandSpec>()
const listeners = new Set<() => void>()

export function registerCommand(spec: CommandSpec): () => void {
  registry.set(spec.id, spec)
  listeners.forEach((l) => l())
  return () => { if (registry.get(spec.id) === spec) { registry.delete(spec.id); listeners.forEach((l) => l()) } }
}
export function registerCommands(specs: CommandSpec[]): () => void {
  const offs = specs.map(registerCommand)
  return () => offs.forEach((o) => o())
}
export function getCommand(id: string): CommandSpec | undefined { return registry.get(id) }
export function listCommands(): CommandSpec[] { return [...registry.values()] }
export function hasCommand(id: string): boolean { return registry.has(id) }
export function onCommandsChanged(l: () => void): () => void { listeners.add(l); return () => { listeners.delete(l) } }

export async function runCommand<T = unknown>(id: string, args?: unknown): Promise<T | undefined> {
  const cmd = registry.get(id)
  if (!cmd) { console.warn(`[commands] unknown command: ${id}`); return undefined }
  if (cmd.when && !cmd.when()) return undefined
  try { return (await cmd.run(args)) as T } catch (e) { console.error(`[commands] ${id} failed`, e); throw e }
}

/** Well-known command ids (keep in sync with docs/UI_COMMANDS.md). */
export const Cmd = {
  // workspace / files
  openFolder: 'workspace.openFolder', closeFolder: 'workspace.closeFolder', openRecent: 'workspace.openRecent', newFile: 'file.new', save: 'file.save', saveAll: 'file.saveAll', revealInFinder: 'file.revealInFinder',
  // editor
  openFile: 'editor.openFile', openDiff: 'editor.openDiff', closeTab: 'editor.closeTab', closeAllTabs: 'editor.closeAllTabs', splitEditor: 'editor.split', focusGroup: 'editor.focusGroup', goToLine: 'editor.goToLine', goToSymbol: 'editor.goToSymbol', goToDefinition: 'editor.goToDefinition', findReferences: 'editor.findReferences', rename: 'editor.rename', format: 'editor.format', find: 'editor.find', replace: 'editor.replace', toggleMinimap: 'editor.toggleMinimap', toggleWordWrap: 'editor.toggleWordWrap', revealRange: 'editor.revealRange', openSettingsUi: 'settings.open', openKeybindings: 'settings.keybindings', openWelcome: 'editor.openWelcome', openPlan: 'editor.openPlan', openBrowser: 'editor.openBrowser', openImage: 'editor.openImage', openMarkdownPreview: 'editor.markdownPreview', toggleBlame: 'editor.toggleBlame', nextProblem: 'editor.nextProblem',
  // tab / inline AI
  acceptInline: 'ai.tab.accept', acceptInlineWord: 'ai.tab.acceptWord', toggleTab: 'ai.tab.toggle', inlineEdit: 'ai.inlineEdit', inlineEditTerminal: 'ai.terminalCmdK', quickQuestion: 'ai.quickQuestion', explainSelection: 'ai.explain', fixInChat: 'ai.fixInChat', applyCodeBlock: 'ai.apply', generateCommitMessage: 'ai.commitMessage', review: 'ai.review',
  // chat
  chatOpen: 'chat.open', chatToggle: 'chat.toggle', chatNew: 'chat.new', chatFocus: 'chat.focusInput', chatAddSelection: 'chat.addSelection', chatAddFile: 'chat.addFile', chatSend: 'chat.sendPrompt', chatHistory: 'chat.history', chatSetMode: 'chat.setMode', chatStop: 'chat.stop', chatAcceptAll: 'chat.acceptAll', chatRejectAll: 'chat.rejectAll', chatNewBackground: 'chat.newBackground', chatAddTerminal: 'chat.addTerminalOutput', chatAddLints: 'chat.addLints',
  // sidebar / panels / layout
  showExplorer: 'view.explorer', showSearch: 'view.search', showScm: 'view.scm', showAgents: 'view.agents', showExtensions: 'view.rules', toggleSidebar: 'view.toggleSidebar', togglePanel: 'view.togglePanel', toggleChat: 'view.toggleChat', showProblems: 'view.problems', showOutput: 'view.output', showReview: 'view.review', toggleAgentsLayout: 'view.toggleAgentsLayout', toggleZen: 'view.zen', zoomIn: 'view.zoomIn', zoomOut: 'view.zoomOut', zoomReset: 'view.zoomReset', focusSidebar: 'view.focusSidebar', focusEditor: 'view.focusEditor', focusPanel: 'view.focusPanel',
  // palette
  commandPalette: 'palette.commands', quickOpen: 'palette.files', findInFiles: 'search.findInFiles', replaceInFiles: 'search.replaceInFiles',
  // terminal
  terminalNew: 'terminal.new', terminalToggle: 'terminal.toggle', terminalFocus: 'terminal.focus', terminalKill: 'terminal.kill', terminalSplit: 'terminal.split', terminalRun: 'terminal.runCommand', terminalClear: 'terminal.clear',
  // git
  gitCommit: 'git.commit', gitStageAll: 'git.stageAll', gitPush: 'git.push', gitPull: 'git.pull', gitFetch: 'git.fetch', gitCheckout: 'git.checkout', gitRefresh: 'git.refresh', gitOpenDiff: 'git.openDiff', gitDiscard: 'git.discard', gitStage: 'git.stage', gitUnstage: 'git.unstage', gitNewBranch: 'git.newBranch',
  // misc
  notify: 'app.notify', reload: 'app.reload', toggleDevtools: 'app.devtools', openConfigFolder: 'app.openConfigFolder', toggleTheme: 'app.toggleTheme', toggleAutoRun: 'agent.toggleAutoRun', openMcp: 'settings.mcp', openRules: 'settings.rules', openIndexing: 'settings.indexing', openModels: 'settings.models', openKeyboardShortcuts: 'help.shortcuts', showAbout: 'help.about',
} as const
export type CommandId = (typeof Cmd)[keyof typeof Cmd]

/** Payload shapes for the cross-module commands (others take no args). */
export interface CommandArgs {
  'editor.openFile': { path: string; line?: number; column?: number; endLine?: number; preview?: boolean; focus?: boolean; group?: number }
  'editor.openDiff': { path: string; original: string; modified: string; title?: string; language?: string; changeId?: string; sessionId?: string }
  'editor.revealRange': { path: string; startLine: number; endLine: number }
  'chat.addSelection': { path: string; startLine: number; endLine: number; text: string } | undefined
  'chat.addFile': { path: string }
  'chat.sendPrompt': { text: string; context?: import('@/types').ContextItem[]; mode?: import('@/types').ModeId; newSession?: boolean; submit?: boolean }
  'chat.open': { sessionId?: string } | undefined
  'chat.setMode': { mode: import('@/types').ModeId }
  'ai.fixInChat': { path: string; line: number; message: string; code?: string }
  'ai.apply': { path?: string; code: string; language?: string }
  'ai.review': { scope?: import('@/types').ReviewRequest['scope']; base?: string; files?: string[] } | undefined
  'terminal.runCommand': { command: string; newTerminal?: boolean; focus?: boolean }
  'terminal.new': { cwd?: string; command?: string } | undefined
  'view.problems': undefined
  'settings.open': { page?: string; query?: string } | undefined
  'app.notify': { level: 'info' | 'warn' | 'error' | 'success'; title: string; message?: string; timeoutMs?: number }
  'editor.openPlan': { path: string }
  'editor.openBrowser': { url?: string; tabId?: string } | undefined
  'git.openDiff': { path: string; staged?: boolean; base?: string }
  'workspace.openFolder': { path?: string } | undefined
}
export function run<K extends keyof CommandArgs>(id: K, args: CommandArgs[K]): Promise<unknown> { return runCommand(id, args) }

// ───────────── chat module (appended by ui-chat) ─────────────
/** Ids registered by the chat module, plus the editor data-query ids it calls when available. */
export const ChatCmd = {
  cycleMode: 'chat.cycleMode', toggleAgentsLayout: 'chat.toggleAgentsLayout', selectModel: 'chat.selectModel', addTerminalOutput: 'chat.addTerminalOutput', addLints: 'chat.addLints', explain: 'ai.explain',
  // expected from the editor module (return data; optional — chat degrades gracefully when absent)
  editorGetActiveFile: 'editor.getActiveFile', editorGetSelection: 'editor.getSelection', editorGetOpenFiles: 'editor.getOpenFiles', editorGetDocumentSymbols: 'editor.getDocumentSymbols', editorInsertAtCursor: 'editor.insertAtCursor',
  // expected from the shell (optional)
  viewReview: 'view.review', viewToggleAgentsLayout: 'view.toggleAgentsLayout', gitSetCommitMessage: 'git.setCommitMessage', helpShortcuts: 'help.shortcuts',
} as const

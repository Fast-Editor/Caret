/**
 * Chat module bootstrap: subscribes to the agent event stream (batched per animation frame),
 * loads sessions, reacts to workspace changes, and registers the chat-domain UI commands.
 */
import type { AgentEvent, ContextItem, ReviewRequest } from '@/types'
import { api } from '@/lib/api'
import { Events, on } from '@/lib/tauri'
import { registerCommands, runCommand, hasCommand } from '@/lib/commands'
import { useChat, AUTO_MODEL, isSessionBusy } from '@/store/chat'
import { emit as busEmit } from '@/lib/bus'
import { initReviewStore, useReview } from '@/store/review'
import { useUi } from '@/store/ui'
import { uid, baseName } from '@/components/chat/util'

let initialized = false
let pending: AgentEvent[] = []
let frame: number | null = null

function flush() {
  frame = null
  const batch = pending
  pending = []
  useChat.getState().ingest(batch)
}
function onAgentEvent(e: AgentEvent) {
  if (e.type === 'session-reset') { void useChat.getState().loadSession(e.sessionId); return }
  if (e.type === 'verification' || e.type === 'review-comments') { useReview.getState().ingestAgentEvent(e); return }
  pending.push(e)
  // Structural events are flushed immediately so approvals/questions never lag behind.
  if (e.type !== 'text-delta' && e.type !== 'thinking-delta' && e.type !== 'tool-input-delta') { if (frame !== null) cancelAnimationFrame(frame); flush(); return }
  if (frame === null) frame = requestAnimationFrame(flush)
}

async function subscribe() {
  try { await api.chat.subscribe(onAgentEvent) } catch (e) { console.error('chat subscribe failed', e) }
}

export interface ActiveFileInfo { path: string; language?: string; text?: string }
export interface SelectionInfo { path: string; startLine: number; endLine: number; text: string }

export async function getActiveFile(): Promise<ActiveFileInfo | null> {
  if (!hasCommand('editor.getActiveFile')) return null
  try { return ((await runCommand<ActiveFileInfo | null>('editor.getActiveFile')) ?? null) } catch { return null }
}
export async function getSelection(): Promise<SelectionInfo | null> {
  if (!hasCommand('editor.getSelection')) return null
  try { const s = await runCommand<SelectionInfo | null>('editor.getSelection'); return s && s.text ? s : null } catch { return null }
}
export async function getOpenFiles(): Promise<string[]> {
  if (!hasCommand('editor.getOpenFiles')) return []
  try { return (await runCommand<string[]>('editor.getOpenFiles')) ?? [] } catch { return [] }
}

export function openPane(sessionId?: string) {
  const ui = useUi.getState()
  if (!ui.chatVisible) ui.set({ chatVisible: true })
  if (sessionId) void useChat.getState().selectSession(sessionId)
  useChat.getState().focusComposer()
}

function selectionItem(sel: SelectionInfo): ContextItem {
  return { id: uid('sel'), kind: 'selection', label: `${baseName(sel.path)}:${sel.startLine}-${sel.endLine}`, path: sel.path, range: { startLine: sel.startLine, endLine: sel.endLine }, data: sel.text }
}

async function addSelectionOrFile(args?: SelectionInfo) {
  const chat = useChat.getState()
  const sessionId = await chat.ensureActiveSession()
  const sel = args ?? (await getSelection())
  if (sel) chat.addContext(sessionId, selectionItem(sel))
  else {
    const f = await getActiveFile()
    if (f) chat.addContext(sessionId, { id: uid('file'), kind: 'file', label: baseName(f.path), path: f.path })
  }
  openPane(sessionId)
}

export function initChatModule(): void {
  if (initialized) { void subscribe(); return }
  initialized = true
  void subscribe()
  initReviewStore()
  let lastRunning = -1
  const publishAgents = () => { const n = useChat.getState().sessions.filter((x) => isSessionBusy(x.status) && !x.isSubagent).length; if (n !== lastRunning) { lastRunning = n; busEmit('agents.status', { running: n }) } }
  useChat.subscribe(publishAgents); publishAgents()
  const chat = useChat.getState()
  void chat.loadSessions()
  void chat.loadMeta()
  on(Events.workspaceChanged, () => { useChat.getState().reset(); void useChat.getState().loadSessions(); void useChat.getState().loadMeta(); void useReview.getState().load() })
  on(Events.rulesChanged, () => { void useChat.getState().loadMeta() })

  registerCommands([
    { id: 'chat.open', title: 'Chat: Open AI Pane', category: 'Chat', run: (a) => openPane((a as { sessionId?: string } | undefined)?.sessionId) },
    { id: 'chat.toggle', title: 'Chat: Toggle AI Pane', category: 'Chat', keybinding: 'mod+l', run: async () => { const ui = useUi.getState(); if (ui.chatVisible) { const sel = await getSelection(); if (sel) { await addSelectionOrFile(sel); return } ui.set({ chatVisible: false }) } else { const sel = await getSelection(); if (sel) await addSelectionOrFile(sel); else { ui.set({ chatVisible: true }); useChat.getState().focusComposer() } } } },
    { id: 'chat.new', title: 'Chat: New Agent Chat', category: 'Chat', keybinding: 'mod+i', run: async (a) => { const o = a as { mode?: string; agentId?: string } | undefined; const s = await useChat.getState().createSession({ mode: o?.mode, agentId: o?.agentId }); openPane(s.id); return s.id } },
    { id: 'chat.focusInput', title: 'Chat: Focus Input', category: 'Chat', hidden: true, run: () => openPane() },
    { id: 'chat.addSelection', title: 'Chat: Add Selection to Chat', category: 'Chat', keybinding: 'mod+shift+l', run: (a) => addSelectionOrFile(a as SelectionInfo | undefined) },
    { id: 'chat.addFile', title: 'Chat: Add File to Chat', category: 'Chat', hidden: true, run: async (a) => { const { path } = a as { path: string }; const c = useChat.getState(); const id = await c.ensureActiveSession(); c.addContext(id, { id: uid('file'), kind: path.endsWith('/') ? 'folder' : 'file', label: baseName(path), path }); openPane(id) } },
    { id: 'chat.sendPrompt', title: 'Chat: Send Prompt', category: 'Chat', hidden: true, run: async (a) => {
      const args = a as { text: string; context?: ContextItem[]; mode?: string; newSession?: boolean; submit?: boolean }
      const c = useChat.getState()
      if (args.submit === false) {
        let id = c.activeSessionId
        if (!id || args.newSession) id = (await c.createSession({ mode: args.mode })).id
        c.updateDraft(id, (d) => ({ text: args.text, context: [...d.context, ...(args.context ?? [])], mode: args.mode ?? d.mode }))
        openPane(id); return id
      }
      const id = await c.sendText(args.text, { context: args.context, mode: args.mode, newSession: args.newSession })
      openPane(id); return id
    } },
    { id: 'chat.history', title: 'Chat: Show History', category: 'Chat', keybinding: 'mod+shift+h', run: () => { openPane(); useChat.getState().setHistoryOpen(true) } },
    { id: 'chat.setMode', title: 'Chat: Set Mode', category: 'Chat', hidden: true, run: (a) => useChat.getState().setMode(null, (a as { mode: string }).mode) },
    { id: 'chat.cycleMode', title: 'Chat: Cycle Mode', category: 'Chat', keybinding: 'mod+.', run: () => { const c = useChat.getState(); const id = c.activeSessionId; if (!id) return; const modes = c.modes.length ? c.modes : []; const cur = c.draft(id).mode; const i = modes.findIndex((m) => m.id === cur); const next = modes[(i + 1) % Math.max(1, modes.length)]; if (next) void c.setMode(id, next.id) } },
    { id: 'chat.stop', title: 'Chat: Stop Generation', category: 'Chat', run: () => { const id = useChat.getState().activeSessionId; if (id) void useChat.getState().stop(id) } },
    { id: 'chat.acceptAll', title: 'Chat: Accept All Changes', category: 'Chat', run: () => { const id = useChat.getState().activeSessionId; if (id) void useChat.getState().acceptChanges(id, 'all') } },
    { id: 'chat.rejectAll', title: 'Chat: Reject All Changes', category: 'Chat', run: () => { const id = useChat.getState().activeSessionId; if (id) void useChat.getState().rejectChanges(id, 'all') } },
    { id: 'chat.newBackground', title: 'Chat: New Background Agent (worktree)', category: 'Chat', keybinding: 'mod+e', run: async () => { const s = await useChat.getState().createSession({ useWorktree: true }); openPane(s.id); void runCommand('app.notify', { level: 'info', title: 'Background agent', message: 'This agent runs in an isolated git worktree. Send a task to start.' }) } },
    { id: 'chat.addTerminalOutput', title: 'Chat: Add Terminal Output to Chat', category: 'Chat', run: async (a) => { const { terminalId, title } = (a ?? {}) as { terminalId?: string; title?: string }; const c = useChat.getState(); const id = await c.ensureActiveSession(); let tid = terminalId; let label = title; if (!tid) { const list = await api.pty.list().catch(() => []); tid = list[0]?.id; label = list[0]?.title } if (!tid) { void runCommand('app.notify', { level: 'warn', title: 'No terminal open' }); return } c.addContext(id, { id: uid('term'), kind: 'terminal', label: label ?? 'Terminal', terminalId: tid }); openPane(id) } },
    { id: 'chat.addLints', title: 'Chat: Add Lint Errors to Chat', category: 'Chat', run: async () => { const c = useChat.getState(); const id = await c.ensureActiveSession(); const f = await getActiveFile(); c.addContext(id, { id: uid('lint'), kind: 'lint', label: 'Lint errors', path: f?.path }); openPane(id) } },
    { id: 'ai.fixInChat', title: 'AI: Fix in Chat', category: 'AI', hidden: true, run: async (a) => {
      const { path, line, message, code } = a as { path: string; line: number; message: string; code?: string }
      const text = `Fix this lint error in ${baseName(path)}:${line}: ${message}${code ? ` (${code})` : ''}`
      const id = await useChat.getState().sendText(text, { context: [{ id: uid('file'), kind: 'file', label: baseName(path), path, range: { startLine: Math.max(1, line - 20), endLine: line + 20 } }, { id: uid('lint'), kind: 'lint', label: 'Lint errors', path }], mode: 'agent' })
      openPane(id)
    } },
    { id: 'ai.explain', title: 'AI: Explain Selection', category: 'AI', run: async () => { const sel = await getSelection(); const f = sel ? null : await getActiveFile(); const ctx: ContextItem[] = sel ? [selectionItem(sel)] : f ? [{ id: uid('file'), kind: 'file', label: baseName(f.path), path: f.path }] : []; const id = await useChat.getState().sendText(sel ? 'Explain this code.' : 'Explain this file.', { context: ctx, mode: 'ask' }); openPane(id) } },
    { id: 'ai.review', title: 'AI: Review Changes (Bugbot)', category: 'AI', keybinding: 'mod+alt+r', run: async (a) => { const args = (a ?? {}) as Partial<ReviewRequest>; if (hasCommand('view.review')) void runCommand('view.review'); else useUi.getState().showPanel('review'); await useReview.getState().run(args) } },
    { id: 'ai.commitMessage', title: 'AI: Generate Commit Message', category: 'AI', keybinding: 'mod+alt+g', run: async () => {
      try {
        const r = await api.ai.commitMessage({ includeBody: true })
        const full = r.body ? `${r.summary}\n\n${r.body}` : r.summary
        if (hasCommand('git.setCommitMessage')) { await runCommand('git.setCommitMessage', full); if (hasCommand('view.scm')) void runCommand('view.scm') }
        else { try { const { writeText } = await import('@tauri-apps/plugin-clipboard-manager'); await writeText(full) } catch {} void runCommand('app.notify', { level: 'success', title: 'Commit message generated', message: `${r.summary} (copied to clipboard)` }) }
      } catch (e) { void runCommand('app.notify', { level: 'error', title: 'Commit message failed', message: e instanceof Error ? e.message : String(e) }) }
    } },
    { id: 'chat.toggleAgentsLayout', title: 'View: Toggle Agents Layout', category: 'View', hidden: true, run: () => { if (hasCommand('view.toggleAgentsLayout')) return runCommand('view.toggleAgentsLayout'); const ui = useUi.getState(); ui.set({ layout: ui.layout === 'agents' ? 'editor' : 'agents' }) } },
    { id: 'chat.selectModel', title: 'Chat: Select Model', category: 'Chat', hidden: true, run: (a) => useChat.getState().setModel(null, (a as { modelId?: string } | undefined)?.modelId ?? AUTO_MODEL) },
  ])
}

import { create } from 'zustand'
import type {
  AgentEvent, ApprovalOption, ChatSession, ChatSessionSummary, ContextItem, Effort, ImageAttachment, ModeId, SendMessageRequest,
  SlashCommandInfo, ThinkingPref, TodoItem, PendingChange, Checkpoint, ModelConfig, MessageSearchHit, FanOutRequest,
} from '@/types'
import { api } from '@/lib/api'
import { emit as busEmit } from '@/lib/bus'
import { runCommand } from '@/lib/commands'
import { useSettings } from '@/store/settings'
import { notify } from '@/store/notifications'
import { applyAgentEvents, emptyCoreState, type ChatCoreState, type PendingQuestion } from './chatReducer'
import { BUILTIN_SLASH } from '@/components/chat/mentions'
import { uid } from '@/components/chat/util'
import { toggleCompareModel } from '@/components/chat/composer/compareModels'

export interface ModeDescriptor { id: string; name: string; description: string; icon?: string; builtin: boolean; tools: string[] | 'all' | 'readonly' }
export interface ToolDesc { name: string; displayName: string; description: string; group: string; readonly: boolean }

export interface ComposerDraft {
  text: string
  context: ContextItem[]
  images: ImageAttachment[]
  mode: ModeId
  modelId: string
  thinking: ThinkingPref
  effort: Effort
  includeActiveFile: boolean
  useWorktree: boolean
  /** 'caret' (built-in) or an ACP agent id (claude-code, codex, …). */
  agentId: string
  /** Models selected for a multi-model fan-out ("Compare models"); 2+ turns Send into "Send to N". */
  compareModelIds?: string[]
}

export const AUTO_MODEL = 'auto'

function draftForSession(session?: { mode?: ModeId; agentId?: string; modelId?: string } | null): ComposerDraft {
  const d = defaultDraft(session?.mode)
  if (session?.agentId) d.agentId = session.agentId
  return d
}

export function defaultDraft(mode?: ModeId): ComposerDraft {
  const s = useSettings.getState().settings
  return {
    text: '', context: [], images: [], mode: mode ?? (s?.agent.defaultMode as ModeId) ?? 'agent',
    modelId: s?.agent.defaultModelId ?? AUTO_MODEL, thinking: (s?.agent.thinking as ThinkingPref) ?? 'auto', effort: (s?.agent.effort as Effort) ?? 'high',
    includeActiveFile: true, useWorktree: false, agentId: s?.acp?.defaultAgentId || 'caret',
  }
}

export interface ChatState extends ChatCoreState {
  activeSessionId: string | null
  drafts: Record<string, ComposerDraft>
  modes: ModeDescriptor[]
  slashCommands: SlashCommandInfo[]
  toolCatalog: ToolDesc[]
  historyOpen: boolean
  editingMessageId: string | null
  sessionsLoaded: boolean
  loadingSession: Record<string, boolean>
  changeCache: Record<string, PendingChange>
  checkpoints: Record<string, Checkpoint[]>
  favoriteModels: string[]
  composerFocusNonce: number
  // thread lifecycle (docs/THREADS.md)
  /** Sessions from every workspace; populated while "group by project" is on. */
  allSessions: ChatSessionSummary[]
  /** Select this session once the list of a newly opened workspace arrives (thread opened from another project). */
  pendingSelect: string | null
  /** Message to scroll into view once its session is shown (message search). */
  revealTarget: { sessionId: string; messageId: string } | null
  /** Bumped by `chat.searchMessages`: the history popover opens focused on the search box with this query. */
  historySearch: { nonce: number; query: string }

  // event ingestion
  ingest: (events: AgentEvent[]) => void
  // sessions
  loadSessions: () => Promise<void>
  loadSession: (id: string) => Promise<ChatSession | null>
  createSession: (opts?: { mode?: ModeId; modelId?: string; title?: string; useWorktree?: boolean; activate?: boolean; agentId?: string }) => Promise<ChatSession>
  selectSession: (id: string | null) => Promise<void>
  ensureActiveSession: () => Promise<string>
  deleteSession: (id: string) => Promise<void>
  renameSession: (id: string, title: string) => Promise<void>
  duplicateSession: (id: string) => Promise<void>
  pinSession: (id: string, pinned: boolean) => Promise<void>
  archiveSession: (id: string, archived: boolean) => Promise<void>
  exportSession: (id: string, format: 'markdown' | 'json') => Promise<string>
  // conversation
  send: (sessionId: string, overrides?: Partial<SendMessageRequest>) => Promise<void>
  sendText: (text: string, opts?: { sessionId?: string; context?: ContextItem[]; mode?: ModeId; newSession?: boolean; images?: ImageAttachment[] }) => Promise<string>
  stop: (sessionId: string) => Promise<void>
  approve: (approvalId: string, option: ApprovalOption) => Promise<void>
  answer: (questionId: string, text: string) => Promise<void>
  acceptChanges: (sessionId: string, ids: string[] | 'all') => Promise<void>
  rejectChanges: (sessionId: string, ids: string[] | 'all') => Promise<void>
  loadChange: (changeId: string) => Promise<PendingChange | null>
  loadCheckpoints: (sessionId: string) => Promise<Checkpoint[]>
  restoreCheckpoint: (sessionId: string, checkpointId: string) => Promise<string[]>
  setTodos: (sessionId: string, todos: TodoItem[]) => Promise<void>
  summarize: (sessionId: string) => Promise<void>
  setMode: (sessionId: string | null, mode: ModeId) => Promise<void>
  setModel: (sessionId: string | null, modelId: string) => Promise<void>
  regenerate: (sessionId: string, messageId: string) => Promise<void>
  editMessage: (sessionId: string, messageId: string, text: string) => Promise<void>
  queueRemove: (sessionId: string, queuedId: string) => Promise<void>
  queueSendNow: (sessionId: string, queuedId: string) => Promise<void>
  // drafts
  draft: (sessionId: string) => ComposerDraft
  updateDraft: (sessionId: string, patch: Partial<ComposerDraft> | ((d: ComposerDraft) => Partial<ComposerDraft>)) => void
  addContext: (sessionId: string, item: ContextItem) => void
  removeContext: (sessionId: string, itemId: string) => void
  addImage: (sessionId: string, img: ImageAttachment) => void
  // misc
  loadMeta: () => Promise<void>
  setHistoryOpen: (open: boolean) => void
  setEditing: (messageId: string | null) => void
  toggleFavoriteModel: (modelId: string) => void
  focusComposer: () => void
  reset: () => void
  // queue / steer / background threads
  /** Inject the draft (or overrides) into the running turn; idle sessions are sent normally. */
  steer: (sessionId: string, overrides?: Partial<SendMessageRequest>) => Promise<void>
  /** Create a background session carrying the draft's agent/model/mode, send there, stay on the current thread. Returns the new id. */
  sendInNewThread: (sessionId: string, overrides?: Partial<SendMessageRequest>) => Promise<string | null>
  queueUpdate: (sessionId: string, queuedId: string, text: string, context?: ContextItem[]) => Promise<void>
  queueMove: (sessionId: string, queuedId: string, index: number) => Promise<void>
  // thread lifecycle (docs/THREADS.md)
  /** `until` in unix ms; null un-snoozes. */
  snoozeSession: (id: string, until: number | null) => Promise<void>
  /** Settling shows a 5 s "Undo" toast unless `opts.undo === false`. */
  settleSession: (id: string, settled: boolean, opts?: { undo?: boolean }) => Promise<void>
  regenerateTitle: (id: string) => Promise<void>
  loadAllSessions: () => Promise<void>
  searchMessages: (query: string, opts?: { includeArchived?: boolean; allWorkspaces?: boolean; limit?: number }) => Promise<MessageSearchHit[]>
  /** Open a thread; a thread from another workspace offers to open that folder first. `messageId` scrolls to a message. */
  openThread: (id: string, workspaceRoot?: string, messageId?: string) => Promise<void>
  clearReveal: () => void
  openHistorySearch: (query?: string) => void
  // forks / provider handoff / multi-model fan-out
  /** Clone a thread (optionally cut right after `messageId`) and make it active. */
  forkSession: (sessionId: string, messageId?: string) => Promise<ChatSession>
  /** Move an idle thread to another agent and/or model; the backend transfers the conversation within the budget. */
  handoffSession: (sessionId: string, target: { agentId?: string; modelId?: string }) => Promise<void>
  /** Send the draft to several models at once (forks of the session when it has history). Returns the new session ids. */
  fanOut: (sessionId: string | null, modelIds: string[], overrides?: { context?: ContextItem[] }) => Promise<string[]>
  toggleCompareModel: (sessionId: string, modelId: string) => void
  clearCompare: (sessionId: string) => void
}

/** Same request shape `send` builds from a draft (shared by steer / background send). Null when there is nothing to send. */
function requestFromDraft(sessionId: string, d: ComposerDraft, overrides?: Partial<SendMessageRequest>): SendMessageRequest | null {
  const text = (overrides?.text ?? d.text).trim()
  const images = overrides?.images ?? d.images
  if (!text && images.length === 0 && !(overrides?.context && overrides.context.length)) return null
  const req: SendMessageRequest = {
    sessionId, text, context: overrides?.context ?? d.context, images,
    mode: overrides?.mode ?? d.mode, modelId: (overrides?.modelId ?? d.modelId) === AUTO_MODEL ? undefined : overrides?.modelId ?? d.modelId,
    thinking: d.thinking, effort: d.effort, useWorktree: d.useWorktree, agentId: d.agentId,
    ...overrides,
  }
  delete (req as Partial<SendMessageRequest>).activeFile
  return req
}

function patchSummaries(list: ChatSessionSummary[], id: string, patch: Partial<ChatSessionSummary>): ChatSessionSummary[] {
  return list.some((x) => x.id === id) ? list.map((x) => (x.id === id ? { ...x, ...patch } : x)) : list
}
/** Mirror list-level agent events onto the cross-workspace list (kept in sync without a reload). */
function applyToAll(list: ChatSessionSummary[], events: AgentEvent[]): ChatSessionSummary[] {
  let next = list
  for (const e of events) {
    if (e.type === 'session-updated') next = patchSummaries(next, e.session.id, e.session)
    else if (e.type === 'session-created') next = next.some((x) => x.id === e.session.id) ? patchSummaries(next, e.session.id, e.session) : [e.session, ...next]
    else if (e.type === 'session-deleted') next = next.filter((x) => x.id !== e.sessionId)
    else if (e.type === 'title') next = patchSummaries(next, e.sessionId, { title: e.title })
    else if (e.type === 'session-status') next = patchSummaries(next, e.sessionId, { status: e.status })
  }
  return next
}
function undoToast(title: string, undo: () => void): void {
  notify('info', title, undefined, { timeoutMs: 5000, actions: [{ id: 'undo', label: 'Undo' }], onAction: (a) => { if (a === 'undo') undo() } })
}

const FAV_KEY = 'caret.chat.favoriteModels'
function loadFavs(): string[] { try { return JSON.parse(localStorage.getItem(FAV_KEY) ?? '[]') } catch { return [] } }

function sameItem(a: ContextItem, b: ContextItem): boolean {
  if (a.id === b.id) return true
  if (a.kind !== b.kind) return false
  const r = (x: ContextItem) => (x.range ? `${x.range.startLine}-${x.range.endLine}` : '')
  return (a.path ?? '') === (b.path ?? '') && r(a) === r(b) && (a.url ?? '') === (b.url ?? '') && (a.query ?? '') === (b.query ?? '') && (a.sha ?? '') === (b.sha ?? '') && (a.terminalId ?? '') === (b.terminalId ?? '') && (a.sessionId ?? '') === (b.sessionId ?? '') && (a.ruleId ?? '') === (b.ruleId ?? '') && (a.docId ?? '') === (b.docId ?? '') && (a.uri ?? '') === (b.uri ?? '')
}

export const useChat = create<ChatState>((set, get) => ({
  ...emptyCoreState(),
  activeSessionId: null,
  drafts: {},
  modes: [],
  slashCommands: [],
  toolCatalog: [],
  historyOpen: false,
  editingMessageId: null,
  sessionsLoaded: false,
  loadingSession: {},
  changeCache: {},
  checkpoints: {},
  favoriteModels: loadFavs(),
  composerFocusNonce: 0,
  allSessions: [],
  pendingSelect: null,
  revealTarget: null,
  historySearch: { nonce: 0, query: '' },

  ingest: (events) => {
    if (!events.length) return
    const s = get()
    const core: ChatCoreState = { sessions: s.sessions, loaded: s.loaded, approvals: s.approvals, questions: s.questions, versions: s.versions, streaming: s.streaming, steering: s.steering }
    const next = applyAgentEvents(core, events)
    if (next !== core) set(next)
    if (s.allSessions.length) { const all = applyToAll(s.allSessions, events); if (all !== s.allSessions) set({ allSessions: all }) }
    // external (ACP) agent status is UI-only state: fan out to the composer's agent picker
    for (const e of events) if (e.type === 'agent-status') busEmit('chat:agent-status', e)
    // auto-activate newly created foreground sessions when none is active
    if (!get().activeSessionId) { const created = events.find((e) => e.type === 'session-created'); if (created && created.type === 'session-created' && !created.session.isSubagent) set({ activeSessionId: created.session.id }) }
  },

  loadSessions: async () => {
    const sessions = await api.chat.listSessions(true)
    set({ sessions, sessionsLoaded: true })
    const active = get().activeSessionId
    if (active && !sessions.some((s) => s.id === active)) set({ activeSessionId: null })
    const pending = get().pendingSelect
    if (pending) { set({ pendingSelect: null }); if (sessions.some((s) => s.id === pending)) void get().selectSession(pending) }
    if (get().allSessions.length) void get().loadAllSessions()
  },
  loadSession: async (id) => {
    if (get().loadingSession[id]) return get().loaded[id] ?? null
    set((s) => ({ loadingSession: { ...s.loadingSession, [id]: true } }))
    try {
      const session = await api.chat.getSession(id)
      set((s) => ({ loaded: { ...s.loaded, [id]: session }, versions: Object.fromEntries(session.messages.map((m) => [m.id, (s.versions[m.id] ?? 0) + 1])) , sessions: s.sessions.some((x) => x.id === id) ? s.sessions : [...s.sessions] }))
      return session
    } catch (e) {
      console.error('loadSession failed', e)
      return null
    } finally {
      set((s) => { const l = { ...s.loadingSession }; delete l[id]; return { loadingSession: l } })
    }
  },
  createSession: async (opts) => {
    const session = await api.chat.createSession({ mode: opts?.mode, modelId: opts?.modelId === AUTO_MODEL ? undefined : opts?.modelId, title: opts?.title, useWorktree: opts?.useWorktree, agentId: opts?.agentId })
    set((s) => ({
      loaded: { ...s.loaded, [session.id]: session },
      sessions: s.sessions.some((x) => x.id === session.id) ? s.sessions : [{ id: session.id, title: session.title, workspaceRoot: session.workspaceRoot, createdAt: session.createdAt, updatedAt: session.updatedAt, mode: session.mode, status: session.status, messageCount: 0, lastMessagePreview: '', worktreePath: session.worktreePath, isSubagent: session.isSubagent, pendingChangeCount: 0, modelId: session.modelId }, ...s.sessions],
      activeSessionId: opts?.activate === false ? s.activeSessionId : session.id,
      drafts: { ...s.drafts, [session.id]: { ...defaultDraft(session.mode), modelId: session.modelId ?? AUTO_MODEL, useWorktree: !!opts?.useWorktree, agentId: session.agentId ?? opts?.agentId ?? 'caret' } },
    }))
    return session
  },
  selectSession: async (id) => {
    set({ activeSessionId: id, editingMessageId: null })
    if (id && !get().loaded[id]) await get().loadSession(id)
  },
  ensureActiveSession: async () => {
    const id = get().activeSessionId
    if (id) return id
    const s = await get().createSession()
    return s.id
  },
  deleteSession: async (id) => {
    await api.chat.deleteSession(id)
    set((s) => {
      const loaded = { ...s.loaded }; delete loaded[id]
      const drafts = { ...s.drafts }; delete drafts[id]
      return { loaded, drafts, sessions: s.sessions.filter((x) => x.id !== id), activeSessionId: s.activeSessionId === id ? null : s.activeSessionId }
    })
  },
  renameSession: async (id, title) => {
    await api.chat.renameSession(id, title)
    set((s) => ({ sessions: s.sessions.map((x) => (x.id === id ? { ...x, title } : x)), loaded: s.loaded[id] ? { ...s.loaded, [id]: { ...s.loaded[id], title } } : s.loaded }))
  },
  duplicateSession: async (id) => {
    const session = await api.chat.duplicateSession(id)
    set((s) => ({ loaded: { ...s.loaded, [session.id]: session }, activeSessionId: session.id }))
    await get().loadSessions()
  },
  pinSession: async (id, pinned) => { await api.chat.pinSession(id, pinned); set((s) => ({ sessions: s.sessions.map((x) => (x.id === id ? { ...x, pinned } : x)) })) },
  archiveSession: async (id, archived) => {
    await api.chat.archiveSession(id, archived)
    set((s) => ({ sessions: patchSummaries(s.sessions, id, { archived, archivedAt: archived ? Date.now() : undefined }) }))
    if (archived) undoToast('Thread archived', () => void get().archiveSession(id, false))
  },
  exportSession: (id, format) => api.chat.exportSession(id, format),

  send: async (sessionId, overrides) => {
    const d = get().draft(sessionId)
    const text = (overrides?.text ?? d.text).trim()
    const images = overrides?.images ?? d.images
    if (!text && images.length === 0 && !(overrides?.context && overrides.context.length)) return
    const session = get().loaded[sessionId]
    const running = session && (session.status === 'running' || session.status === 'awaiting-approval' || session.status === 'summarizing')
    const req: SendMessageRequest = {
      sessionId, text, context: overrides?.context ?? d.context, images,
      mode: overrides?.mode ?? d.mode, modelId: (overrides?.modelId ?? d.modelId) === AUTO_MODEL ? undefined : overrides?.modelId ?? d.modelId,
      thinking: d.thinking, effort: d.effort, useWorktree: d.useWorktree, queue: !!running, agentId: d.agentId,
      ...overrides,
    }
    delete (req as Partial<SendMessageRequest>).activeFile
    // keep pinned items, clear the rest
    set((s) => ({ drafts: { ...s.drafts, [sessionId]: { ...d, text: '', images: [], context: d.context.filter((c) => c.pinned) } }, editingMessageId: null }))
    try {
      await api.chat.send(req)
    } catch (e) {
      // restore draft on failure
      set((s) => ({ drafts: { ...s.drafts, [sessionId]: { ...d } } }))
      throw e
    }
  },
  sendText: async (text, opts) => {
    let sessionId = opts?.sessionId ?? get().activeSessionId
    if (!sessionId || opts?.newSession) { const s = await get().createSession({ mode: opts?.mode }); sessionId = s.id }
    else if (!get().loaded[sessionId]) await get().loadSession(sessionId)
    const d = get().draft(sessionId)
    if (opts?.mode) get().updateDraft(sessionId, { mode: opts.mode })
    await get().send(sessionId, { text, context: [...(opts?.context ?? []), ...d.context], images: opts?.images ?? [], mode: opts?.mode ?? d.mode })
    return sessionId
  },
  stop: (sessionId) => api.chat.stop(sessionId),
  approve: async (approvalId, option) => {
    await api.chat.approve(approvalId, option)
    set((s) => { const approvals = { ...s.approvals }; delete approvals[approvalId]; return { approvals } })
  },
  answer: async (questionId, text) => {
    const q = get().questions[questionId]
    await api.chat.answerQuestion(questionId, text)
    set((s) => {
      const questions = { ...s.questions }; delete questions[questionId]
      if (!q) return { questions }
      const session = s.loaded[q.sessionId]
      if (!session) return { questions }
      const messages = session.messages.map((m) => (m.id === q.messageId ? { ...m, parts: m.parts.map((p) => (p.type === 'question' && p.question.questionId === questionId ? { ...p, answer: text } : p)) } : m))
      return { questions, loaded: { ...s.loaded, [q.sessionId]: { ...session, messages } }, versions: { ...s.versions, [q.messageId]: (s.versions[q.messageId] ?? 0) + 1 } }
    })
  },
  acceptChanges: async (sessionId, ids) => {
    await api.chat.acceptChanges(sessionId, ids)
    set((s) => { const sess = s.loaded[sessionId]; if (!sess) return {}; return { loaded: { ...s.loaded, [sessionId]: { ...sess, pendingChanges: sess.pendingChanges.map((c) => (ids === 'all' || ids.includes(c.changeId)) && c.status === 'pending' ? { ...c, status: 'accepted' } : c) } } } })
  },
  rejectChanges: async (sessionId, ids) => {
    await api.chat.rejectChanges(sessionId, ids)
    set((s) => { const sess = s.loaded[sessionId]; if (!sess) return {}; return { loaded: { ...s.loaded, [sessionId]: { ...sess, pendingChanges: sess.pendingChanges.map((c) => (ids === 'all' || ids.includes(c.changeId)) && c.status === 'pending' ? { ...c, status: 'rejected' } : c) } } } })
  },
  loadChange: async (changeId) => {
    try { const c = await api.chat.changeDiff(changeId); set((s) => ({ changeCache: { ...s.changeCache, [changeId]: c } })); return c } catch { return null }
  },
  loadCheckpoints: async (sessionId) => {
    const cps = await api.chat.checkpoints(sessionId)
    set((s) => ({ checkpoints: { ...s.checkpoints, [sessionId]: cps } }))
    return cps
  },
  restoreCheckpoint: async (sessionId, checkpointId) => {
    const files = await api.chat.restoreCheckpoint(sessionId, checkpointId)
    await get().loadSession(sessionId)
    return files
  },
  setTodos: async (sessionId, todos) => {
    set((s) => { const sess = s.loaded[sessionId]; return sess ? { loaded: { ...s.loaded, [sessionId]: { ...sess, todos } } } : {} })
    await api.chat.setTodos(sessionId, todos)
  },
  summarize: (sessionId) => api.chat.summarize(sessionId),
  setMode: async (sessionId, mode) => {
    const id = sessionId ?? get().activeSessionId
    if (id) { get().updateDraft(id, { mode }); set((s) => ({ loaded: s.loaded[id] ? { ...s.loaded, [id]: { ...s.loaded[id], mode } } : s.loaded, sessions: s.sessions.map((x) => (x.id === id ? { ...x, mode } : x)) })); api.chat.setMode(id, mode).catch(() => {}) }
  },
  setModel: async (sessionId, modelId) => {
    const id = sessionId ?? get().activeSessionId
    if (id) { get().updateDraft(id, { modelId }); if (modelId !== AUTO_MODEL) api.chat.setModel(id, modelId).catch(() => {}) }
  },
  regenerate: (sessionId, messageId) => api.chat.regenerate(sessionId, messageId),
  editMessage: async (sessionId, messageId, text) => { set({ editingMessageId: null }); await api.chat.editMessage(sessionId, messageId, text) },
  queueRemove: (sessionId, queuedId) => api.chat.queueRemove(sessionId, queuedId),
  queueSendNow: (sessionId, queuedId) => api.chat.queueSendNow(sessionId, queuedId),

  // A fresh draft for an existing session inherits the session's agent (so reopening a Claude Code chat keeps it on Claude Code).
  draft: (sessionId) => get().drafts[sessionId] ?? draftForSession(get().loaded[sessionId]),
  updateDraft: (sessionId, patch) => set((s) => { const d = s.drafts[sessionId] ?? draftForSession(s.loaded[sessionId]); const p = typeof patch === 'function' ? patch(d) : patch; return { drafts: { ...s.drafts, [sessionId]: { ...d, ...p } } } }),
  addContext: (sessionId, item) => get().updateDraft(sessionId, (d) => (d.context.some((c) => sameItem(c, item)) ? {} : { context: [...d.context, { ...item, id: item.id || uid('ctx') }] })),
  removeContext: (sessionId, itemId) => get().updateDraft(sessionId, (d) => ({ context: d.context.filter((c) => c.id !== itemId) })),
  addImage: (sessionId, img) => get().updateDraft(sessionId, (d) => ({ images: [...d.images, img] })),

  loadMeta: async () => {
    const [modes, cmds, tools] = await Promise.all([
      api.chat.modes().catch(() => [] as ModeDescriptor[]),
      api.commands.list().catch(() => [] as SlashCommandInfo[]),
      api.chat.toolCatalog().catch(() => [] as ToolDesc[]),
    ])
    const builtin: SlashCommandInfo[] = BUILTIN_SLASH.map((b) => ({ name: b.name, description: b.description, source: 'builtin' }))
    set({ modes: modes.length ? modes : fallbackModes(), slashCommands: [...builtin, ...cmds.filter((c) => !builtin.some((b) => b.name === c.name))], toolCatalog: tools })
  },
  setHistoryOpen: (open) => set({ historyOpen: open }),
  setEditing: (messageId) => set({ editingMessageId: messageId }),
  toggleFavoriteModel: (modelId) => set((s) => { const f = s.favoriteModels.includes(modelId) ? s.favoriteModels.filter((m) => m !== modelId) : [...s.favoriteModels, modelId]; try { localStorage.setItem(FAV_KEY, JSON.stringify(f)) } catch {} return { favoriteModels: f } }),
  focusComposer: () => set((s) => ({ composerFocusNonce: s.composerFocusNonce + 1 })),
  reset: () => set({ ...emptyCoreState(), activeSessionId: null, drafts: {}, sessionsLoaded: false, changeCache: {}, checkpoints: {} }),

  // ── queue / steer / background threads ──
  steer: async (sessionId, overrides) => {
    const d = get().draft(sessionId)
    const req = requestFromDraft(sessionId, d, overrides)
    if (!req) return
    set((s) => ({ drafts: { ...s.drafts, [sessionId]: { ...d, text: '', images: [], context: d.context.filter((c) => c.pinned) } }, editingMessageId: null }))
    try { await api.chat.steer(req) } catch (e) { set((s) => ({ drafts: { ...s.drafts, [sessionId]: { ...d } } })); throw e }
  },
  sendInNewThread: async (sessionId, overrides) => {
    const d = get().draft(sessionId)
    const req = requestFromDraft(sessionId, d, overrides)
    if (!req) return null
    // The new thread inherits the draft's agent, model, mode and worktree choice; the user stays where they are.
    const created = await get().createSession({ activate: false, mode: req.mode, modelId: d.modelId, agentId: d.agentId, useWorktree: d.useWorktree })
    set((s) => ({ drafts: { ...s.drafts, [sessionId]: { ...d, text: '', images: [], context: d.context.filter((c) => c.pinned) }, [created.id]: { ...(s.drafts[created.id] ?? defaultDraft(req.mode)), thinking: d.thinking, effort: d.effort, agentId: d.agentId, modelId: d.modelId, useWorktree: d.useWorktree } } }))
    try { await api.chat.send({ ...req, sessionId: created.id, queue: false }) }
    catch (e) { set((s) => ({ drafts: { ...s.drafts, [sessionId]: { ...d } } })); throw e }
    return created.id
  },
  queueUpdate: (sessionId, queuedId, text, context) => api.chat.queueUpdate(sessionId, queuedId, text, context),
  queueMove: (sessionId, queuedId, index) => api.chat.queueMove(sessionId, queuedId, index),
  // ───────── thread lifecycle (docs/THREADS.md) ─────────
  snoozeSession: async (id, until) => {
    const sv = await api.chat.snoozeSession(id, until)
    set((s) => ({ sessions: patchSummaries(s.sessions, id, { snoozedUntil: sv.snoozedUntil, settled: sv.settled }), allSessions: patchSummaries(s.allSessions, id, { snoozedUntil: sv.snoozedUntil, settled: sv.settled }) }))
  },
  settleSession: async (id, settled, opts) => {
    await api.chat.settleSession(id, settled)
    set((s) => ({ sessions: patchSummaries(s.sessions, id, { settled }), allSessions: patchSummaries(s.allSessions, id, { settled }) }))
    if (settled && opts?.undo !== false) undoToast('Thread settled', () => void get().settleSession(id, false))
  },
  regenerateTitle: async (id) => {
    try { await api.chat.regenerateTitle(id) } // the `title` event updates the list and the pane
    catch (e) { notify('error', 'Could not regenerate title', e instanceof Error ? e.message : String(e)); throw e }
  },
  loadAllSessions: async () => {
    try { set({ allSessions: await api.chat.listSessionsAll(true) }) } catch (e) { console.error('loadAllSessions failed', e) }
  },
  searchMessages: (query, opts) => api.chat.searchMessages(query, opts),
  openThread: async (id, workspaceRoot, messageId) => {
    const local = get().sessions.some((s) => s.id === id)
    if (!local && workspaceRoot) {
      const name = workspaceRoot.split('/').filter(Boolean).pop() ?? workspaceRoot
      if (!window.confirm(`This thread belongs to "${name}" (${workspaceRoot}). Open that folder to continue it?`)) return
      set({ pendingSelect: id, revealTarget: messageId ? { sessionId: id, messageId } : null })
      await runCommand('workspace.openFolder', { path: workspaceRoot }) // workspace-changed reloads the list, which selects `pendingSelect`
      return
    }
    set({ revealTarget: messageId ? { sessionId: id, messageId } : null })
    await get().selectSession(id)
  },
  clearReveal: () => set({ revealTarget: null }),
  openHistorySearch: (query = '') => set((s) => ({ historyOpen: true, historySearch: { nonce: s.historySearch.nonce + 1, query } })),
  forkSession: async (sessionId, messageId) => {
    const session = await api.chat.forkSession(sessionId, messageId)
    set((s) => {
      const from = s.drafts[sessionId] ?? draftForSession(s.loaded[sessionId])
      return {
        loaded: { ...s.loaded, [session.id]: session }, activeSessionId: session.id, editingMessageId: null,
        drafts: { ...s.drafts, [session.id]: { ...from, text: '', images: [], context: [], compareModelIds: [], modelId: session.modelId ?? AUTO_MODEL, agentId: session.agentId ?? 'caret', useWorktree: false } },
      }
    })
    await get().loadSessions()
    return session
  },
  handoffSession: async (sessionId, target) => {
    const session = await api.chat.handoffSession(sessionId, target)
    set((s) => ({ loaded: { ...s.loaded, [sessionId]: session }, versions: Object.fromEntries(session.messages.map((m) => [m.id, (s.versions[m.id] ?? 0) + 1])), sessions: s.sessions.map((x) => (x.id === sessionId ? { ...x, agentId: session.agentId, modelId: session.modelId } : x)) }))
    get().updateDraft(sessionId, { agentId: session.agentId ?? 'caret', modelId: session.modelId ?? AUTO_MODEL })
  },
  fanOut: async (sessionId, modelIds, overrides) => {
    const id = sessionId ?? get().activeSessionId
    const d = id ? get().draft(id) : defaultDraft()
    const text = d.text.trim()
    const models = [...new Set(modelIds.filter((m) => m && m !== AUTO_MODEL))]
    if ((!text && d.images.length === 0) || models.length < 2) return []
    const hasHistory = !!id && (get().loaded[id]?.messages.length ?? 0) > 0
    const req: FanOutRequest = { sessionId: hasHistory && id ? id : undefined, text, modelIds: models, useWorktree: d.useWorktree, mode: d.mode, context: overrides?.context ?? d.context, images: d.images, thinking: d.thinking, effort: d.effort }
    if (id) set((s) => ({ drafts: { ...s.drafts, [id]: { ...d, text: '', images: [], context: d.context.filter((c) => c.pinned) } } }))
    try {
      const ids = await api.chat.fanOut(req)
      await get().loadSessions()
      if (ids[0]) await get().selectSession(ids[0])
      return ids
    } catch (e) {
      if (id) set((s) => ({ drafts: { ...s.drafts, [id]: { ...d } } }))
      throw e
    }
  },
  toggleCompareModel: (sessionId, modelId) => get().updateDraft(sessionId, (d) => ({ compareModelIds: toggleCompareModel(d.compareModelIds ?? [], modelId, d.modelId) })),
  clearCompare: (sessionId) => get().updateDraft(sessionId, { compareModelIds: [] }),
}))

export function fallbackModes(): ModeDescriptor[] {
  return [
    { id: 'agent', name: 'Agent', description: 'Autonomous coding with tools', icon: 'bot', builtin: true, tools: 'all' },
    { id: 'ask', name: 'Ask', description: 'Read-only Q&A and explanations', icon: 'message-circle', builtin: true, tools: 'readonly' },
    { id: 'plan', name: 'Plan', description: 'Research then write a plan to approve', icon: 'list-checks', builtin: true, tools: 'readonly' },
    { id: 'debug', name: 'Debug', description: 'Hypothesis-driven debugging', icon: 'bug', builtin: true, tools: 'all' },
  ]
}

/** Enabled models grouped by provider for the picker. */
export function useModelGroups(): { providerId: string; providerName: string; models: ModelConfig[] }[] {
  const settings = useSettings((s) => s.settings)
  if (!settings) return []
  const groups: Record<string, { providerId: string; providerName: string; models: ModelConfig[] }> = {}
  for (const m of settings.models) {
    if (!m.enabled) continue
    const p = settings.providers.find((x) => x.id === m.providerId)
    if (p && !p.enabled) continue
    const g = (groups[m.providerId] ??= { providerId: m.providerId, providerName: p?.name ?? m.providerId, models: [] })
    g.models.push(m)
  }
  return Object.values(groups)
}

export function resolveModel(modelId: string | undefined): ModelConfig | undefined {
  const s = useSettings.getState().settings
  if (!s) return undefined
  if (!modelId || modelId === AUTO_MODEL) return s.models.find((m) => m.enabled && (m.id === s.agent.defaultModelId || m.isDefault)) ?? s.models.find((m) => m.enabled)
  return s.models.find((m) => m.id === modelId)
}

/** Selectors */
export const useActiveSession = (): ChatSession | null => useChat((s) => (s.activeSessionId ? s.loaded[s.activeSessionId] ?? null : null))
export const useSessionSummary = (id: string | null): ChatSessionSummary | undefined => useChat((s) => (id ? s.sessions.find((x) => x.id === id) : undefined))
export const isSessionBusy = (status: string | undefined): boolean => status === 'running' || status === 'awaiting-approval' || status === 'awaiting-input' || status === 'summarizing'
export type { PendingQuestion }

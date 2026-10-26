/**
 * Pure reducers that fold AgentEvents into chat state. No Tauri imports so they can be unit-tested.
 */
import type {
  AgentEvent, ApprovalRequest, ChatMessage, ChatSession, ChatSessionSummary, FileChangeSummary, MessagePart, ToolCallRecord, AgentQuestion,
} from '@/types'

export interface PendingQuestion extends AgentQuestion { sessionId: string; messageId: string }

export interface ChatCoreState {
  sessions: ChatSessionSummary[]
  loaded: Record<string, ChatSession>
  approvals: Record<string, ApprovalRequest>
  questions: Record<string, PendingQuestion>
  /** messageId → monotonically increasing version (memoization key for rows). */
  versions: Record<string, number>
  /** sessionId → messageId currently streaming. */
  streaming: Record<string, string>
}

export const emptyCoreState = (): ChatCoreState => ({ sessions: [], loaded: {}, approvals: {}, questions: {}, versions: {}, streaming: {} })

function bump(versions: Record<string, number>, id: string): Record<string, number> {
  return { ...versions, [id]: (versions[id] ?? 0) + 1 }
}

function updateSummary(list: ChatSessionSummary[], id: string, patch: Partial<ChatSessionSummary>): ChatSessionSummary[] {
  let found = false
  const out = list.map((s) => { if (s.id !== id) return s; found = true; return { ...s, ...patch } })
  return found ? out : list
}

function summaryOf(s: ChatSession): ChatSessionSummary {
  const last = [...s.messages].reverse().find((m) => m.role !== 'system')
  return {
    id: s.id, title: s.title, workspaceRoot: s.workspaceRoot, createdAt: s.createdAt, updatedAt: s.updatedAt, mode: s.mode, status: s.status,
    messageCount: s.messages.length, lastMessagePreview: last ? previewOf(last) : '', worktreePath: s.worktreePath, isSubagent: s.isSubagent,
    pendingChangeCount: s.pendingChanges.filter((c) => c.status === 'pending').length, pinned: s.pinned, archived: s.archived, modelId: s.modelId,
  }
}

export function previewOf(m: ChatMessage): string {
  for (const p of m.parts) {
    if (p.type === 'text' && p.text.trim()) return p.text.trim().slice(0, 140)
    if (p.type === 'tool') return `${p.call.displayName}`
  }
  return ''
}

/** Replace a message inside a loaded session by id, returning new state objects along the path. */
function mapMessage(state: ChatCoreState, sessionId: string, messageId: string, fn: (m: ChatMessage) => ChatMessage): ChatCoreState {
  const session = state.loaded[sessionId]
  if (!session) return state
  let changed = false
  const messages = session.messages.map((m) => { if (m.id !== messageId) return m; changed = true; return fn(m) })
  if (!changed) return state
  return { ...state, loaded: { ...state.loaded, [sessionId]: { ...session, messages } }, versions: bump(state.versions, messageId) }
}

function mapSession(state: ChatCoreState, sessionId: string, fn: (s: ChatSession) => ChatSession, summaryPatch?: Partial<ChatSessionSummary>): ChatCoreState {
  const session = state.loaded[sessionId]
  const loaded = session ? { ...state.loaded, [sessionId]: fn(session) } : state.loaded
  const sessions = summaryPatch ? updateSummary(state.sessions, sessionId, summaryPatch) : session ? updateSummary(state.sessions, sessionId, summaryOf(loaded[sessionId])) : state.sessions
  return { ...state, loaded, sessions }
}

function appendText(parts: MessagePart[], kind: 'text' | 'thinking', text: string): MessagePart[] {
  const last = parts[parts.length - 1]
  if (last && last.type === kind) {
    const updated = { ...last, text: last.text + text } as MessagePart
    return [...parts.slice(0, -1), updated]
  }
  const fresh: MessagePart = kind === 'text' ? { type: 'text', text } : { type: 'thinking', text }
  return [...parts, fresh]
}

function mapToolPart(parts: MessagePart[], toolCallId: string, fn: (c: ToolCallRecord) => ToolCallRecord): MessagePart[] {
  return parts.map((p) => (p.type === 'tool' && p.call.id === toolCallId ? { type: 'tool', call: fn(p.call) } : p))
}

function upsertChange(list: FileChangeSummary[], change: FileChangeSummary): FileChangeSummary[] {
  const idx = list.findIndex((c) => c.changeId === change.changeId)
  if (idx === -1) return [...list, change]
  const out = list.slice(); out[idx] = change; return out
}

/** Fold one event. Text/thinking deltas may be pre-coalesced by the bridge. */
export function applyAgentEvent(state: ChatCoreState, ev: AgentEvent): ChatCoreState {
  switch (ev.type) {
    case 'session-created': {
      if (state.sessions.some((s) => s.id === ev.session.id)) return { ...state, sessions: updateSummary(state.sessions, ev.session.id, ev.session) }
      return { ...state, sessions: [ev.session, ...state.sessions] }
    }
    case 'session-deleted': {
      const loaded = { ...state.loaded }; delete loaded[ev.sessionId]
      const streaming = { ...state.streaming }; delete streaming[ev.sessionId]
      return { ...state, loaded, streaming, sessions: state.sessions.filter((s) => s.id !== ev.sessionId) }
    }
    case 'session-status':
      return mapSession(state, ev.sessionId, (s) => ({ ...s, status: ev.status }), { status: ev.status })
    case 'title':
      return mapSession(state, ev.sessionId, (s) => ({ ...s, title: ev.title }), { title: ev.title })
    case 'message-start': {
      const session = state.loaded[ev.sessionId]
      const next = { ...state, streaming: ev.message.role === 'assistant' ? { ...state.streaming, [ev.sessionId]: ev.message.id } : state.streaming }
      if (!session) return { ...next, sessions: updateSummary(state.sessions, ev.sessionId, { updatedAt: ev.message.createdAt }) }
      if (session.messages.some((m) => m.id === ev.message.id)) return mapMessage(next, ev.sessionId, ev.message.id, () => ev.message)
      const messages = [...session.messages, ev.message]
      const updated = { ...session, messages, updatedAt: ev.message.createdAt }
      return { ...next, loaded: { ...next.loaded, [ev.sessionId]: updated }, versions: bump(state.versions, ev.message.id), sessions: updateSummary(state.sessions, ev.sessionId, summaryOf(updated)) }
    }
    case 'text-delta':
      return mapMessage(state, ev.sessionId, ev.messageId, (m) => ({ ...m, parts: appendText(m.parts, 'text', ev.text), status: 'streaming' }))
    case 'thinking-delta':
      return mapMessage(state, ev.sessionId, ev.messageId, (m) => ({ ...m, parts: appendText(m.parts, 'thinking', ev.text), status: 'streaming' }))
    case 'tool-start':
      return mapMessage(state, ev.sessionId, ev.messageId, (m) => {
        if (m.parts.some((p) => p.type === 'tool' && p.call.id === ev.call.id)) return { ...m, parts: mapToolPart(m.parts, ev.call.id, () => ev.call) }
        return { ...m, parts: [...m.parts, { type: 'tool', call: ev.call }] }
      })
    case 'tool-input-delta':
      return mapMessage(state, ev.sessionId, ev.messageId, (m) => ({ ...m, parts: mapToolPart(m.parts, ev.toolCallId, (c) => ({ ...c, inputText: (c.inputText ?? '') + ev.text })) }))
    case 'tool-update': {
      const next = mapMessage(state, ev.sessionId, ev.messageId, (m) => ({ ...m, parts: mapToolPart(m.parts, ev.call.id, () => ev.call) }))
      if (ev.call.status !== 'awaiting-approval') {
        const stale = Object.values(next.approvals).filter((a) => a.toolCallId === ev.call.id || a.approvalId === ev.call.approval?.approvalId)
        if (stale.length) { const approvals = { ...next.approvals }; for (const a of stale) delete approvals[a.approvalId]; return { ...next, approvals } }
      }
      return next
    }
    case 'approval-request': {
      const approvals = { ...state.approvals, [ev.request.approvalId]: ev.request }
      const session = state.loaded[ev.sessionId]
      let next: ChatCoreState = { ...state, approvals }
      if (session) {
        const messages = session.messages.map((m) => m.parts.some((p) => p.type === 'tool' && p.call.id === ev.request.toolCallId)
          ? { ...m, parts: mapToolPart(m.parts, ev.request.toolCallId, (c) => ({ ...c, status: 'awaiting-approval', approval: ev.request })) }
          : m)
        const touched = session.messages.find((m) => m.parts.some((p) => p.type === 'tool' && p.call.id === ev.request.toolCallId))
        next = { ...next, loaded: { ...state.loaded, [ev.sessionId]: { ...session, messages, status: 'awaiting-approval' } }, versions: touched ? bump(state.versions, touched.id) : state.versions }
      }
      return { ...next, sessions: updateSummary(next.sessions, ev.sessionId, { status: 'awaiting-approval' }) }
    }
    case 'question': {
      const questions = { ...state.questions, [ev.question.questionId]: { ...ev.question, sessionId: ev.sessionId, messageId: ev.messageId } }
      const next = mapMessage({ ...state, questions }, ev.sessionId, ev.messageId, (m) => m.parts.some((p) => p.type === 'question' && p.question.questionId === ev.question.questionId) ? m : { ...m, parts: [...m.parts, { type: 'question', question: ev.question }] })
      return mapSession(next, ev.sessionId, (s) => ({ ...s, status: 'awaiting-input' }), { status: 'awaiting-input' })
    }
    case 'file-change':
      return mapSession(state, ev.sessionId, (s) => ({ ...s, pendingChanges: upsertChange(s.pendingChanges, ev.change) }))
    case 'file-change-status':
      return mapSession(state, ev.sessionId, (s) => ({ ...s, pendingChanges: s.pendingChanges.map((c) => (ev.changeIds.includes(c.changeId) ? { ...c, status: ev.status } : c)) }))
    case 'checkpoint':
      return mapMessage(state, ev.sessionId, ev.messageId, (m) => ({ ...m, checkpointId: ev.checkpointId }))
    case 'todos':
      return mapSession(state, ev.sessionId, (s) => ({ ...s, todos: ev.todos }))
    case 'usage': {
      const next = mapMessage(state, ev.sessionId, ev.messageId, (m) => ({ ...m, usage: ev.usage }))
      return mapSession(next, ev.sessionId, (s) => ({ ...s, usage: ev.sessionUsage, contextTokens: ev.contextTokens }))
    }
    case 'message-end': {
      const streaming = { ...state.streaming }
      if (streaming[ev.sessionId] === ev.messageId) delete streaming[ev.sessionId]
      const next = mapMessage({ ...state, streaming }, ev.sessionId, ev.messageId, (m) => ({ ...m, status: ev.status, durationMs: ev.durationMs, parts: ev.error && ev.status === 'error' && !m.parts.some((p) => p.type === 'error') ? [...m.parts, { type: 'error', message: ev.error }] : m.parts }))
      return mapSession(next, ev.sessionId, (s) => ({ ...s, updatedAt: Date.now() }))
    }
    case 'queue':
      return mapSession(state, ev.sessionId, (s) => ({ ...s, queuedMessages: ev.queued }))
    case 'summary':
      return mapSession(state, ev.sessionId, (s) => {
        const last = [...s.messages].reverse().find((m) => m.role === 'assistant')
        const part: MessagePart = { type: 'summary', text: ev.text }
        if (!last) return { ...s, summary: ev.text, messages: [...s.messages, { id: `summary-${Date.now()}`, role: 'system', parts: [part], createdAt: Date.now(), status: 'done' }] }
        return { ...s, summary: ev.text, messages: s.messages.map((m) => (m.id === last.id ? { ...m, parts: [...m.parts, part] } : m)) }
      })
    case 'plan': {
      const mid = state.streaming[ev.sessionId]
      if (mid) return mapMessage(state, ev.sessionId, mid, (m) => ({ ...m, parts: [...m.parts, { type: 'plan', path: ev.path, title: ev.title }] }))
      return mapSession(state, ev.sessionId, (s) => {
        const last = [...s.messages].reverse().find((m) => m.role === 'assistant')
        if (!last) return s
        return { ...s, messages: s.messages.map((m) => (m.id === last.id ? { ...m, parts: [...m.parts, { type: 'plan', path: ev.path, title: ev.title }] } : m)) }
      })
    }
    case 'error': {
      const mid = state.streaming[ev.sessionId]
      if (mid) return mapMessage(state, ev.sessionId, mid, (m) => ({ ...m, parts: [...m.parts, { type: 'error', message: ev.message }], status: 'error' }))
      return mapSession(state, ev.sessionId, (s) => ({ ...s, status: 'error', messages: [...s.messages, { id: `err-${Date.now()}`, role: 'assistant', parts: [{ type: 'error', message: ev.message }], createdAt: Date.now(), status: 'error' }] }), { status: 'error' })
    }
    default:
      return state
  }
}

/** Coalesce consecutive text/thinking deltas for the same message to reduce store writes. */
export function coalesceEvents(events: AgentEvent[]): AgentEvent[] {
  const out: AgentEvent[] = []
  for (const ev of events) {
    const prev = out[out.length - 1]
    if (prev && (ev.type === 'text-delta' || ev.type === 'thinking-delta') && prev.type === ev.type && prev.sessionId === ev.sessionId && prev.messageId === ev.messageId) {
      out[out.length - 1] = { ...prev, text: prev.text + ev.text }
    } else if (prev && ev.type === 'tool-input-delta' && prev.type === 'tool-input-delta' && prev.toolCallId === ev.toolCallId && prev.messageId === ev.messageId) {
      out[out.length - 1] = { ...prev, text: prev.text + ev.text }
    } else {
      out.push(ev)
    }
  }
  return out
}

export function applyAgentEvents(state: ChatCoreState, events: AgentEvent[]): ChatCoreState {
  return coalesceEvents(events).reduce(applyAgentEvent, state)
}

import { describe, expect, it } from 'vitest'
import { applyAgentEvent, applyAgentEvents, coalesceEvents, emptyCoreState, type ChatCoreState } from './chatReducer'
import type { AgentEvent, ChatSession, ToolCallRecord } from '@/types'

const session = (id = 's1'): ChatSession => ({ id, title: 'T', workspaceRoot: '/w', createdAt: 1, updatedAt: 1, mode: 'agent', messages: [], todos: [], usage: { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 }, pendingChanges: [], status: 'idle', queuedMessages: [], contextTokens: 0 })
const withSession = (): ChatCoreState => ({ ...emptyCoreState(), loaded: { s1: session() }, sessions: [{ id: 's1', title: 'T', workspaceRoot: '/w', createdAt: 1, updatedAt: 1, mode: 'agent', status: 'idle', messageCount: 0, lastMessagePreview: '', pendingChangeCount: 0 }] })
const start: AgentEvent = { type: 'message-start', sessionId: 's1', message: { id: 'm1', role: 'assistant', parts: [], createdAt: 2, status: 'streaming' } }
const call = (over: Partial<ToolCallRecord> = {}): ToolCallRecord => ({ id: 't1', name: 'read_file', displayName: 'Read file', input: { path: 'a.ts' }, status: 'running', ...over })

describe('coalesceEvents', () => {
  it('merges consecutive text deltas for the same message', () => {
    const out = coalesceEvents([
      { type: 'text-delta', sessionId: 's1', messageId: 'm1', text: 'Hel' },
      { type: 'text-delta', sessionId: 's1', messageId: 'm1', text: 'lo' },
      { type: 'thinking-delta', sessionId: 's1', messageId: 'm1', text: 'x' },
      { type: 'text-delta', sessionId: 's1', messageId: 'm1', text: '!' },
    ])
    expect(out).toHaveLength(3)
    expect(out[0]).toMatchObject({ type: 'text-delta', text: 'Hello' })
  })
  it('does not merge across messages', () => {
    const out = coalesceEvents([{ type: 'text-delta', sessionId: 's1', messageId: 'm1', text: 'a' }, { type: 'text-delta', sessionId: 's1', messageId: 'm2', text: 'b' }])
    expect(out).toHaveLength(2)
  })
})

describe('applyAgentEvent', () => {
  it('appends streamed text into a single text part and bumps the version', () => {
    let st = applyAgentEvents(withSession(), [start, { type: 'text-delta', sessionId: 's1', messageId: 'm1', text: 'Hello ' }, { type: 'text-delta', sessionId: 's1', messageId: 'm1', text: 'world' }])
    const msg = st.loaded.s1.messages[0]
    expect(msg.parts).toEqual([{ type: 'text', text: 'Hello world' }])
    expect(st.versions.m1).toBeGreaterThanOrEqual(2)
    expect(st.streaming.s1).toBe('m1')
    st = applyAgentEvent(st, { type: 'message-end', sessionId: 's1', messageId: 'm1', status: 'done', durationMs: 10 })
    expect(st.loaded.s1.messages[0].status).toBe('done')
    expect(st.streaming.s1).toBeUndefined()
    expect(st.sessions[0].messageCount).toBe(1)
    expect(st.sessions[0].lastMessagePreview).toBe('Hello world')
  })
  it('starts a new text part after a tool call', () => {
    const st = applyAgentEvents(withSession(), [start, { type: 'text-delta', sessionId: 's1', messageId: 'm1', text: 'a' }, { type: 'tool-start', sessionId: 's1', messageId: 'm1', call: call() }, { type: 'text-delta', sessionId: 's1', messageId: 'm1', text: 'b' }])
    expect(st.loaded.s1.messages[0].parts.map((p) => p.type)).toEqual(['text', 'tool', 'text'])
  })
  it('updates tool calls in place and accumulates input deltas', () => {
    let st = applyAgentEvents(withSession(), [start, { type: 'tool-start', sessionId: 's1', messageId: 'm1', call: call({ status: 'pending' }) }, { type: 'tool-input-delta', sessionId: 's1', messageId: 'm1', toolCallId: 't1', text: '{"pa' }, { type: 'tool-input-delta', sessionId: 's1', messageId: 'm1', toolCallId: 't1', text: 'th":1}' }])
    let part = st.loaded.s1.messages[0].parts[0]
    expect(part.type === 'tool' && part.call.inputText).toBe('{"path":1}')
    st = applyAgentEvent(st, { type: 'tool-update', sessionId: 's1', messageId: 'm1', call: call({ status: 'done', output: 'ok', exitCode: 0 }) })
    part = st.loaded.s1.messages[0].parts[0]
    expect(part.type === 'tool' && part.call.status).toBe('done')
    expect(st.loaded.s1.messages[0].parts).toHaveLength(1)
  })
  it('tracks approvals: request marks the tool awaiting, update clears it', () => {
    const req = { approvalId: 'ap1', sessionId: 's1', toolCallId: 't1', kind: 'command' as const, title: 'Run', detail: 'npm test', command: 'npm test', options: ['allow' as const, 'deny' as const] }
    let st = applyAgentEvents(withSession(), [start, { type: 'tool-start', sessionId: 's1', messageId: 'm1', call: call() }, { type: 'approval-request', sessionId: 's1', request: req }])
    expect(st.approvals.ap1).toBeDefined()
    expect(st.loaded.s1.status).toBe('awaiting-approval')
    const p = st.loaded.s1.messages[0].parts[0]
    expect(p.type === 'tool' && p.call.status).toBe('awaiting-approval')
    st = applyAgentEvent(st, { type: 'tool-update', sessionId: 's1', messageId: 'm1', call: call({ status: 'running', approval: req }) })
    expect(st.approvals.ap1).toBeUndefined()
  })
  it('records file changes and status transitions on the session', () => {
    const change = { changeId: 'c1', path: '/w/a.ts', kind: 'modify' as const, additions: 3, deletions: 1, status: 'pending' as const }
    let st = applyAgentEvent(withSession(), { type: 'file-change', sessionId: 's1', change })
    expect(st.loaded.s1.pendingChanges).toHaveLength(1)
    expect(st.sessions[0].pendingChangeCount).toBe(1)
    st = applyAgentEvent(st, { type: 'file-change', sessionId: 's1', change: { ...change, additions: 5 } })
    expect(st.loaded.s1.pendingChanges).toHaveLength(1)
    expect(st.loaded.s1.pendingChanges[0].additions).toBe(5)
    st = applyAgentEvent(st, { type: 'file-change-status', sessionId: 's1', changeIds: ['c1'], status: 'accepted' })
    expect(st.loaded.s1.pendingChanges[0].status).toBe('accepted')
    expect(st.sessions[0].pendingChangeCount).toBe(0)
  })
  it('handles questions, usage, todos, title and session lifecycle', () => {
    let st = applyAgentEvents(withSession(), [start, { type: 'question', sessionId: 's1', messageId: 'm1', question: { questionId: 'q1', question: 'Which?', options: ['a', 'b'], allowFreeText: true } }])
    expect(st.questions.q1.sessionId).toBe('s1')
    expect(st.loaded.s1.status).toBe('awaiting-input')
    st = applyAgentEvent(st, { type: 'usage', sessionId: 's1', messageId: 'm1', usage: { inputTokens: 10, outputTokens: 5, cacheReadTokens: 0, cacheWriteTokens: 0 }, contextTokens: 1234, sessionUsage: { inputTokens: 100, outputTokens: 50, cacheReadTokens: 0, cacheWriteTokens: 0 } })
    expect(st.loaded.s1.contextTokens).toBe(1234)
    expect(st.loaded.s1.messages[0].usage?.inputTokens).toBe(10)
    st = applyAgentEvent(st, { type: 'todos', sessionId: 's1', todos: [{ id: '1', content: 'x', status: 'pending' }] })
    expect(st.loaded.s1.todos).toHaveLength(1)
    st = applyAgentEvent(st, { type: 'title', sessionId: 's1', title: 'New title' })
    expect(st.sessions[0].title).toBe('New title')
    st = applyAgentEvent(st, { type: 'session-created', session: { id: 's2', title: 'B', workspaceRoot: '/w', createdAt: 3, updatedAt: 3, mode: 'ask', status: 'idle', messageCount: 0, lastMessagePreview: '', pendingChangeCount: 0 } })
    expect(st.sessions.map((s) => s.id)).toEqual(['s2', 's1'])
    st = applyAgentEvent(st, { type: 'session-deleted', sessionId: 's1' })
    expect(st.loaded.s1).toBeUndefined()
    expect(st.sessions.map((s) => s.id)).toEqual(['s2'])
  })
  it('ignores deltas for unknown sessions without throwing', () => {
    const st = applyAgentEvent(emptyCoreState(), { type: 'text-delta', sessionId: 'nope', messageId: 'm', text: 'x' })
    expect(st.loaded).toEqual({})
  })
  it('attaches errors to the streaming message', () => {
    const st = applyAgentEvents(withSession(), [start, { type: 'error', sessionId: 's1', message: 'boom' }])
    expect(st.loaded.s1.messages[0].parts).toEqual([{ type: 'error', message: 'boom' }])
    expect(st.loaded.s1.messages[0].status).toBe('error')
  })
})

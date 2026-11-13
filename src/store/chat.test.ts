import { describe, expect, it, vi } from 'vitest'
import type { AgentEvent, ChatSessionSummary } from '@/types'

vi.mock('@/lib/tauri', () => ({ Events: {}, on: () => () => {}, invoke: vi.fn(), channel: (cb: unknown) => cb }))
vi.mock('@/lib/api', () => ({ api: {} }))
vi.mock('@/lib/bus', () => ({ emit: () => {}, subscribe: () => () => {} }))

import { dropForeignSessionEvents, sessionBelongsHere } from './chat'

const summary = (id: string, workspaceRoot: string, scratch?: boolean): ChatSessionSummary => ({ id, title: id, workspaceRoot, createdAt: 1, updatedAt: 1, mode: 'agent', status: 'idle', messageCount: 0, lastMessagePreview: '', pendingChangeCount: 0, scratch })
const created = (s: ChatSessionSummary): AgentEvent => ({ type: 'session-created', session: s })

describe('sessionBelongsHere', () => {
  it('matches the thread folder against the window folder', () => {
    expect(sessionBelongsHere(summary('a', '/repo/a'), '/repo/a')).toBe(true)
    expect(sessionBelongsHere(summary('b', '/repo/b'), '/repo/a')).toBe(false)
  })
  it('shows folderless threads only in windows without a folder', () => {
    expect(sessionBelongsHere(summary('g', ''), null)).toBe(true)
    expect(sessionBelongsHere(summary('g', ''), undefined)).toBe(true)
    expect(sessionBelongsHere(summary('g', ''), '/repo/a')).toBe(false)
    expect(sessionBelongsHere(summary('a', '/repo/a'), null)).toBe(false)
  })
  it('lists scratch threads everywhere', () => {
    expect(sessionBelongsHere(summary('s', '/Users/me/.caret/scratch/s', true), '/repo/a')).toBe(true)
    expect(sessionBelongsHere(summary('s', '/Users/me/.caret/scratch/s', true), null)).toBe(true)
  })
})

describe('dropForeignSessionEvents', () => {
  it('drops session-created for threads of other folders and keeps everything else', () => {
    const events: AgentEvent[] = [
      created(summary('mine', '/repo/a')),
      created(summary('theirs', '/repo/b')),
      created(summary('scratch', '/scratch/x', true)),
      { type: 'session-status', sessionId: 'theirs', status: 'running' },
      { type: 'session-deleted', sessionId: 'theirs' },
    ]
    const out = dropForeignSessionEvents(events, '/repo/a')
    expect(out.map((e) => (e.type === 'session-created' ? e.session.id : e.type))).toEqual(['mine', 'scratch', 'session-status', 'session-deleted'])
  })
  it('returns the same array when no thread was created', () => {
    const events: AgentEvent[] = [{ type: 'session-status', sessionId: 'x', status: 'idle' }]
    expect(dropForeignSessionEvents(events, '/repo/a')).toBe(events)
  })
})

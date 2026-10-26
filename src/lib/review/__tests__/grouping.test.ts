import { describe, it, expect } from 'vitest'
import { computeGroups } from '../grouping'
import type { ChatMessage, FileChangeSummary } from '@/types'

const msgs: ChatMessage[] = [
  { id: 'u1', role: 'user', parts: [{ type: 'text', text: 'Fix the discount bug' }], createdAt: 1, turnId: 't1' },
  { id: 'a1', role: 'assistant', createdAt: 2, turnId: 't1', parts: [
    { type: 'text', text: '*[Proxy] banner*\n\nI will fix the discount calculation first.' },
    { type: 'tool', call: { id: 'c1', name: 'edit_file', displayName: 'Edit file', input: { path: 'src/pricing.ts' }, status: 'done' } },
    { type: 'text', text: 'Now adding a regression test.' },
    { type: 'tool', call: { id: 'c2', name: 'write_file', displayName: 'Write file', input: { path: 'tests/pricing.test.ts' }, status: 'done' } },
    { type: 'tool', call: { id: 'c3', name: 'edit_file', displayName: 'Edit file', input: { path: 'src/types.ts' }, status: 'done', summary: 'Export CartItem type' } },
  ] },
]
const changes: FileChangeSummary[] = [
  { changeId: 'x1', path: '/r/src/pricing.ts', kind: 'modify', additions: 1, deletions: 2, status: 'pending', toolCallId: 'c1', turnId: 't1' },
  { changeId: 'x2', path: '/r/tests/pricing.test.ts', kind: 'create', additions: 10, deletions: 0, status: 'pending', toolCallId: 'c2', turnId: 't1' },
  { changeId: 'x3', path: '/r/src/types.ts', kind: 'modify', additions: 3, deletions: 0, status: 'pending', toolCallId: 'c3', turnId: 't1' },
  { changeId: 'x4', path: '/r/README.md', kind: 'modify', additions: 2, deletions: 0, status: 'pending', toolCallId: 'c1', turnId: 't1' },
  { changeId: 'x5', path: '/r/src/old.ts', kind: 'modify', additions: 1, deletions: 1, status: 'accepted', toolCallId: 'c1', turnId: 't1' },
]

describe('computeGroups', () => {
  it('groups by intent with titles from preceding assistant text or tool summary, and splits ancillary kinds', () => {
    const g = computeGroups({ pendingChanges: changes, messages: msgs })
    expect(g.map((x) => x.title)).toEqual(['I will fix the discount calculation first.', 'Export CartItem type', 'Tests', 'Docs'])
    expect(g[0].changeIds).toEqual(['x1'])
    expect(g[1].changeIds).toEqual(['x3'])
    expect(g[2].kind).toBe('file-kind'); expect(g[2].collapsedByDefault).toBe(true); expect(g[2].changeIds).toEqual(['x2'])
    expect(g[3].changeIds).toEqual(['x4'])
    expect(g.flatMap((x) => x.changeIds)).not.toContain('x5')
  })
  it('returns no groups without pending changes and falls back to turn grouping', () => {
    expect(computeGroups({ pendingChanges: [], messages: msgs })).toEqual([])
    const g = computeGroups({ pendingChanges: [{ changeId: 'y', path: '/r/src/a.ts', kind: 'modify', additions: 1, deletions: 0, status: 'pending', turnId: 't1' }], messages: msgs })
    expect(g).toHaveLength(1); expect(g[0].kind).toBe('intent')
  })
})

import { describe, it, expect, vi, beforeEach } from 'vitest'

const calls: Record<string, unknown[]> = {}
vi.mock('@/lib/tauri', () => ({ Events: { reviewChanged: 'x' }, on: () => () => {}, invoke: vi.fn(), channel: (cb: unknown) => cb }))
vi.mock('@/lib/bus', () => ({ emit: (topic: string, p: unknown) => { (calls[topic] ??= []).push(p) }, subscribe: () => () => {} }))
vi.mock('@/lib/api', () => {
  const notImpl = () => Promise.reject(new Error('[internal] verification engine not implemented yet'))
  return {
    api: { ai: { reviews: () => Promise.resolve([]), review: notImpl, dismissReview: notImpl, dismissFinding: notImpl } },
    apiWave1: {
      verify: {
        run: (scope: unknown, onEvent: (e: unknown) => void) => {
          onEvent({ type: 'report', report: { id: 'r1', scope, sessionId: 's1', workspaceRoot: '/', startedAt: 1, status: 'running', checks: [{ id: 'c1', kind: 'tests', title: 'Tests', status: 'running', summary: '', findings: [] }], riskTier: 0, riskReasons: [], facts: [], claims: [], filesTouched: [], additions: 0, deletions: 0, summary: '' } })
          onEvent({ type: 'check', reportId: 'r1', check: { id: 'c1', kind: 'tests', title: 'Tests', status: 'fail', summary: '1 failing', findings: [], metrics: { passed: 3, failed: 1 } } })
          onEvent({ type: 'done', report: { id: 'r1', scope, sessionId: 's1', workspaceRoot: '/', startedAt: 1, finishedAt: 2, status: 'fail', checks: [{ id: 'c1', kind: 'tests', title: 'Tests', status: 'fail', summary: '1 failing', findings: [], metrics: { passed: 3, failed: 1 } }], riskTier: 2, riskReasons: ['tests failing'], facts: ['tests: 3 passed, 1 failed'], claims: [], filesTouched: [], additions: 0, deletions: 0, summary: '' } })
          return Promise.resolve('r1')
        },
        latestForSession: notImpl, list: notImpl, cancel: notImpl, applyFix: notImpl, get: notImpl,
      },
      review: { comments: notImpl, addComment: notImpl, resolveComment: notImpl, deleteComment: notImpl, sendComments: notImpl, groups: notImpl },
    },
  }
})

import { useReview } from './review'

describe('review store · verification', () => {
  beforeEach(() => { for (const k of Object.keys(calls)) delete calls[k] })
  it('streams report → check → done into state and publishes a summary', async () => {
    const id = await useReview.getState().runVerification('s1')
    expect(id).toBe('r1')
    const s = useReview.getState()
    expect(s.latestBySession.s1).toBe('r1')
    expect(s.reportsById.r1.status).toBe('fail')
    expect(s.reportsById.r1.checks[0].metrics?.failed).toBe(1)
    expect(s.verifying.s1).toBe(false)
    const summaries = calls['verify.summary'] as { riskTier: number; failing: boolean }[]
    expect(summaries.at(-1)).toMatchObject({ sessionId: 's1', riskTier: 2, failing: true })
  })
  it('degrades gracefully when the backend is missing', async () => {
    expect(await useReview.getState().loadLatest('s9')).toBeNull()
    await useReview.getState().loadReports()
    const c = await useReview.getState().addComment('s9', { path: '/a.ts', line: 3, side: 'after', text: 'why?' })
    expect(c.author).toBe('user'); expect(useReview.getState().comments.s9).toHaveLength(1)
    await useReview.getState().resolveComment('s9', c.id)
    expect(useReview.getState().comments.s9[0].resolved).toBe(true)
    const groups = await useReview.getState().loadGroups('s9', { pendingChanges: [{ changeId: 'x', path: '/r/src/a.ts', kind: 'modify', additions: 1, deletions: 0, status: 'pending', turnId: 't' }], messages: [] })
    expect(groups).toHaveLength(1)
  })
  it('ingests agent events', () => {
    useReview.getState().ingestAgentEvent({ type: 'review-comments', sessionId: 's2', comments: [{ id: 'k', path: '/a', line: 1, side: 'after', text: 't', createdAt: 1, author: 'bot' }] })
    expect(useReview.getState().comments.s2).toHaveLength(1)
  })
})

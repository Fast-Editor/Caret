import { describe, it, expect } from 'vitest'
import { createReloadScheduler, TIER_DELAY_MS, type ReloadTimer } from './workspaceReload'
import type { WorkspaceInfo } from '@/types'

/** Deterministic timer: `advance(ms)` fires due callbacks in order of their due time. */
function fakeTimer() {
  let nowMs = 0; let seq = 0
  const queue: { id: number; due: number; fn: () => void }[] = []
  const timer: ReloadTimer = {
    set: (fn, ms) => { const id = ++seq; queue.push({ id, due: nowMs + ms, fn }); return id },
    clear: (id) => { const i = queue.findIndex((q) => q.id === id); if (i >= 0) queue.splice(i, 1) },
  }
  const advance = (ms: number) => {
    const target = nowMs + ms
    for (;;) {
      queue.sort((a, b) => a.due - b.due)
      const next = queue[0]
      if (!next || next.due > target) break
      nowMs = next.due; queue.shift(); next.fn()
    }
    nowMs = target
  }
  return { timer, advance, queued: () => queue.length }
}

const ws = (root: string): WorkspaceInfo => ({ root, name: root.split('/').pop() ?? root, isGitRepo: true, hasCaretDir: false, hasCursorDir: false })

describe('workspace reload scheduler', () => {
  it('runs tiers in order: now synchronously, soon and later after their delays', () => {
    const t = fakeTimer(); const s = createReloadScheduler(t.timer)
    const log: string[] = []
    s.add('later', () => log.push('later'))
    s.add('now', () => log.push('now'))
    s.add('soon', () => log.push('soon'))
    s.dispatch(ws('/a'))
    expect(log).toEqual(['now'])
    expect(s.pending()).toBe(2)
    t.advance(TIER_DELAY_MS.soon)
    expect(log).toEqual(['now', 'soon'])
    t.advance(TIER_DELAY_MS.later - TIER_DELAY_MS.soon)
    expect(log).toEqual(['now', 'soon', 'later'])
    expect(s.pending()).toBe(0)
  })

  it('a rapid second change cancels the pending tiers of the first and delivers the new workspace', () => {
    const t = fakeTimer(); const s = createReloadScheduler(t.timer)
    const seen: string[] = []
    s.add('now', (w) => seen.push(`now:${w?.root}`))
    s.add('soon', (w) => seen.push(`soon:${w?.root}`))
    s.add('later', (w) => seen.push(`later:${w?.root}`))
    s.dispatch(ws('/first'))
    t.advance(50)
    s.dispatch(ws('/second'))
    t.advance(TIER_DELAY_MS.later + 10)
    expect(seen).toEqual(['now:/first', 'now:/second', 'soon:/second', 'later:/second'])
    expect(t.queued()).toBe(0)
  })

  it('handlers receive null when the workspace closes and unsubscribed handlers stop running', () => {
    const t = fakeTimer(); const s = createReloadScheduler(t.timer)
    const calls: (string | null)[] = []
    const off = s.add('soon', (w) => calls.push(w?.root ?? null))
    s.dispatch(null)
    t.advance(TIER_DELAY_MS.soon)
    expect(calls).toEqual([null])
    off()
    s.dispatch(ws('/x'))
    t.advance(TIER_DELAY_MS.later)
    expect(calls).toEqual([null])
  })

  it('a throwing handler does not stop the others in its tier', () => {
    const t = fakeTimer(); const s = createReloadScheduler(t.timer)
    const ran: string[] = []
    const origError = console.error; console.error = () => {}
    try {
      s.add('now', () => { throw new Error('boom') })
      s.add('now', () => ran.push('second'))
      s.dispatch(ws('/a'))
    } finally { console.error = origError }
    expect(ran).toEqual(['second'])
  })

  it('custom delays are honoured', () => {
    const t = fakeTimer(); const s = createReloadScheduler(t.timer, { now: 0, soon: 10, later: 20 })
    const log: string[] = []
    s.add('soon', () => log.push('soon')); s.add('later', () => log.push('later'))
    s.dispatch(ws('/a'))
    t.advance(9); expect(log).toEqual([])
    t.advance(1); expect(log).toEqual(['soon'])
    t.advance(10); expect(log).toEqual(['soon', 'later'])
  })
})

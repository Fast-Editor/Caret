import { describe, expect, it } from 'vitest'
import { contextLabel, toolGroup, toolTitle, fmtTokens, fmtDuration, sortContext, dayGroup, groupSessions, groupSessionsByProject, flattenThreadOrder, threadSection, snoozePresets, fmtWake, projectFoldKey } from './util'
import type { ChatSessionSummary, ContextItem } from '@/types'

describe('contextLabel', () => {
  it('labels files with ranges relative to the workspace', () => {
    expect(contextLabel({ id: '1', kind: 'file', label: 'a', path: '/w/src/a.ts' })).toBe('a.ts')
    expect(contextLabel({ id: '1', kind: 'selection', label: 'a', path: '/w/src/a.ts', range: { startLine: 3, endLine: 9 } })).toBe('a.ts:3-9')
    expect(contextLabel({ id: '1', kind: 'folder', label: 'src', path: '/w/src' }, '/w')).toBe('src/')
    expect(contextLabel({ id: '1', kind: 'git-branch-diff', label: '', base: 'develop' })).toBe('Diff vs develop')
    expect(contextLabel({ id: '1', kind: 'link', label: '', url: 'https://example.com/docs' })).toBe('example.com/docs')
  })
})

describe('tool helpers', () => {
  it('titles common tools', () => {
    expect(toolTitle({ id: 't', name: 'read_file', displayName: 'Read', input: { path: '/w/src/a.ts' }, status: 'done' })).toBe('Read a.ts')
    expect(toolTitle({ id: 't', name: 'run_terminal_cmd', displayName: 'Run', input: { command: 'npm test' }, status: 'done' })).toBe('Ran `npm test`')
    expect(toolTitle({ id: 't', name: 'codebase_search', displayName: 'Search', input: { query: 'auth flow' }, status: 'done' })).toBe('Searched codebase "auth flow"')
  })
  it('groups tools', () => {
    expect(toolGroup({ id: 't', name: 'edit_file', displayName: '', input: {}, status: 'done' })).toBe('edit')
    expect(toolGroup({ id: 't', name: 'x', displayName: '', input: {}, status: 'done', mcpServerId: 'srv' })).toBe('mcp')
    expect(toolGroup({ id: 't', name: 'browser_click', displayName: '', input: {}, status: 'done' })).toBe('browser')
  })
})

describe('formatters', () => {
  it('formats tokens and durations', () => {
    expect(fmtTokens(950)).toBe('950'); expect(fmtTokens(12_500)).toBe('13k'); expect(fmtTokens(2_400_000)).toBe('2.4M')
    expect(fmtDuration(850)).toBe('850ms'); expect(fmtDuration(4200)).toBe('4.2s'); expect(fmtDuration(125_000)).toBe('2m 5s')
  })
  it('sorts context pills by kind priority', () => {
    const items: ContextItem[] = [{ id: '1', kind: 'image', label: 'i' }, { id: '2', kind: 'selection', label: 's' }, { id: '3', kind: 'file', label: 'f' }]
    expect(sortContext(items).map((i) => i.kind)).toEqual(['selection', 'file', 'image'])
  })
})

// ───────────── thread lifecycle grouping (docs/THREADS.md) ─────────────
const base = (over: Partial<ChatSessionSummary> & { id: string }): ChatSessionSummary => ({ title: 'T', workspaceRoot: '/w', createdAt: 0, updatedAt: 0, mode: 'agent', status: 'idle', messageCount: 0, lastMessagePreview: '', pendingChangeCount: 0, ...over })
const H = 3_600_000
const D = 86_400_000
const NOW = new Date(2026, 10, 7, 12, 0, 0).getTime() // Sat 7 Nov 2026 12:00 local
const lifecycleFixture = (): ChatSessionSummary[] => [
  base({ id: 'old', updatedAt: NOW - 30 * D }),
  base({ id: 'today', updatedAt: NOW - H }),
  base({ id: 'pinned', pinned: true, pinnedAt: NOW - D, updatedAt: NOW - 3 * D }),
  base({ id: 'pinned2', pinned: true, pinnedAt: NOW - H, updatedAt: NOW - 10 * D }),
  base({ id: 'running', status: 'running', updatedAt: NOW - 2 * H }),
  base({ id: 'waiting', status: 'awaiting-approval', updatedAt: NOW - 5 * H }),
  base({ id: 'snoozed', snoozedUntil: NOW + 2 * H, updatedAt: NOW - H }),
  base({ id: 'snoozed-soon', snoozedUntil: NOW + H, updatedAt: NOW - 2 * H }),
  base({ id: 'expired-snooze', snoozedUntil: NOW - H, updatedAt: NOW - 1.5 * H }),
  base({ id: 'settled', settled: true, updatedAt: NOW - H }),
  base({ id: 'settled-pinned', settled: true, pinned: true, updatedAt: NOW - 2 * H }),
  base({ id: 'archived', archived: true, archivedAt: NOW - H, updatedAt: NOW - 2 * D }),
  base({ id: 'yesterday', updatedAt: NOW - D }),
  base({ id: 'week', updatedAt: NOW - 4 * D }),
  base({ id: 'sub', isSubagent: true, updatedAt: NOW }),
]

describe('threadSection', () => {
  it('ranks archived, working, snoozed, settled, pinned, then the day bucket', () => {
    expect(threadSection(base({ id: 'a', archived: true, status: 'running' }), { now: NOW })).toBe('archived')
    expect(threadSection(base({ id: 'a', status: 'running', snoozedUntil: NOW + H, pinned: true }), { now: NOW })).toBe('working')
    expect(threadSection(base({ id: 'a', snoozedUntil: NOW + H, settled: true, pinned: true }), { now: NOW })).toBe('snoozed')
    expect(threadSection(base({ id: 'a', settled: true, pinned: true }), { now: NOW })).toBe('settled')
    expect(threadSection(base({ id: 'a', pinned: true, updatedAt: NOW - 40 * D }), { now: NOW })).toBe('pinned')
    expect(threadSection(base({ id: 'a', updatedAt: NOW - 40 * D }), { now: NOW })).toBe('earlier')
    expect(threadSection(base({ id: 'a', snoozedUntil: NOW - 1, updatedAt: NOW }), { now: NOW })).toBe('today')
  })
})

describe('groupSessions', () => {
  it('orders sections Working, Pinned, day buckets, Snoozed, Settled and hides archived unless asked', () => {
    const secs = groupSessions(lifecycleFixture(), { now: NOW })
    expect(secs.map((s) => s.id)).toEqual(['working', 'pinned', 'today', 'yesterday', 'week', 'earlier', 'snoozed', 'settled'])
    const ids = Object.fromEntries(secs.map((s) => [s.id, s.sessions.map((x) => x.id)]))
    expect(ids.working).toEqual(['waiting', 'running'])
    expect(ids.pinned).toEqual(['pinned2', 'pinned'])
    expect(ids.today).toEqual(['today', 'expired-snooze'])
    expect(ids.yesterday).toEqual(['yesterday'])
    expect(ids.week).toEqual(['week'])
    expect(ids.earlier).toEqual(['old'])
    expect(ids.snoozed).toEqual(['snoozed-soon', 'snoozed'])
    expect(ids.settled).toEqual(['settled', 'settled-pinned'])
    const all = secs.flatMap((s) => s.sessions.map((x) => x.id))
    expect(all).not.toContain('sub')
    expect(all).not.toContain('archived')
    const withArchived = groupSessions(lifecycleFixture(), { now: NOW, showArchived: true })
    expect(withArchived[withArchived.length - 1]).toMatchObject({ id: 'archived', title: 'Archived', collapsed: false })
    expect(withArchived[withArchived.length - 1].sessions.map((x) => x.id)).toEqual(['archived'])
    expect(groupSessions([], { now: NOW })).toEqual([])
  })
  it('applies default and persisted fold state', () => {
    const byId = Object.fromEntries(groupSessions(lifecycleFixture(), { now: NOW }).map((s) => [s.id, s]))
    expect(byId.snoozed).toMatchObject({ collapsed: true, defaultCollapsed: true })
    expect(byId.settled).toMatchObject({ collapsed: true, defaultCollapsed: true })
    expect(byId.working).toMatchObject({ collapsed: false, defaultCollapsed: false })
    const folded = Object.fromEntries(groupSessions(lifecycleFixture(), { now: NOW, folds: { snoozed: false, today: true } }).map((s) => [s.id, s.collapsed]))
    expect(folded.snoozed).toBe(false)
    expect(folded.today).toBe(true)
    expect(folded.settled).toBe(true)
  })
  it('filters by title or preview and honours a custom busy predicate', () => {
    const list = [base({ id: 'a', title: 'Fix login', updatedAt: NOW }), base({ id: 'b', title: 'Other', lastMessagePreview: 'about LOGIN flows', updatedAt: NOW }), base({ id: 'c', title: 'Nope', updatedAt: NOW, status: 'error' })]
    expect(groupSessions(list, { now: NOW, query: ' login ' }).flatMap((s) => s.sessions.map((x) => x.id))).toEqual(['a', 'b'])
    const custom = groupSessions(list, { now: NOW, isBusy: (st) => st === 'error' })
    expect(custom[0].id).toBe('working')
    expect(custom[0].sessions.map((x) => x.id)).toEqual(['c'])
  })
  it('flattens the visible order and skips collapsed sections', () => {
    const order = flattenThreadOrder(groupSessions(lifecycleFixture(), { now: NOW, folds: { pinned: true } }))
    expect(order.slice(0, 2)).toEqual(['waiting', 'running'])
    expect(order).toContain('today')
    expect(order).not.toContain('pinned')
    expect(order).not.toContain('snoozed')
    expect(order).not.toContain('settled')
  })
})

describe('groupSessionsByProject', () => {
  const list = [
    base({ id: 'a1', workspaceRoot: '/code/alpha', updatedAt: NOW - 10 * D }),
    base({ id: 'b1', workspaceRoot: '/code/beta', updatedAt: NOW - H }),
    base({ id: 'b2', workspaceRoot: '/code/beta', status: 'running', updatedAt: NOW - 2 * H }),
    base({ id: 'g1', workspaceRoot: '', updatedAt: NOW - 5 * D }),
    base({ id: 'z1', workspaceRoot: '/code/zeta', archived: true, updatedAt: NOW }),
  ]
  it('puts the current workspace first, then most recent activity, with lifecycle sections inside', () => {
    const groups = groupSessionsByProject(list, '/code/alpha', { now: NOW })
    expect(groups.map((g) => g.title)).toEqual(['alpha', 'beta', 'No folder'])
    expect(groups[0]).toMatchObject({ current: true, root: '/code/alpha', count: 1, collapsed: false })
    expect(groups[1].sections.map((s) => s.id)).toEqual(['working', 'today'])
    expect(groups[1].count).toBe(2)
    expect(groupSessionsByProject(list, '/code/alpha', { now: NOW, showArchived: true }).map((g) => g.title)).toEqual(['alpha', 'zeta', 'beta', 'No folder'])
  })
  it('folds whole projects with the project:<root> key', () => {
    const folded = groupSessionsByProject(list, null, { now: NOW, folds: { [projectFoldKey('/code/beta')]: true } })
    expect(folded.map((g) => g.title)).toEqual(['beta', 'No folder', 'alpha'])
    expect(folded[0].collapsed).toBe(true)
    expect(flattenThreadOrder(folded)).toEqual(['g1', 'a1'])
  })
})

describe('snooze presets and wake labels', () => {
  it('computes presets relative to now', () => {
    const wed = new Date(2026, 10, 4, 10, 30).getTime() // Wed 4 Nov 2026
    const p = Object.fromEntries(snoozePresets(wed).map((x) => [x.id, x]))
    expect(p.hour.until).toBe(wed + H)
    expect(p.tonight.label).toBe('Tonight 18:00')
    expect(new Date(p.tonight.until).getDate()).toBe(4)
    expect(new Date(p.tonight.until).getHours()).toBe(18)
    expect(new Date(p.tomorrow.until).getDate()).toBe(5)
    expect(new Date(p.tomorrow.until).getHours()).toBe(9)
    expect(new Date(p.week.until).getDay()).toBe(1)
    expect(new Date(p.week.until).getDate()).toBe(9)
    const evening = new Date(2026, 10, 4, 19, 0).getTime()
    const q = Object.fromEntries(snoozePresets(evening).map((x) => [x.id, x]))
    expect(q.tonight.label).toBe('Tomorrow 18:00')
    expect(new Date(q.tonight.until).getDate()).toBe(5)
    const monday = new Date(2026, 10, 9, 9, 0).getTime()
    expect(new Date(Object.fromEntries(snoozePresets(monday).map((x) => [x.id, x])).week.until).getDate()).toBe(16)
  })
  it('formats wake times', () => {
    const now = new Date(2026, 10, 4, 10, 30).getTime()
    expect(fmtWake(now + 45 * 60_000, now)).toBe('in 45m')
    expect(fmtWake(new Date(2026, 10, 4, 18, 0).getTime(), now)).toBe('Today 18:00')
    expect(fmtWake(new Date(2026, 10, 5, 9, 0).getTime(), now)).toBe('Tomorrow 09:00')
    expect(fmtWake(new Date(2026, 10, 9, 9, 0).getTime(), now)).toBe('Mon 09:00')
    expect(fmtWake(now - 1, now)).toBe('now')
    expect(fmtWake(new Date(2026, 11, 24, 9, 0).getTime(), now)).toContain('09:00')
  })
  it('dayGroup accepts an injected clock', () => {
    expect(dayGroup(NOW - H, NOW)).toBe('Today')
    expect(dayGroup(NOW - D, NOW)).toBe('Yesterday')
    expect(dayGroup(NOW - 4 * D, NOW)).toBe('This week')
    expect(dayGroup(NOW - 30 * D, NOW)).toBe('Earlier')
  })
})

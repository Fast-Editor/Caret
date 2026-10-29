import { describe, expect, it } from 'vitest'
import { classifyChange, distanceFromBottom, followAction, isNearBottom, STICK_THRESHOLD } from './scrollFollow'

const m = (scrollTop: number, scrollHeight = 1000, clientHeight = 400) => ({ scrollTop, scrollHeight, clientHeight })

describe('scrollFollow', () => {
  it('measures the distance from the bottom and the stick threshold', () => {
    expect(distanceFromBottom(m(600))).toBe(0)
    expect(distanceFromBottom(m(500))).toBe(100)
    expect(isNearBottom(m(600))).toBe(true)
    expect(isNearBottom(m(600 - STICK_THRESHOLD + 1))).toBe(true)
    expect(isNearBottom(m(600 - STICK_THRESHOLD))).toBe(false)
    expect(isNearBottom(m(0))).toBe(false)
  })
  it('glides to the end on append only when pinned', () => {
    expect(followAction('append', true, m(600), 1000)).toEqual({ kind: 'bottom', smooth: true })
    expect(followAction('append', false, m(100), 1000)).toEqual({ kind: 'none' })
  })
  it('keeps the end in view instantly on streaming deltas and resizes', () => {
    expect(followAction('delta', true, m(600), 1000)).toEqual({ kind: 'bottom', smooth: false })
    expect(followAction('resize', true, m(600), 1000)).toEqual({ kind: 'bottom', smooth: false })
    expect(followAction('delta', false, m(100), 1000)).toEqual({ kind: 'none' })
  })
  it('jumps instantly when the session changes, pinned or not', () => {
    expect(followAction('session', false, m(100), 0)).toEqual({ kind: 'bottom', smooth: false })
  })
  it('anchors the viewport when content is revealed above', () => {
    expect(followAction('prepend', false, m(120, 1800), 1000)).toEqual({ kind: 'anchor', top: 920 })
    expect(followAction('prepend', true, m(120, 1000), 1000)).toEqual({ kind: 'none' })
  })
  it('classifies transcript changes', () => {
    const base = { sessionId: 's1', lastId: 'm3', count: 3, start: 0 }
    expect(classifyChange(base, { ...base, sessionId: 's2' })).toBe('session')
    expect(classifyChange(base, { ...base, lastId: 'm4', count: 4 })).toBe('append')
    expect(classifyChange(base, { ...base, lastId: 'm3b' })).toBe('append') // regenerate replaces the tail
    expect(classifyChange({ ...base, count: 200, start: 120 }, { ...base, count: 200, start: 40 })).toBe('prepend')
    expect(classifyChange(base, base)).toBeNull()
    expect(classifyChange(base, { ...base, count: 2, lastId: 'm2' })).toBe('append') // tail replaced
  })
})

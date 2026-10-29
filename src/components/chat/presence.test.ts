import { describe, expect, it } from 'vitest'
import { mergePresence, removedBetween } from './presence'

type P = { id: string; label: string }
const key = (p: P) => p.id
const a = { id: 'a', label: 'A' }, b = { id: 'b', label: 'B' }, c = { id: 'c', label: 'C' }

describe('presence', () => {
  it('reports removed items with the index they held', () => {
    const removed = removedBetween([a, b, c], [a, c], key)
    expect([...removed.keys()]).toEqual(['b'])
    expect(removed.get('b')).toEqual({ item: b, index: 1 })
    expect(removedBetween([a], [a, b], key).size).toBe(0)
  })
  it('re-inserts leaving items at their old position, flagged leaving', () => {
    const leaving = new Map([['b', { item: b, index: 1 }]])
    expect(mergePresence([a, c], leaving, key)).toEqual([
      { key: 'a', item: a, leaving: false }, { key: 'b', item: b, leaving: true }, { key: 'c', item: c, leaving: false },
    ])
  })
  it('clamps the index when the list shrank below it and never duplicates a live key', () => {
    const leaving = new Map([['c', { item: c, index: 7 }], ['a', { item: a, index: 0 }]])
    const out = mergePresence([a], leaving, key)
    expect(out.map((e) => e.key)).toEqual(['a', 'c'])
    expect(out[0].leaving).toBe(false)
    expect(out[1].leaving).toBe(true)
  })
  it('is the identity when nothing is leaving', () => {
    expect(mergePresence([a, b], new Map(), key).map((e) => e.key)).toEqual(['a', 'b'])
  })
})

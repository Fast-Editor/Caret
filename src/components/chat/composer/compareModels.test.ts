import { describe, expect, it } from 'vitest'
import { compareColumns, isFanOut, pruneCompare, sendLabel, toggleCompareModel } from './compareModels'

describe('toggleCompareModel', () => {
  it('seeds the set with the primary model on the first toggle', () => {
    expect(toggleCompareModel([], 'gpt-5', 'claude-opus-5-5')).toEqual(['claude-opus-5-5', 'gpt-5'])
  })
  it('does not seed with auto or with the same model', () => {
    expect(toggleCompareModel([], 'gpt-5', 'auto')).toEqual(['gpt-5'])
    expect(toggleCompareModel([], 'gpt-5', 'gpt-5')).toEqual(['gpt-5'])
  })
  it('removes an already selected model and ignores auto', () => {
    expect(toggleCompareModel(['a', 'b'], 'a', 'a')).toEqual(['b'])
    expect(toggleCompareModel(['a', 'b'], 'auto', 'a')).toEqual(['a', 'b'])
  })
  it('caps the selection', () => {
    expect(toggleCompareModel(['a', 'b', 'c', 'd'], 'e', 'a')).toEqual(['a', 'b', 'c', 'd'])
    expect(toggleCompareModel(['a', 'b'], 'c', 'a', 2)).toEqual(['a', 'b'])
  })
})

describe('fan-out helpers', () => {
  it('needs two distinct concrete models', () => {
    expect(isFanOut(undefined)).toBe(false)
    expect(isFanOut(['a'])).toBe(false)
    expect(isFanOut(['a', 'a'])).toBe(false)
    expect(isFanOut(['a', 'auto'])).toBe(false)
    expect(isFanOut(['a', 'b'])).toBe(true)
  })
  it('labels the send button', () => {
    expect(sendLabel(['a'])).toBe('Send')
    expect(sendLabel(['a', 'b', 'c'])).toBe('Send to 3')
  })
  it('prunes models that are no longer available', () => {
    expect(pruneCompare(['a', 'gone', 'b'], ['a', 'b', 'c'])).toEqual(['a', 'b'])
  })
})

describe('compareColumns', () => {
  const g = [{ id: 'x', createdAt: 3 }, { id: 'y', createdAt: 1 }, { id: 'z', createdAt: 2 }, { id: 'w', createdAt: 4 }, { id: 'v', createdAt: 5 }]
  it('puts the active session first, then creation order, capped at the max', () => {
    expect(compareColumns(g, 'z').map((s) => s.id)).toEqual(['z', 'y', 'x', 'w'])
    expect(compareColumns(g, null, 2).map((s) => s.id)).toEqual(['y', 'z'])
    expect(compareColumns(g, 'nope').map((s) => s.id)).toEqual(['y', 'z', 'x', 'w'])
  })
})

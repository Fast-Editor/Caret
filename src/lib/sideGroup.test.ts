import { describe, it, expect } from 'vitest'
import { sideGroupIndex } from './sideGroup'

describe('sideGroupIndex', () => {
  it('needs a split when there is one group', () => { expect(sideGroupIndex(1, 0)).toBeNull(); expect(sideGroupIndex(0, 0)).toBeNull() })
  it('prefers the group to the right, else the one to the left', () => {
    expect(sideGroupIndex(2, 0)).toBe(1)
    expect(sideGroupIndex(2, 1)).toBe(0)
    expect(sideGroupIndex(3, 1)).toBe(2)
    expect(sideGroupIndex(3, 2)).toBe(1)
  })
  it('falls back to the first group for an unknown active index', () => { expect(sideGroupIndex(2, -1)).toBe(0); expect(sideGroupIndex(2, 5)).toBe(0) })
})

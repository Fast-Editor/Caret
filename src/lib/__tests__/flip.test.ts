import { describe, expect, it } from 'vitest'
import { flipDeltas } from '../flip'

describe('flipDeltas', () => {
  it('returns the offset from new to old position for keys present on both sides', () => {
    expect(flipDeltas({ a: 0, b: 120, c: 240 }, { b: 0, a: 120, c: 240 })).toEqual({ a: -120, b: 120 })
  })
  it('ignores sub-pixel noise and keys that were added or removed', () => {
    expect(flipDeltas({ a: 0, gone: 50 }, { a: 0.3, added: 10 })).toEqual({})
  })
})

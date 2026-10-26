import { describe, it, expect } from 'vitest'
import { hashLine, makeAnchor, reanchor } from '../anchor'

describe('comment anchoring', () => {
  const lines = ['import x', '', 'export function a() {', '  return 1', '}', '', 'export function b() {', '  return 2', '}']
  it('keeps the line when unchanged and re-anchors after insertions', () => {
    const a = makeAnchor(lines, 4)
    expect(reanchor(lines, a)).toBe(4)
    const shifted = ['// header', '// more', ...lines]
    expect(reanchor(shifted, a)).toBe(6)
  })
  it('ignores whitespace differences and returns null when the line is gone', () => {
    expect(hashLine('  return   1 ')).toBe(hashLine('return 1'))
    const a = makeAnchor(lines, 4)
    expect(reanchor(lines.map((l) => (l === '  return 1' ? '  return 42' : l)), a)).toBeNull()
  })
})

import { describe, it, expect } from 'vitest'
import { trimSuffixOverlap, isUselessCompletion, cleanCompletion, limitLines, inLineComment, contextWindows } from '../monaco/completionUtils'
import { lineDiff } from '../monaco/lineDiff'

describe('completion post-processing', () => {
  it('trims suffix overlap on the first line', () => {
    expect(trimSuffixOverlap('foo(bar))', ')')).toBe('foo(bar)')
    expect(trimSuffixOverlap('x = 1;\nreturn x;', ';')).toBe('x = 1\nreturn x;')
    expect(trimSuffixOverlap('abc', 'xyz')).toBe('abc')
  })
  it('detects useless completions', () => {
    expect(isUselessCompletion('   ', 'const a')).toBe(true)
    expect(isUselessCompletion('a', 'const a')).toBe(true)
    expect(isUselessCompletion(' = 1', 'const a')).toBe(false)
  })
  it('cleans fences and tags', () => {
    expect(cleanCompletion('```ts\nlet a = 1\n```')).toBe('let a = 1')
    expect(cleanCompletion('<completion>x</completion>')).toBe('x')
    expect(cleanCompletion('a\n\n\n')).toBe('a\n')
  })
  it('limits lines and detects comments', () => {
    expect(limitLines('1\n2\n3\n4', 2)).toBe('1\n2')
    expect(inLineComment('const a = 1 // note', '//')).toBe(true)
    expect(inLineComment('const s = "// not"', '//')).toBe(false)
    expect(inLineComment('x = 1 # c', '#')).toBe(true)
  })
  it('builds context windows', () => {
    const { prefix, suffix } = contextWindows(['a', 'bc', 'def', 'g'], 2, 2, 1, 1)
    expect(prefix).toBe('a\nb'); expect(suffix).toBe('c\ndef')
  })
})

describe('lineDiff', () => {
  it('finds added/modified/deleted hunks', () => {
    expect(lineDiff(['a', 'b', 'c'], ['a', 'b', 'c'])).toEqual([])
    expect(lineDiff(['a', 'c'], ['a', 'b', 'c'])).toEqual([{ kind: 'added', start: 2, end: 2 }])
    expect(lineDiff(['a', 'b', 'c'], ['a', 'c'])).toEqual([{ kind: 'deleted', start: 1, end: 1 }])
    expect(lineDiff(['a', 'b', 'c'], ['a', 'B', 'c'])).toEqual([{ kind: 'modified', start: 2, end: 2 }])
    expect(lineDiff([], ['x', 'y'])).toEqual([{ kind: 'added', start: 1, end: 2 }])
  })
})

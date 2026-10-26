import { describe, expect, it } from 'vitest'
import { diffStats, parseCodeHeader, parsePathRef, parseUnifiedDiff } from './parse'

describe('parseCodeHeader', () => {
  it('reads language from className', () => {
    expect(parseCodeHeader('language-ts', 'x').language).toBe('typescript')
  })
  it('reads lang:path info strings', () => {
    const r = parseCodeHeader('language-ts:src/a.ts', 'const a = 1')
    expect(r).toMatchObject({ language: 'typescript', path: 'src/a.ts', code: 'const a = 1' })
  })
  it('treats a bare path info string as the path and infers the language', () => {
    expect(parseCodeHeader('language-src/lib/x.py', 'print(1)')).toMatchObject({ language: 'python', path: 'src/lib/x.py' })
  })
  it('extracts a first-line comment path and strips it', () => {
    const r = parseCodeHeader('language-typescript', '// src/components/App.tsx\nexport {}')
    expect(r).toMatchObject({ path: 'src/components/App.tsx', code: 'export {}' })
    const py = parseCodeHeader(undefined, '# scripts/run.py:12\nprint()')
    expect(py).toMatchObject({ path: 'scripts/run.py', startLine: 12, language: 'python' })
  })
  it('leaves ordinary comments alone', () => {
    expect(parseCodeHeader('language-js', '// helper\nx()').path).toBeUndefined()
  })
})

describe('parsePathRef', () => {
  it('matches file paths with optional line', () => {
    expect(parsePathRef('src/a.ts')).toEqual({ path: 'src/a.ts', line: undefined })
    expect(parsePathRef('src/a.ts:42')).toEqual({ path: 'src/a.ts', line: 42 })
    expect(parsePathRef('a.ts:42:3')).toEqual({ path: 'a.ts', line: 42 })
  })
  it('rejects versions, words and expressions', () => {
    expect(parsePathRef('1.2.3')).toBeNull()
    expect(parsePathRef('hello')).toBeNull()
    expect(parsePathRef('foo.bar()')).toBeNull()
  })
})

describe('parseUnifiedDiff', () => {
  const diff = `--- a/x.ts\n+++ b/x.ts\n@@ -1,3 +1,3 @@\n a\n-b\n+c\n d`
  it('assigns line numbers', () => {
    const lines = parseUnifiedDiff(diff).filter((l) => l.kind !== 'meta' && l.kind !== 'hunk')
    expect(lines.map((l) => [l.kind, l.oldNo, l.newNo])).toEqual([['ctx', 1, 1], ['del', 2, undefined], ['add', undefined, 2], ['ctx', 3, 3]])
  })
  it('computes stats', () => { expect(diffStats(diff)).toEqual({ additions: 1, deletions: 1 }) })
})

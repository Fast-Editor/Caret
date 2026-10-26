import { describe, it, expect } from 'vitest'
import { parseQuickOpenInput, fuzzyScore } from '@/components/palette/parse'

describe('quick open input parsing', () => {
  it('detects prefixes', () => {
    expect(parseQuickOpenInput('>reload').mode).toBe('commands')
    expect(parseQuickOpenInput('@sym')).toMatchObject({ mode: 'symbols', query: 'sym' })
    expect(parseQuickOpenInput('#todo')).toMatchObject({ mode: 'search', query: 'todo' })
    expect(parseQuickOpenInput(':42')).toMatchObject({ mode: 'line', line: 42 })
    expect(parseQuickOpenInput('').mode).toBe('recent')
  })
  it('parses file:line:col', () => {
    expect(parseQuickOpenInput('src/app.tsx:12:3')).toMatchObject({ mode: 'files', query: 'src/app.tsx', line: 12, column: 3 })
    expect(parseQuickOpenInput('main.rs:7')).toMatchObject({ mode: 'files', query: 'main.rs', line: 7 })
    expect(parseQuickOpenInput('readme')).toMatchObject({ mode: 'files', query: 'readme' })
  })
  it('fuzzy scores subsequences with word-start bonus', () => {
    expect(fuzzyScore('ap', 'src/App.tsx')).not.toBeNull()
    expect(fuzzyScore('zzz', 'src/App.tsx')).toBeNull()
    expect(fuzzyScore('app', 'src/App.tsx')!.score).toBeGreaterThan(fuzzyScore('app', 'wrapper.ts')!.score)
  })
})

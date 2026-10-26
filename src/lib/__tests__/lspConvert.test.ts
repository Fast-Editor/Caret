import { describe, it, expect } from 'vitest'
import { toMonacoRange, toLspRange, pathToUri, uriToPath, frameMessage, parseFrames, hoverToMarkdown, COMPLETION_KIND_MAP, SEVERITY_MAP } from '../lsp/convert'
import { applyTextEdits } from '../lsp/providers-pure'

describe('lsp conversions', () => {
  it('ranges round-trip (0-based ↔ 1-based)', () => {
    const l = { start: { line: 2, character: 4 }, end: { line: 3, character: 0 } }
    const m = toMonacoRange(l)
    expect(m).toEqual({ startLineNumber: 3, startColumn: 5, endLineNumber: 4, endColumn: 1 })
    expect(toLspRange(m)).toEqual(l)
  })
  it('uri ↔ path incl. spaces and unicode', () => {
    const p = '/Users/me/My Project/ü.ts'
    const u = pathToUri(p)
    expect(u.startsWith('file:///Users/me/My%20Project/')).toBe(true)
    expect(uriToPath(u)).toBe(p)
    expect(uriToPath('file:///c:/x/y.ts')).toBe('c:/x/y.ts')
  })
  it('frames and parses Content-Length messages (multi-byte safe)', () => {
    const a = JSON.stringify({ jsonrpc: '2.0', id: 1, result: 'é' })
    const b = JSON.stringify({ jsonrpc: '2.0', method: 'x' })
    const { messages, rest } = parseFrames(frameMessage(a) + frameMessage(b) + 'Content-Length: 5\r\n\r\n{"')
    expect(messages).toEqual([a, b])
    expect(rest).toBe('Content-Length: 5\r\n\r\n{"')
  })
  it('hover contents normalise', () => {
    expect(hoverToMarkdown('x')).toEqual(['x'])
    expect(hoverToMarkdown({ kind: 'markdown', value: '**b**' })).toEqual(['**b**'])
    expect(hoverToMarkdown([{ language: 'ts', value: 'let a' }, ''])).toEqual(['```ts\nlet a\n```'])
  })
  it('kind maps are complete', () => { for (let k = 1; k <= 25; k++) expect(COMPLETION_KIND_MAP[k]).toBeTypeOf('number'); expect(SEVERITY_MAP[1]).toBe(8) })
  it('applies text edits bottom-up', () => {
    const text = 'a\nbb\nccc\n'
    const out = applyTextEdits(text, [{ range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } }, newText: 'A' }, { range: { start: { line: 2, character: 1 }, end: { line: 2, character: 2 } }, newText: 'XX' }])
    expect(out).toBe('A\nbb\ncXXc\n')
  })
})

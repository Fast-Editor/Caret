/** Pure conversions between LSP and Monaco shapes (no Monaco import → testable). */
import type { LspPosition, LspRange } from '@/types'

export interface MRange { startLineNumber: number; startColumn: number; endLineNumber: number; endColumn: number }
export interface MPosition { lineNumber: number; column: number }

export const toMonacoRange = (r: LspRange): MRange => ({ startLineNumber: r.start.line + 1, startColumn: r.start.character + 1, endLineNumber: r.end.line + 1, endColumn: r.end.character + 1 })
export const toLspRange = (r: MRange): LspRange => ({ start: { line: r.startLineNumber - 1, character: r.startColumn - 1 }, end: { line: r.endLineNumber - 1, character: r.endColumn - 1 } })
export const toMonacoPosition = (p: LspPosition): MPosition => ({ lineNumber: p.line + 1, column: p.character + 1 })
export const toLspPosition = (p: MPosition): LspPosition => ({ line: p.lineNumber - 1, character: p.column - 1 })

export function pathToUri(path: string): string {
  if (path.startsWith('file://')) return path
  const p = path.startsWith('/') ? path : '/' + path
  return 'file://' + encodeURI(p).replace(/[?#]/g, encodeURIComponent)
}
export function uriToPath(uri: string): string {
  if (!uri.startsWith('file://')) return uri
  let p = uri.slice('file://'.length)
  if (p.startsWith('localhost/')) p = p.slice('localhost'.length)
  try { p = decodeURIComponent(p) } catch { /* keep raw */ }
  // windows: /c:/foo → c:/foo
  if (/^\/[A-Za-z]:\//.test(p)) p = p.slice(1)
  return p
}

/** LSP CompletionItemKind (1..25) → Monaco CompletionItemKind numbers (Monaco enum values). */
export const COMPLETION_KIND_MAP: Record<number, number> = {
  1: 18 /*Text*/, 2: 0 /*Method*/, 3: 1 /*Function*/, 4: 2 /*Constructor*/, 5: 3 /*Field*/, 6: 4 /*Variable*/, 7: 5 /*Class*/, 8: 7 /*Interface*/, 9: 8 /*Module*/, 10: 9 /*Property*/,
  11: 12 /*Unit*/, 12: 13 /*Value*/, 13: 15 /*Enum*/, 14: 17 /*Keyword*/, 15: 27 /*Snippet*/, 16: 19 /*Color*/, 17: 20 /*File*/, 18: 21 /*Reference*/, 19: 23 /*Folder*/, 20: 16 /*EnumMember*/,
  21: 14 /*Constant*/, 22: 6 /*Struct*/, 23: 10 /*Event*/, 24: 11 /*Operator*/, 25: 24 /*TypeParameter*/,
}
/** LSP SymbolKind (1..26) → Monaco SymbolKind (0-based enum). */
export const SYMBOL_KIND_MAP: Record<number, number> = {
  1: 0, 2: 1, 3: 2, 4: 3, 5: 4, 6: 5, 7: 6, 8: 7, 9: 8, 10: 9, 11: 10, 12: 11, 13: 12, 14: 13, 15: 14, 16: 15, 17: 16, 18: 17, 19: 18, 20: 19, 21: 20, 22: 21, 23: 22, 24: 23, 25: 24, 26: 25,
}
export function symbolKindName(kind: number): string {
  return ['File', 'Module', 'Namespace', 'Package', 'Class', 'Method', 'Property', 'Field', 'Constructor', 'Enum', 'Interface', 'Function', 'Variable', 'Constant', 'String', 'Number', 'Boolean', 'Array', 'Object', 'Key', 'Null', 'EnumMember', 'Struct', 'Event', 'Operator', 'TypeParameter'][kind - 1] ?? 'Symbol'
}

/** LSP DiagnosticSeverity (1..4) → Monaco MarkerSeverity (8,4,2,1). */
export const SEVERITY_MAP: Record<number, number> = { 1: 8, 2: 4, 3: 2, 4: 1 }

export function hoverToMarkdown(contents: unknown): string[] {
  const arr = Array.isArray(contents) ? contents : [contents]
  const out: string[] = []
  for (const c of arr) {
    if (!c) continue
    if (typeof c === 'string') out.push(c)
    else if (typeof c === 'object' && 'kind' in (c as object)) { const m = c as { kind: string; value: string }; out.push(m.kind === 'plaintext' ? '```text\n' + m.value + '\n```' : m.value) }
    else if (typeof c === 'object' && 'language' in (c as object)) { const m = c as { language: string; value: string }; out.push('```' + m.language + '\n' + m.value + '\n```') }
    else if (typeof c === 'object' && 'value' in (c as object)) out.push(String((c as { value: string }).value))
  }
  return out.filter((s) => s.trim().length)
}
export function docToMarkdown(doc: unknown): string | undefined {
  if (!doc) return undefined
  if (typeof doc === 'string') return doc
  if (typeof doc === 'object' && 'value' in (doc as object)) return String((doc as { value: string }).value)
  return undefined
}

/** Frame a JSON-RPC message with LSP Content-Length header (server side relays raw JSON, but tests cover framing). */
export function frameMessage(json: string): string { return `Content-Length: ${new TextEncoder().encode(json).length}\r\n\r\n${json}` }
/** Parse concatenated framed messages; returns parsed bodies and the remaining buffer. */
export function parseFrames(buffer: string): { messages: string[]; rest: string } {
  const messages: string[] = []
  let rest = buffer
  for (;;) {
    const m = /^Content-Length: (\d+)\r\n(?:[^\r\n]+\r\n)*\r\n/.exec(rest)
    if (!m) break
    const len = Number(m[1]); const bodyStart = m[0].length
    const bytes = new TextEncoder().encode(rest.slice(bodyStart))
    if (bytes.length < len) break
    const body = new TextDecoder().decode(bytes.slice(0, len))
    messages.push(body)
    rest = new TextDecoder().decode(bytes.slice(len))
  }
  return { messages, rest }
}

/** Monaco content change (range in 1-based + text) → LSP incremental change. */
export function toLspContentChange(c: { range: MRange; text: string }): { range: LspRange; text: string } { return { range: toLspRange(c.range), text: c.text } }

export type QuickInputMode = 'files' | 'commands' | 'symbols' | 'search' | 'line' | 'recent'
export interface ParsedQuickInput { mode: QuickInputMode; query: string; line?: number; column?: number }

/** VS Code-style quick input prefixes: ">" commands, "@" symbols, "#" search, ":" go to line, "file:12:3". */
export function parseQuickOpenInput(raw: string): ParsedQuickInput {
  const input = raw.replace(/^\s+/, '')
  if (input.startsWith('>')) return { mode: 'commands', query: input.slice(1).trimStart() }
  if (input.startsWith('@')) return { mode: 'symbols', query: input.slice(1).trimStart() }
  if (input.startsWith('#')) return { mode: 'search', query: input.slice(1).trimStart() }
  if (/^:\s*\d*$/.test(input)) { const n = parseInt(input.slice(1), 10); return { mode: 'line', query: '', line: Number.isFinite(n) ? n : undefined } }
  const m = /^(.*?):(\d+)(?::(\d+))?\s*$/.exec(input)
  if (m && m[1].trim()) return { mode: 'files', query: m[1].trim(), line: parseInt(m[2], 10), column: m[3] ? parseInt(m[3], 10) : undefined }
  return { mode: input.trim() ? 'files' : 'recent', query: input.trim() }
}

/** Simple subsequence fuzzy score with contiguous/word-start bonuses; returns null when no match. */
export function fuzzyScore(query: string, text: string): { score: number; indices: number[] } | null {
  if (!query) return { score: 0, indices: [] }
  const q = query.toLowerCase(); const t = text.toLowerCase()
  let qi = 0; let score = 0; let last = -2; const indices: number[] = []
  for (let i = 0; i < t.length && qi < q.length; i++) {
    if (t[i] !== q[qi]) continue
    let s = 1
    if (i === last + 1) s += 2
    if (i === 0 || /[\s/_\-.]/.test(t[i - 1])) s += 3
    if (text[i] !== t[i]) s += 1 // camelCase boundary
    score += s; last = i; indices.push(i); qi++
  }
  if (qi < q.length) return null
  score -= Math.max(0, (indices[indices.length - 1] - indices[0] + 1 - q.length)) * 0.1
  return { score, indices }
}

/** Palette inputs carry a mode prefix (`>` commands, `@` symbols, `#` search). Matching must score only the text after it. */
export function stripPalettePrefix(search: string): string {
  return search.replace(/^[>@#]\s*/, '')
}

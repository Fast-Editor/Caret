/** Pure helpers for Tab completions (testable without Monaco). */

/** Remove the part of `completion` that duplicates what already follows the cursor on the same line. */
export function trimSuffixOverlap(completion: string, lineSuffix: string): string {
  if (!completion || !lineSuffix) return completion
  const firstLineEnd = completion.indexOf('\n')
  const first = firstLineEnd >= 0 ? completion.slice(0, firstLineEnd) : completion
  const rest = firstLineEnd >= 0 ? completion.slice(firstLineEnd) : ''
  const suffix = lineSuffix.trimEnd()
  // find the longest tail of `first` that is a prefix of `suffix`
  for (let k = Math.min(first.length, suffix.length); k > 0; k--) {
    if (first.endsWith(suffix.slice(0, k))) return first.slice(0, first.length - k) + rest
  }
  return completion
}

/** Drop a completion that merely repeats the current line prefix or is whitespace. */
export function isUselessCompletion(completion: string, linePrefix: string): boolean {
  const c = completion.trim()
  if (!c) return true
  if (linePrefix.trim() && linePrefix.trim().endsWith(c)) return true
  return false
}

/** Strip code fences / tags a model may wrap the completion in. */
export function cleanCompletion(raw: string): string {
  let s = raw.replace(/\r\n/g, '\n')
  const tag = /<completion>([\s\S]*?)<\/completion>/.exec(s)
  if (tag) s = tag[1]
  const fence = /^\s*```[\w+-]*\n([\s\S]*?)\n?```\s*$/.exec(s)
  if (fence) s = fence[1]
  return s.replace(/\n+$/, (m) => (m.length > 1 ? '\n' : m))
}

/** Limit to `maxLines` lines (keeps a trailing newline if cut). */
export function limitLines(s: string, maxLines: number): string {
  const lines = s.split('\n')
  if (lines.length <= maxLines) return s
  return lines.slice(0, maxLines).join('\n')
}

/** True when the cursor position is inside a line comment given the line prefix. */
export function inLineComment(linePrefix: string, commentPrefix: string | null): boolean {
  if (!commentPrefix) return false
  // crude but fast: ignore comment markers inside strings
  let inStr: string | null = null
  for (let i = 0; i < linePrefix.length; i++) {
    const ch = linePrefix[i]
    if (inStr) { if (ch === '\\') i++; else if (ch === inStr) inStr = null; continue }
    if (ch === '"' || ch === "'" || ch === '`') { inStr = ch; continue }
    if (linePrefix.startsWith(commentPrefix, i)) return true
  }
  return false
}

/** Build prefix/suffix windows around a cursor from an array of lines (1-based line, 1-based column). */
export function contextWindows(lines: string[], line: number, column: number, before: number, after: number): { prefix: string; suffix: string } {
  const start = Math.max(0, line - 1 - before)
  const end = Math.min(lines.length, line + after)
  const cur = lines[line - 1] ?? ''
  const prefix = [...lines.slice(start, line - 1), cur.slice(0, column - 1)].join('\n')
  const suffix = [cur.slice(column - 1), ...lines.slice(line, end)].join('\n')
  return { prefix, suffix }
}

/** Stable short hash for cache keys. */
export function hash(s: string): string {
  let h = 2166136261
  for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 16777619) }
  return (h >>> 0).toString(36)
}

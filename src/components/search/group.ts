import type { SearchMatch } from '@/types'

export interface GroupedFile { path: string; matches: SearchMatch[] }
/** Group a flat stream of matches by file preserving first-seen order. */
export function groupMatches(matches: SearchMatch[]): GroupedFile[] {
  const map = new Map<string, GroupedFile>()
  for (const m of matches) { let g = map.get(m.path); if (!g) { g = { path: m.path, matches: [] }; map.set(m.path, g) } g.matches.push(m) }
  return [...map.values()]
}
/** Split a line into [before, match, after] trimmed to a window around the match for compact rendering. */
export function previewParts(m: SearchMatch, windowBefore = 24): { before: string; match: string; after: string; truncatedStart: boolean } {
  const start = Math.max(0, m.matchStart - windowBefore)
  const before = m.lineText.slice(start, m.matchStart)
  const match = m.lineText.slice(m.matchStart, m.matchEnd)
  const after = m.lineText.slice(m.matchEnd, m.matchEnd + 120)
  return { before: before.replace(/^\s+/, start === 0 ? '' : ''), match, after, truncatedStart: start > 0 }
}

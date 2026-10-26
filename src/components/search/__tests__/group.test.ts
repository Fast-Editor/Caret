import { describe, it, expect } from 'vitest'
import { groupMatches, previewParts } from '@/components/search/group'
import type { SearchMatch } from '@/types'

const m = (path: string, line: number, text: string, start: number, end: number): SearchMatch => ({ path, line, column: start + 1, lineText: text, matchStart: start, matchEnd: end })
describe('search grouping', () => {
  it('groups by file preserving order', () => {
    const g = groupMatches([m('/a.ts', 1, 'foo', 0, 3), m('/b.ts', 2, 'foo', 0, 3), m('/a.ts', 5, 'xfoo', 1, 4)])
    expect(g.map((x) => x.path)).toEqual(['/a.ts', '/b.ts'])
    expect(g[0].matches).toHaveLength(2)
  })
  it('builds preview parts around the match', () => {
    const long = 'x'.repeat(60) + 'needle' + 'y'.repeat(200)
    const p = previewParts(m('/a', 1, long, 60, 66))
    expect(p.match).toBe('needle'); expect(p.truncatedStart).toBe(true); expect(p.before.length).toBe(24); expect(p.after.length).toBe(120)
  })
})

import { describe, it, expect } from 'vitest'
import { compactChain, flattenTree } from '@/components/explorer/compact'
import type { DirEntry } from '@/types'

const d = (path: string): DirEntry => ({ name: path.split('/').pop()!, path, kind: 'dir', isHidden: false, isIgnored: false })
const f = (path: string): DirEntry => ({ name: path.split('/').pop()!, path, kind: 'file', isHidden: path.split('/').pop()!.startsWith('.'), isIgnored: false })

describe('compact folders', () => {
  const children = {
    '/r': [d('/r/src'), f('/r/README.md')],
    '/r/src': [d('/r/src/main')],
    '/r/src/main': [d('/r/src/main/java')],
    '/r/src/main/java': [f('/r/src/main/java/App.java')],
  }
  it('collapses single-child directory chains', () => {
    const c = compactChain(d('/r/src'), children, true)
    expect(c.label).toBe('src/main/java')
    expect(c.path).toBe('/r/src/main/java')
    expect(c.chain).toEqual(['/r/src', '/r/src/main', '/r/src/main/java'])
  })
  it('does not collapse when disabled or when children unknown', () => {
    expect(compactChain(d('/r/src'), children, false).label).toBe('src')
    expect(compactChain(d('/r/src'), { '/r/src': undefined }, true).label).toBe('src')
  })
  it('flattens respecting expansion and hidden filter', () => {
    const rows = flattenTree({ root: '/r', children: { ...children, '/r': [d('/r/src'), f('/r/.env'), f('/r/README.md')] }, expanded: { '/r/src/main/java': true }, loading: {}, showHidden: false, filter: '', compact: true })
    expect(rows.map((r) => r.label)).toEqual(['src/main/java', 'App.java', 'README.md'])
    expect(rows[1].depth).toBe(1)
  })
  it('filter keeps matching files and their ancestors', () => {
    const rows = flattenTree({ root: '/r', children, expanded: {}, loading: {}, showHidden: true, filter: 'app', compact: true })
    expect(rows.map((r) => r.entry.name)).toEqual(['src', 'main', 'java', 'App.java'])
  })
})

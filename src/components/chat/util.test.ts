import { describe, expect, it } from 'vitest'
import { contextLabel, toolGroup, toolTitle, fmtTokens, fmtDuration, sortContext } from './util'
import type { ContextItem } from '@/types'

describe('contextLabel', () => {
  it('labels files with ranges relative to the workspace', () => {
    expect(contextLabel({ id: '1', kind: 'file', label: 'a', path: '/w/src/a.ts' })).toBe('a.ts')
    expect(contextLabel({ id: '1', kind: 'selection', label: 'a', path: '/w/src/a.ts', range: { startLine: 3, endLine: 9 } })).toBe('a.ts:3-9')
    expect(contextLabel({ id: '1', kind: 'folder', label: 'src', path: '/w/src' }, '/w')).toBe('src/')
    expect(contextLabel({ id: '1', kind: 'git-branch-diff', label: '', base: 'develop' })).toBe('Diff vs develop')
    expect(contextLabel({ id: '1', kind: 'link', label: '', url: 'https://example.com/docs' })).toBe('example.com/docs')
  })
})

describe('tool helpers', () => {
  it('titles common tools', () => {
    expect(toolTitle({ id: 't', name: 'read_file', displayName: 'Read', input: { path: '/w/src/a.ts' }, status: 'done' })).toBe('Read a.ts')
    expect(toolTitle({ id: 't', name: 'run_terminal_cmd', displayName: 'Run', input: { command: 'npm test' }, status: 'done' })).toBe('Ran `npm test`')
    expect(toolTitle({ id: 't', name: 'codebase_search', displayName: 'Search', input: { query: 'auth flow' }, status: 'done' })).toBe('Searched codebase "auth flow"')
  })
  it('groups tools', () => {
    expect(toolGroup({ id: 't', name: 'edit_file', displayName: '', input: {}, status: 'done' })).toBe('edit')
    expect(toolGroup({ id: 't', name: 'x', displayName: '', input: {}, status: 'done', mcpServerId: 'srv' })).toBe('mcp')
    expect(toolGroup({ id: 't', name: 'browser_click', displayName: '', input: {}, status: 'done' })).toBe('browser')
  })
})

describe('formatters', () => {
  it('formats tokens and durations', () => {
    expect(fmtTokens(950)).toBe('950'); expect(fmtTokens(12_500)).toBe('13k'); expect(fmtTokens(2_400_000)).toBe('2.4M')
    expect(fmtDuration(850)).toBe('850ms'); expect(fmtDuration(4200)).toBe('4.2s'); expect(fmtDuration(125_000)).toBe('2m 5s')
  })
  it('sorts context pills by kind priority', () => {
    const items: ContextItem[] = [{ id: '1', kind: 'image', label: 'i' }, { id: '2', kind: 'selection', label: 's' }, { id: '3', kind: 'file', label: 'f' }]
    expect(sortContext(items).map((i) => i.kind)).toEqual(['selection', 'file', 'image'])
  })
})

import { describe, it, expect } from 'vitest'
import { currentWindowLabel, displayPath, legacySessionKey, opensInNewWindow, readSession, sessionKey, windowPickItems, windowTitle } from './windowing'
import type { WindowSummary } from '@/types'

function storage(entries: Record<string, string>): Pick<Storage, 'getItem'> {
  return { getItem: (k: string) => (k in entries ? entries[k] : null) }
}

describe('editor session keys', () => {
  it('keys sessions by folder and window label', () => {
    expect(sessionKey('/repo/a', 'main')).toBe('caret.editor.session./repo/a::main')
    expect(sessionKey('/repo/a', 'w2')).toBe('caret.editor.session./repo/a::w2')
    expect(sessionKey('/repo/a', 'w2')).not.toBe(sessionKey('/repo/a', 'main'))
    expect(legacySessionKey('/repo/a')).toBe('caret.editor.session./repo/a')
  })
  it('reads the per-window key first', () => {
    const s = storage({ [sessionKey('/r', 'w2')]: '{"groups":[{"tabs":[{"path":"/r/new.ts"}]}],"activeGroup":0}', [legacySessionKey('/r')]: '{"groups":[{"tabs":[{"path":"/r/old.ts"}]}],"activeGroup":0}' })
    expect(readSession<{ groups: { tabs: { path: string }[] }[] }>(s, '/r', 'w2')?.groups[0].tabs[0].path).toBe('/r/new.ts')
  })
  it('falls back to the pre-multi-window key when the per-window key is empty', () => {
    const legacy = '{"groups":[{"tabs":[{"path":"/r/old.ts"}]}],"activeGroup":0}'
    expect(readSession<{ groups: unknown[] }>(storage({ [legacySessionKey('/r')]: legacy }), '/r', 'main')).toEqual(JSON.parse(legacy))
    expect(readSession<{ groups: unknown[] }>(storage({ [legacySessionKey('/r')]: legacy }), '/r', 'w3')).toEqual(JSON.parse(legacy))
    expect(readSession(storage({ [sessionKey('/r', 'main')]: '', [legacySessionKey('/r')]: legacy }), '/r', 'main')).toEqual(JSON.parse(legacy))
  })
  it('tolerates corrupt or null entries and returns null when nothing is saved', () => {
    const legacy = '{"groups":[],"activeGroup":0}'
    expect(readSession(storage({ [sessionKey('/r', 'main')]: '{not json', [legacySessionKey('/r')]: legacy }), '/r', 'main')).toEqual(JSON.parse(legacy))
    expect(readSession(storage({ [sessionKey('/r', 'main')]: 'null', [legacySessionKey('/r')]: legacy }), '/r', 'main')).toEqual(JSON.parse(legacy))
    expect(readSession(storage({}), '/r', 'main')).toBeNull()
    expect(readSession(storage({ [legacySessionKey('/other')]: legacy }), '/r', 'main')).toBeNull()
  })
  it('reports the main window outside Tauri', () => {
    expect(currentWindowLabel()).toBe('main')
  })
})

describe('opensInNewWindow', () => {
  it('always opens in place in an empty window', () => {
    expect(opensInNewWindow(false, 'new')).toBe(false)
    expect(opensInNewWindow(false, 'current')).toBe(false)
    expect(opensInNewWindow(false, undefined, true)).toBe(false)
  })
  it('follows window.openFoldersIn when a folder is open (default: new)', () => {
    expect(opensInNewWindow(true, undefined)).toBe(true)
    expect(opensInNewWindow(true, 'new')).toBe(true)
    expect(opensInNewWindow(true, 'current')).toBe(false)
  })
  it('inverts the setting for the Alt gesture', () => {
    expect(opensInNewWindow(true, 'new', true)).toBe(false)
    expect(opensInNewWindow(true, 'current', true)).toBe(true)
  })
})

describe('windowTitle', () => {
  it('matches the native title: "<folder> — Caret", or "Caret" when empty', () => {
    expect(windowTitle('caret')).toBe('caret — Caret')
    expect(windowTitle(null)).toBe('Caret')
    expect(windowTitle(undefined)).toBe('Caret')
    expect(windowTitle('')).toBe('Caret')
  })
  it('shortens the home directory for display', () => {
    expect(displayPath('/Users/me/repo/a')).toBe('~/repo/a')
    expect(displayPath('/srv/b')).toBe('/srv/b')
  })
})

describe('windowPickItems', () => {
  const list: WindowSummary[] = [
    { label: 'main', root: '/Users/me/repo/a', name: 'a', focused: false },
    { label: 'w2', root: null, name: null, focused: true },
    { label: 'w3', root: '/srv/b', name: 'b', focused: false },
  ]
  it('shows folder name, path and label, and marks the current window', () => {
    const items = windowPickItems(list, 'w2')
    expect(items[0]).toMatchObject({ id: 'main', label: 'a', description: '~/repo/a', detail: 'main' })
    expect(items[1]).toMatchObject({ id: 'w3', label: 'b', description: '/srv/b', detail: 'w3' })
    expect(items[2]).toMatchObject({ id: 'w2', label: 'Empty window', description: 'No folder open', detail: 'w2 · current' })
    expect(items[0].keywords).toBe('main /Users/me/repo/a')
  })
  it('puts the current window last and keeps creation order for the others', () => {
    expect(windowPickItems(list, 'w2').map((i) => i.id)).toEqual(['main', 'w3', 'w2'])
    expect(windowPickItems(list, 'main').map((i) => i.id)).toEqual(['w2', 'w3', 'main'])
  })
  it('falls back to the focused flag when the current label is not in the list', () => {
    const items = windowPickItems(list, 'zz')
    expect(items.map((i) => i.id)).toEqual(['main', 'w2', 'w3'])
    expect(items[1].detail).toBe('w2 · current')
  })
  it('handles a single window', () => {
    expect(windowPickItems([list[0]], 'main')).toHaveLength(1)
    expect(windowPickItems([], 'main')).toEqual([])
  })
})

import { describe, it, expect } from 'vitest'
import { currentWindowLabel, legacySessionKey, opensInNewWindow, readSession, sessionKey } from './windowing'

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

import { describe, it, expect } from 'vitest'
import { DEFAULTS, SETTINGS_FIELDS, SETTINGS_PAGES, patchFor, getPath, valuesEqual, isModified, searchFields, formatKeyChord, prettyKeybinding, fieldsForPage } from './settingsSchema'
import type { Settings } from '@/types'

const clone = (): Settings => JSON.parse(JSON.stringify(DEFAULTS))

describe('settings schema registry', () => {
  it('every field points at a known page and a real default path', () => {
    const pages = new Set(SETTINGS_PAGES.map((p) => p.id))
    for (const f of SETTINGS_FIELDS) {
      expect(pages.has(f.page), f.id).toBe(true)
      const [section] = f.id.split('.')
      expect(section in DEFAULTS, `${f.id} section`).toBe(true)
      if (!f.optional) expect(getPath(DEFAULTS, f.id), `${f.id} default`).not.toBeUndefined()
      if (f.type === 'select') expect(f.options?.length, `${f.id} options`).toBeGreaterThan(0)
    }
  })
  it('field ids are unique', () => {
    const ids = SETTINGS_FIELDS.map((f) => f.id)
    expect(new Set(ids).size).toBe(ids.length)
  })
  it('select defaults are one of the options', () => {
    for (const f of SETTINGS_FIELDS.filter((x) => x.type === 'select')) {
      const d = getPath(DEFAULTS, f.id) as string
      expect(f.options!.map((o) => o.value), f.id).toContain(d)
    }
  })
})

describe('patchFor / getPath', () => {
  it('builds nested patches and encodes undefined as null', () => {
    expect(patchFor('ui.fontSize', 14)).toEqual({ ui: { fontSize: 14 } })
    expect(patchFor('tab.modelId', undefined)).toEqual({ tab: { modelId: null } })
    expect(patchFor('userRules', 'x')).toEqual({ userRules: 'x' })
  })
  it('reads dotted paths safely', () => {
    expect(getPath(DEFAULTS, 'agent.autoRun')).toBe('allowlist')
    expect(getPath(DEFAULTS, 'nope.deeper')).toBeUndefined()
    expect(getPath(null, 'ui.theme')).toBeUndefined()
  })
})

describe('default-diff detection', () => {
  it('valuesEqual compares scalars, arrays and objects structurally', () => {
    expect(valuesEqual(1, 1)).toBe(true)
    expect(valuesEqual(['a', 'b'], ['a', 'b'])).toBe(true)
    expect(valuesEqual(['a', 'b'], ['b', 'a'])).toBe(false)
    expect(valuesEqual({ a: 1, b: [2] }, { b: [2], a: 1 })).toBe(true)
    expect(valuesEqual(undefined, null)).toBe(true)
    expect(valuesEqual(0, null)).toBe(false)
  })
  it('isModified flags only changed fields', () => {
    const s = clone()
    expect(isModified(s, 'ui.fontSize')).toBe(false)
    s.ui.fontSize = 15
    expect(isModified(s, 'ui.fontSize')).toBe(true)
    expect(isModified(s, 'ui.theme')).toBe(false)
    s.agent.commandAllowlist = [...s.agent.commandAllowlist, 'make']
    expect(isModified(s, 'agent.commandAllowlist')).toBe(true)
    expect(isModified(null, 'ui.fontSize')).toBe(false)
  })
})

describe('searchFields', () => {
  it('returns all fields for an empty query', () => { expect(searchFields('')).toHaveLength(SETTINGS_FIELDS.length) })
  it('ranks title matches above description matches', () => {
    const r = searchFields('font size')
    expect(r[0].id).toBe('ui.fontSize')
    expect(r.map((f) => f.id)).toContain('ui.terminalFontSize')
  })
  it('matches keywords and page names', () => {
    expect(searchFields('yolo')[0].id).toBe('agent.autoRun')
    expect(searchFields('autocomplete').some((f) => f.id === 'tab.enabled')).toBe(true)
    expect(searchFields('privacy').some((f) => f.id === 'privacy.privacyMode')).toBe(true)
  })
  it('requires every term to match', () => {
    expect(searchFields('font zzzznotaword')).toHaveLength(0)
  })
  it('fieldsForPage groups correctly', () => {
    expect(fieldsForPage('tab').every((f) => f.page === 'tab')).toBe(true)
    expect(fieldsForPage('tab').length).toBeGreaterThan(5)
  })
})

describe('keybinding capture formatting', () => {
  const ev = (key: string, mods: Partial<{ metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean; code: string }> = {}) => ({ key, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...mods })
  it('formats mac chords with mod for cmd', () => {
    expect(formatKeyChord(ev('k', { metaKey: true }))).toBe('mod+k')
    expect(formatKeyChord(ev('P', { metaKey: true, shiftKey: true }))).toBe('mod+shift+p')
    expect(formatKeyChord(ev('`', { ctrlKey: true }))).toBe('ctrl+`')
  })
  it('ignores bare modifier presses', () => {
    expect(formatKeyChord(ev('Meta', { metaKey: true }))).toBeNull()
    expect(formatKeyChord(ev('Shift', { shiftKey: true }))).toBeNull()
  })
  it('maps special keys and uses physical key for alt-symbols', () => {
    expect(formatKeyChord(ev('ArrowRight', { metaKey: true }))).toBe('mod+right')
    expect(formatKeyChord(ev(' ', { ctrlKey: true }))).toBe('ctrl+space')
    expect(formatKeyChord(ev('å', { altKey: true, code: 'KeyA' }))).toBe('alt+a')
  })
  it('uses ctrl as mod on non-mac', () => {
    expect(formatKeyChord(ev('s', { ctrlKey: true }), 'other')).toBe('mod+s')
    expect(formatKeyChord(ev('s', { metaKey: true }), 'other')).toBe('meta+s')
  })
  it('pretty prints with glyphs and chord separators', () => {
    expect(prettyKeybinding('mod+shift+p')).toEqual(['⌘', '⇧', 'P'])
    expect(prettyKeybinding('mod+k mod+s')).toEqual(['⌘', 'K', 'then', '⌘', 'S'])
    expect(prettyKeybinding(undefined)).toEqual([])
    expect(prettyKeybinding('ctrl+enter', 'other')).toEqual(['Ctrl', 'Enter'])
  })
})

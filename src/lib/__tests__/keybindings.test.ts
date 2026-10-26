import { describe, it, expect } from 'vitest'
import { parseChord, parseKeybinding, normalizeKeybinding, formatKeybinding, chordFromEvent, chordToString, isMac } from '@/lib/keybindings'

const mac = isMac()
describe('keybindings parsing', () => {
  it('parses modifiers and keys', () => {
    const c = parseChord('mod+shift+p')
    expect(c.shift).toBe(true)
    expect(mac ? c.meta : c.ctrl).toBe(true)
    expect(c.key).toBe('p')
  })
  it('handles aliases and special keys', () => {
    expect(parseChord('ctrl+`').key).toBe('`')
    expect(parseChord('cmd+esc').key).toBe('escape')
    expect(parseChord('alt+ArrowUp').key).toBe('up')
    expect(parseChord('mod+=').key).toBe('=')
    expect(parseChord('mod+\\').key).toBe('\\')
  })
  it('parses chords', () => {
    const ch = parseKeybinding('mod+k mod+s')
    expect(ch).toHaveLength(2)
    expect(ch[1].key).toBe('s')
    expect(normalizeKeybinding('Mod+K  mod+S')).toBe(normalizeKeybinding('mod+k mod+s'))
  })
  it('normalizes modifier order', () => {
    expect(normalizeKeybinding('shift+ctrl+a')).toBe('ctrl+shift+a')
  })
  it('formats for display', () => {
    const f = formatKeybinding('mod+shift+p')
    expect(mac ? f : f.toLowerCase()).toBe(mac ? '⇧⌘P' : 'ctrl+shift+p')
    expect(formatKeybinding('mod+k mod+s').split(' ')).toHaveLength(2)
    expect(formatKeybinding('enter')).toBe('↩')
  })
  it('builds chords from keyboard events using code', () => {
    const ev = { code: 'KeyP', key: 'π', metaKey: true, ctrlKey: false, altKey: true, shiftKey: false } as unknown as KeyboardEvent
    const c = chordFromEvent(ev)!
    expect(c.key).toBe('p'); expect(c.alt).toBe(true); expect(c.meta).toBe(true)
    expect(chordToString(c)).toBe('alt+meta+p')
    expect(chordFromEvent({ code: 'ShiftLeft', key: 'Shift', metaKey: false, ctrlKey: false, altKey: false, shiftKey: true } as unknown as KeyboardEvent)).toBeNull()
    expect(chordFromEvent({ code: 'Backquote', key: '`', metaKey: false, ctrlKey: true, altKey: false, shiftKey: false } as unknown as KeyboardEvent)!.key).toBe('`')
  })
})

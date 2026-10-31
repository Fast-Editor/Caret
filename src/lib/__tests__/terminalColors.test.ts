import { describe, it, expect } from 'vitest'
import { normalizeHex, blendHex, parseColorScheme, mergedAnsi, DARK_ANSI, toWindowsTerminalScheme } from '../terminalColors'

describe('terminalColors', () => {
  it('normalises hex forms', () => {
    expect(normalizeHex('#ABC')).toBe('#aabbcc')
    expect(normalizeHex('112233')).toBe('#112233')
    expect(normalizeHex('#11223344')).toBe('#112233')
    expect(normalizeHex('red')).toBeNull()
  })
  it('blends colours and leaves non-hex alone', () => {
    expect(blendHex('#000000', '#ffffff', 0.5)).toBe('#808080')
    expect(blendHex('rgba(0,0,0,0)', '#ff0000', 0.5)).toBe('rgba(0,0,0,0)')
  })
  it('imports a Windows Terminal scheme (purple → magenta)', () => {
    const s = parseColorScheme(JSON.stringify({ name: 'Campbell', black: '#0C0C0C', purple: '#881798', brightPurple: '#B4009E', background: '#0C0C0C', cursorColor: '#FFFFFF' }))!
    expect(s.name).toBe('Campbell'); expect(s.ansi.magenta).toBe('#881798'); expect(s.ansi.brightMagenta).toBe('#b4009e'); expect(s.background).toBe('#0c0c0c'); expect(s.cursor).toBe('#ffffff')
  })
  it('imports the first scheme of a settings.json and VS Code customisations', () => {
    expect(parseColorScheme(JSON.stringify({ schemes: [{ name: 'A', red: '#ff0000' }] }))!.ansi.red).toBe('#ff0000')
    expect(parseColorScheme(JSON.stringify({ 'workbench.colorCustomizations': { 'terminal.ansiBrightBlue': '#123456', 'terminal.background': '#000000' } }))).toMatchObject({ ansi: { brightBlue: '#123456' }, background: '#000000' })
  })
  it('imports an iTerm2 plist', () => {
    const plist = `<?xml version="1.0"?><plist><dict><key>Ansi 1 Color</key><dict><key>Blue Component</key><real>0.2</real><key>Green Component</key><real>0.3</real><key>Red Component</key><real>1</real></dict><key>Background Color</key><dict><key>Blue Component</key><real>0</real><key>Green Component</key><real>0</real><key>Red Component</key><real>0</real></dict></dict></plist>`
    const s = parseColorScheme(plist)!
    expect(s.ansi.red).toBe('#ff4d33'); expect(s.background).toBe('#000000')
  })
  it('rejects garbage and merges overrides onto a base palette', () => {
    expect(parseColorScheme('not json')).toBeNull()
    expect(parseColorScheme('{"foo":1}')).toBeNull()
    expect(mergedAnsi(DARK_ANSI, { red: '#abc', green: 'nope' })).toMatchObject({ red: '#aabbcc', green: DARK_ANSI.green })
    expect(JSON.parse(toWindowsTerminalScheme('x', DARK_ANSI)).purple).toBe(DARK_ANSI.magenta)
  })
})

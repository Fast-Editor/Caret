/**
 * Terminal colour helpers: the 16-colour ANSI palette per app theme, user overrides, tinting, and importers for the
 * common colour-scheme files (Windows Terminal JSON, iTerm2 .itermcolors, VS Code colorCustomizations).
 */
export const ANSI_KEYS = ['black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white', 'brightBlack', 'brightRed', 'brightGreen', 'brightYellow', 'brightBlue', 'brightMagenta', 'brightCyan', 'brightWhite'] as const
export type AnsiKey = (typeof ANSI_KEYS)[number]
export type AnsiPalette = Record<AnsiKey, string>
export type AnsiOverrides = Partial<Record<AnsiKey, string>>

export const ANSI_LABELS: Record<AnsiKey, string> = {
  black: 'Black', red: 'Red', green: 'Green', yellow: 'Yellow', blue: 'Blue', magenta: 'Magenta', cyan: 'Cyan', white: 'White',
  brightBlack: 'Bright black', brightRed: 'Bright red', brightGreen: 'Bright green', brightYellow: 'Bright yellow', brightBlue: 'Bright blue', brightMagenta: 'Bright magenta', brightCyan: 'Bright cyan', brightWhite: 'Bright white',
}

export const DARK_ANSI: AnsiPalette = {
  black: '#1e1e1e', red: '#f14c4c', green: '#23d18b', yellow: '#e5c07b', blue: '#3b8eea', magenta: '#d670d6', cyan: '#29b8db', white: '#cccccc',
  brightBlack: '#666666', brightRed: '#ff6b6b', brightGreen: '#3ddc97', brightYellow: '#f5d487', brightBlue: '#5aa7ff', brightMagenta: '#e58ae5', brightCyan: '#4fd2ee', brightWhite: '#ffffff',
}
export const LIGHT_ANSI: AnsiPalette = {
  black: '#1f2328', red: '#d1242f', green: '#1a7f37', yellow: '#9a6700', blue: '#0969da', magenta: '#8250df', cyan: '#1b7c83', white: '#6e7781',
  brightBlack: '#57606a', brightRed: '#a40e26', brightGreen: '#116329', brightYellow: '#7d4e00', brightBlue: '#0550ae', brightMagenta: '#6639ba', brightCyan: '#0e6670', brightWhite: '#8c959f',
}

/** `#rgb`, `#rrggbb` or `#rrggbbaa` → lower-case `#rrggbb`; anything else → null. */
export function normalizeHex(s: string | undefined | null): string | null {
  if (!s) return null
  const t = s.trim().toLowerCase()
  const m = /^#?([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/.exec(t)
  if (!m) return null
  const h = m[1]
  if (h.length === 3) return '#' + h.split('').map((c) => c + c).join('')
  return '#' + h.slice(0, 6)
}

function rgb(hex: string): [number, number, number] { const h = normalizeHex(hex) ?? '#000000'; return [parseInt(h.slice(1, 3), 16), parseInt(h.slice(3, 5), 16), parseInt(h.slice(5, 7), 16)] }
function hex(r: number, g: number, b: number): string { return '#' + [r, g, b].map((v) => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, '0')).join('') }

/** Mix `tint` into `base` by `amount` (0 = base, 1 = tint). Non-hex inputs return `base` unchanged. */
export function blendHex(base: string, tint: string, amount: number): string {
  const b = normalizeHex(base); const t = normalizeHex(tint)
  if (!b || !t) return base
  const [br, bg, bb] = rgb(b); const [tr, tg, tb] = rgb(t); const a = Math.max(0, Math.min(1, amount))
  return hex(br + (tr - br) * a, bg + (tg - bg) * a, bb + (tb - bb) * a)
}

export function mergedAnsi(base: AnsiPalette, overrides?: AnsiOverrides | Record<string, string>): AnsiPalette {
  const out = { ...base }
  if (overrides) for (const k of ANSI_KEYS) { const v = normalizeHex((overrides as Record<string, string | undefined>)[k]); if (v) out[k] = v }
  return out
}

export interface ImportedScheme { name?: string; ansi: AnsiOverrides; background?: string; foreground?: string; cursor?: string }

const WT_KEYS: Record<string, AnsiKey> = {
  black: 'black', red: 'red', green: 'green', yellow: 'yellow', blue: 'blue', purple: 'magenta', magenta: 'magenta', cyan: 'cyan', white: 'white',
  brightblack: 'brightBlack', brightred: 'brightRed', brightgreen: 'brightGreen', brightyellow: 'brightYellow', brightblue: 'brightBlue', brightpurple: 'brightMagenta', brightmagenta: 'brightMagenta', brightcyan: 'brightCyan', brightwhite: 'brightWhite',
}

function fromFlatObject(o: Record<string, unknown>): ImportedScheme | null {
  const ansi: AnsiOverrides = {}
  for (const [k, v] of Object.entries(o)) {
    if (typeof v !== 'string') continue
    // Windows Terminal ("brightPurple"), VS Code ("terminal.ansiBrightMagenta"), Alacritty-ish ("bright_black")
    const key = k.replace(/^terminal\.ansi/i, '').replace(/[_-]/g, '').toLowerCase()
    const mapped = WT_KEYS[key]
    const col = normalizeHex(v)
    if (mapped && col) ansi[mapped] = col
  }
  if (Object.keys(ansi).length === 0) return null
  return {
    name: typeof o.name === 'string' ? o.name : undefined, ansi,
    background: normalizeHex((o.background ?? o['terminal.background']) as string) ?? undefined,
    foreground: normalizeHex((o.foreground ?? o['terminal.foreground']) as string) ?? undefined,
    cursor: normalizeHex((o.cursorColor ?? o.cursor) as string) ?? undefined,
  }
}

function fromItermPlist(xml: string): ImportedScheme | null {
  const ansi: AnsiOverrides = {}
  let background: string | undefined, foreground: string | undefined, cursor: string | undefined
  const re = /<key>(Ansi (\d+) Color|Background Color|Foreground Color|Cursor Color)<\/key>\s*<dict>([\s\S]*?)<\/dict>/g
  let m: RegExpExecArray | null
  while ((m = re.exec(xml))) {
    const body = m[3]
    const comp = (name: string) => { const r = new RegExp(`<key>${name} Component</key>\\s*<real>([\\d.eE+-]+)</real>`).exec(body); return r ? Math.round(parseFloat(r[1]) * 255) : null }
    const r = comp('Red'); const g = comp('Green'); const b = comp('Blue')
    if (r == null || g == null || b == null) continue
    const col = hex(r, g, b)
    if (m[2] !== undefined) { const i = parseInt(m[2], 10); if (i >= 0 && i < 16) ansi[ANSI_KEYS[i]] = col }
    else if (m[1] === 'Background Color') background = col
    else if (m[1] === 'Foreground Color') foreground = col
    else if (m[1] === 'Cursor Color') cursor = col
  }
  if (Object.keys(ansi).length === 0) return null
  return { ansi, background, foreground, cursor }
}

/** Parse a colour scheme from pasted text or a file: Windows Terminal JSON (a scheme, or a settings.json with `schemes`), VS Code `workbench.colorCustomizations`, iTerm2 .itermcolors. */
export function parseColorScheme(text: string): ImportedScheme | null {
  const t = text.trim()
  if (!t) return null
  if (t.startsWith('<')) return fromItermPlist(t)
  let json: unknown
  try { json = JSON.parse(t) } catch { return null }
  if (!json || typeof json !== 'object') return null
  if (Array.isArray(json)) return json.length ? fromFlatObject(json[0] as Record<string, unknown>) : null
  const o = json as Record<string, unknown>
  if (Array.isArray(o.schemes) && o.schemes.length) return fromFlatObject(o.schemes[0] as Record<string, unknown>)
  if (o['workbench.colorCustomizations'] && typeof o['workbench.colorCustomizations'] === 'object') return fromFlatObject(o['workbench.colorCustomizations'] as Record<string, unknown>)
  return fromFlatObject(o)
}

/** Serialise a palette as a Windows Terminal scheme (the most widely accepted interchange format). */
export function toWindowsTerminalScheme(name: string, palette: AnsiPalette, extra: { background?: string; foreground?: string; cursor?: string } = {}): string {
  const o: Record<string, string> = { name }
  for (const k of ANSI_KEYS) o[k === 'magenta' ? 'purple' : k === 'brightMagenta' ? 'brightPurple' : k] = palette[k]
  if (extra.background) o.background = extra.background
  if (extra.foreground) o.foreground = extra.foreground
  if (extra.cursor) o.cursorColor = extra.cursor
  return JSON.stringify(o, null, 2)
}

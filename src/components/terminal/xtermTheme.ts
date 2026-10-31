import type { ITheme } from '@xterm/xterm'
import { cssVar, themeKind } from '@/lib/theme'
import { DARK_ANSI, LIGHT_ANSI, blendHex, mergedAnsi, type AnsiOverrides, type AnsiPalette } from '@/lib/terminalColors'

export type TerminalBackgroundMode = 'panel' | 'editor' | 'app' | 'transparent'
export interface TerminalThemeOptions {
  /** User overrides for the 16 ANSI colours of the current app theme. */
  ansi?: AnsiOverrides | Record<string, string>
  background?: TerminalBackgroundMode
  /** Per-terminal colour: mixed into the background so split panes are telling apart. */
  tint?: string
}

/** The 16-colour palette an app theme starts from (before user overrides). */
export function defaultAnsi(themeId: string | undefined): AnsiPalette { return themeKind(themeId) === 'dark' ? { ...DARK_ANSI } : { ...LIGHT_ANSI } }

function baseBackground(mode: TerminalBackgroundMode, dark: boolean): string {
  switch (mode) {
    case 'editor': return cssVar('--bg-editor') || (dark ? '#1a1a1a' : '#ffffff')
    case 'app': return cssVar('--bg-app') || (dark ? '#141414' : '#f6f6f8')
    case 'transparent': return 'rgba(0,0,0,0)'
    default: return cssVar('--bg-panel') || (dark ? '#181818' : '#f0f0f3')
  }
}

export function xtermTheme(themeId: string | undefined, opts: TerminalThemeOptions = {}): ITheme {
  const dark = themeKind(themeId) === 'dark'
  const fg = cssVar('--fg') || (dark ? '#cccccc' : '#1f2328')
  let bg = baseBackground(opts.background ?? 'panel', dark)
  if (opts.tint && bg !== 'rgba(0,0,0,0)') bg = blendHex(bg, opts.tint, 0.14)
  const palette = mergedAnsi(defaultAnsi(themeId), opts.ansi)
  return {
    foreground: fg, background: bg, cursor: cssVar('--accent') || '#ececec', cursorAccent: bg === 'rgba(0,0,0,0)' ? (dark ? '#181818' : '#f0f0f3') : bg,
    selectionBackground: dark ? 'rgba(255,255,255,0.22)' : 'rgba(47,111,237,0.25)', selectionInactiveBackground: dark ? 'rgba(255,255,255,0.12)' : 'rgba(47,111,237,0.14)',
    scrollbarSliderBackground: 'rgba(127,127,127,0.25)', scrollbarSliderHoverBackground: 'rgba(127,127,127,0.4)', scrollbarSliderActiveBackground: 'rgba(127,127,127,0.5)',
    ...palette,
  }
}

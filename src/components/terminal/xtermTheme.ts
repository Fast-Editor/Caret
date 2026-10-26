import type { ITheme } from '@xterm/xterm'
import { cssVar, themeKind } from '@/lib/theme'

const DARK_ANSI = { black: '#1e1e1e', red: '#f14c4c', green: '#23d18b', yellow: '#e5c07b', blue: '#3b8eea', magenta: '#d670d6', cyan: '#29b8db', white: '#cccccc', brightBlack: '#666666', brightRed: '#ff6b6b', brightGreen: '#3ddc97', brightYellow: '#f5e07b', brightBlue: '#5ea3f0', brightMagenta: '#e58ae5', brightCyan: '#4fd1e6', brightWhite: '#ffffff' }
const LIGHT_ANSI = { black: '#1f2328', red: '#d1242f', green: '#1a7f37', yellow: '#9a6700', blue: '#0969da', magenta: '#8250df', cyan: '#1b7c83', white: '#6e7781', brightBlack: '#57606a', brightRed: '#a40e26', brightGreen: '#116329', brightYellow: '#7d4e00', brightBlue: '#0550ae', brightMagenta: '#6639ba', brightCyan: '#0f6f7a', brightWhite: '#1f2328' }

export function xtermTheme(themeId: string | undefined): ITheme {
  const dark = themeKind(themeId) === 'dark'
  const fg = cssVar('--fg') || (dark ? '#cccccc' : '#1f2328')
  const bg = cssVar('--bg-panel') || (dark ? '#181818' : '#f0f0f3')
  return {
    foreground: fg, background: bg, cursor: cssVar('--accent') || '#ececec', cursorAccent: bg,
    selectionBackground: dark ? 'rgba(255,255,255,0.22)' : 'rgba(47,111,237,0.25)', selectionInactiveBackground: dark ? 'rgba(255,255,255,0.12)' : 'rgba(47,111,237,0.14)',
    scrollbarSliderBackground: 'rgba(127,127,127,0.25)', scrollbarSliderHoverBackground: 'rgba(127,127,127,0.4)', scrollbarSliderActiveBackground: 'rgba(127,127,127,0.5)',
    ...(dark ? DARK_ANSI : LIGHT_ANSI),
  }
}

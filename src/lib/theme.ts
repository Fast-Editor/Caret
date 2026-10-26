import { useSettings } from '@/store/settings'
import { useUi } from '@/store/ui'
import type { ThemeId } from '@/types'

export const THEMES: { id: ThemeId; label: string; kind: 'dark' | 'light' }[] = [
  { id: 'caret-dark', label: 'Caret Dark', kind: 'dark' },
  { id: 'caret-midnight', label: 'Caret Midnight', kind: 'dark' },
  { id: 'caret-light', label: 'Caret Light', kind: 'light' },
  { id: 'caret-high-contrast', label: 'High Contrast', kind: 'dark' },
]
export function themeKind(id: string | undefined): 'dark' | 'light' { return THEMES.find((t) => t.id === id)?.kind ?? 'dark' }

export function applyTheme(theme: string | undefined, reduceMotion = false): void {
  const html = document.documentElement
  const id = THEMES.some((t) => t.id === theme) ? (theme as string) : 'caret-dark'
  for (const c of Array.from(html.classList)) if (c.startsWith('theme-')) html.classList.remove(c)
  html.classList.add(`theme-${id}`)
  html.classList.toggle('dark', themeKind(id) === 'dark')
  html.classList.toggle('light', themeKind(id) === 'light')
  html.classList.toggle('reduce-motion', reduceMotion)
  html.style.colorScheme = themeKind(id)
  document.dispatchEvent(new CustomEvent('caret:theme', { detail: { theme: id } }))
}

export function applyZoom(level: number): void {
  const factor = Math.pow(1.1, level)
  const root = document.getElementById('root')
  if (root) (root.style as unknown as Record<string, string>).zoom = factor === 1 ? '' : String(factor)
}

let inited = false
export function initTheme(): void {
  if (inited) return
  inited = true
  const sync = () => {
    const s = useSettings.getState().settings
    applyTheme(s?.ui.theme, s?.ui.reduceMotion)
    const z = useUi.getState().zoom || s?.ui.zoomLevel || 0
    applyZoom(z)
    if (s?.ui.fontFamily) document.documentElement.style.setProperty('--font-mono', s.ui.fontFamily)
  }
  sync()
  useSettings.subscribe(sync)
  useUi.subscribe((st, prev) => { if (st.zoom !== prev.zoom) applyZoom(st.zoom) })
}

/** Read a CSS token value (e.g. '--bg-editor') from the document. */
export function cssVar(name: string): string { return getComputedStyle(document.documentElement).getPropertyValue(name).trim() }

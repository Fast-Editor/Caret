import { create } from 'zustand'

export type SidebarView = 'explorer' | 'search' | 'scm' | 'agents' | 'rules' | 'outline' | 'debug'
export type PanelView = 'terminal' | 'problems' | 'output' | 'review' | 'ports' | 'debugConsole'
export type LayoutMode = 'editor' | 'agents'
export type PaletteMode = null | 'commands' | 'files' | 'symbols' | 'recent' | 'search'

const LS_KEY = 'caret.ui.v1'
type Persisted = Pick<UiStore, 'sidebarVisible' | 'sidebarView' | 'sidebarWidth' | 'panelVisible' | 'panelView' | 'panelHeight' | 'chatVisible' | 'chatWidth' | 'layout' | 'zoom'>
function loadPersisted(): Partial<Persisted> {
  try { const raw = localStorage.getItem(LS_KEY); return raw ? (JSON.parse(raw) as Partial<Persisted>) : {} } catch { return {} }
}
let saveTimer: number | undefined
function persist(s: UiStore): void {
  if (saveTimer) window.clearTimeout(saveTimer)
  saveTimer = window.setTimeout(() => {
    const p: Persisted = { sidebarVisible: s.sidebarVisible, sidebarView: s.sidebarView, sidebarWidth: s.sidebarWidth, panelVisible: s.panelVisible, panelView: s.panelView, panelHeight: s.panelHeight, chatVisible: s.chatVisible, chatWidth: s.chatWidth, layout: s.layout, zoom: s.zoom }
    try { localStorage.setItem(LS_KEY, JSON.stringify(p)) } catch { /* ignore */ }
  }, 150)
}

export interface UiStore {
  sidebarVisible: boolean
  sidebarView: SidebarView
  sidebarWidth: number
  panelVisible: boolean
  panelView: PanelView
  panelHeight: number
  panelMaximized: boolean
  chatVisible: boolean
  chatWidth: number
  layout: LayoutMode
  zenMode: boolean
  paletteOpen: PaletteMode
  paletteInitial: string
  settingsOpen: boolean
  settingsPage: string | undefined
  settingsQuery: string | undefined
  zoom: number
  focusRegion: 'sidebar' | 'editor' | 'panel' | 'chat'
  /** Platform hint set at boot (plugin-os). */
  platform: 'macos' | 'windows' | 'linux' | 'other'
  windowFocused: boolean
  set: (patch: Partial<UiStore>) => void
  toggleSidebar: (view?: SidebarView) => void
  togglePanel: (view?: PanelView) => void
  toggleChat: () => void
  showPanel: (view: PanelView) => void
  showSidebar: (view: SidebarView) => void
  openPalette: (mode: Exclude<PaletteMode, null>, initial?: string) => void
  closePalette: () => void
  openSettings: (page?: string, query?: string) => void
  closeSettings: () => void
  setZoom: (z: number) => void
}

export const useUi = create<UiStore>((set, get) => ({
  sidebarVisible: true, sidebarView: 'explorer', sidebarWidth: 260,
  panelVisible: false, panelView: 'terminal', panelHeight: 260, panelMaximized: false,
  chatVisible: true, chatWidth: 420,
  layout: 'editor', zenMode: false, paletteOpen: null, paletteInitial: '', settingsOpen: false, settingsPage: undefined, settingsQuery: undefined, zoom: 0, focusRegion: 'editor',
  platform: 'other', windowFocused: true,
  ...loadPersisted(),
  set: (patch) => set(patch),
  toggleSidebar: (view) => {
    const s = get()
    if (view && (view !== s.sidebarView || !s.sidebarVisible)) set({ sidebarVisible: true, sidebarView: view })
    else set({ sidebarVisible: !s.sidebarVisible })
  },
  togglePanel: (view) => {
    const s = get()
    if (view && (view !== s.panelView || !s.panelVisible)) set({ panelVisible: true, panelView: view })
    else set({ panelVisible: !s.panelVisible, panelMaximized: false })
  },
  toggleChat: () => set({ chatVisible: !get().chatVisible }),
  showPanel: (view) => set({ panelVisible: true, panelView: view }),
  showSidebar: (view) => set({ sidebarVisible: true, sidebarView: view }),
  openPalette: (mode, initial = '') => set({ paletteOpen: mode, paletteInitial: initial }),
  closePalette: () => set({ paletteOpen: null, paletteInitial: '' }),
  openSettings: (page, query) => set({ settingsOpen: true, settingsPage: page, settingsQuery: query, layout: 'editor' }),
  closeSettings: () => set({ settingsOpen: false }),
  setZoom: (z) => set({ zoom: Math.max(-5, Math.min(8, Math.round(z))) }),
}))
useUi.subscribe((s) => persist(s))

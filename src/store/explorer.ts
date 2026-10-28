import { create } from 'zustand'
import type { DirEntry } from '@/types'
import { api } from '@/lib/api'

export type EditingState = { kind: 'rename'; path: string } | { kind: 'create-file' | 'create-dir'; parent: string } | null

interface ExplorerStore {
  children: Record<string, DirEntry[] | undefined>
  loading: Record<string, boolean | undefined>
  errors: Record<string, string | undefined>
  expanded: Record<string, true | undefined>
  selected: Record<string, true | undefined>
  focused: string | null
  anchor: string | null
  showHidden: boolean
  filter: string
  filterOpen: boolean
  followActive: boolean
  editing: EditingState
  clipboard: { paths: string[]; cut: boolean } | null
  openEditorsCollapsed: boolean
  treeCollapsed: boolean
  /** Bottom sections (VS Code style), collapsed by default. */
  outlineCollapsed: boolean
  timelineCollapsed: boolean
  loadDir: (path: string, force?: boolean) => Promise<DirEntry[]>
  toggle: (path: string) => void
  expand: (path: string) => void
  collapse: (path: string) => void
  collapseAll: () => void
  expandTo: (path: string, root: string) => Promise<void>
  refreshPaths: (paths: string[]) => void
  select: (path: string, opts?: { toggle?: boolean; range?: boolean; rows?: string[] }) => void
  setFocused: (path: string | null) => void
  set: (patch: Partial<ExplorerStore>) => void
  reset: () => void
}

function parentOf(p: string): string { const i = p.lastIndexOf('/'); return i > 0 ? p.slice(0, i) : '/' }

export const useExplorer = create<ExplorerStore>((set, get) => ({
  children: {}, loading: {}, errors: {}, expanded: {}, selected: {}, focused: null, anchor: null,
  showHidden: true, filter: '', filterOpen: false, followActive: true, editing: null, clipboard: null, openEditorsCollapsed: false, treeCollapsed: false, outlineCollapsed: true, timelineCollapsed: true,
  loadDir: async (path, force = false) => {
    const s = get()
    if (!force && s.children[path]) return s.children[path]!
    if (s.loading[path]) return s.children[path] ?? []
    set((st) => ({ loading: { ...st.loading, [path]: true } }))
    try {
      const entries = await api.fs.readDir(path, true)
      entries.sort((a, b) => (a.kind === 'dir' ? 0 : 1) - (b.kind === 'dir' ? 0 : 1) || a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' }))
      set((st) => ({ children: { ...st.children, [path]: entries }, loading: { ...st.loading, [path]: undefined }, errors: { ...st.errors, [path]: undefined } }))
      return entries
    } catch (e) {
      set((st) => ({ loading: { ...st.loading, [path]: undefined }, errors: { ...st.errors, [path]: String((e as Error).message ?? e) }, children: { ...st.children, [path]: [] } }))
      return []
    }
  },
  toggle: (path) => { const s = get(); if (s.expanded[path]) s.collapse(path); else s.expand(path) },
  expand: (path) => { set((st) => ({ expanded: { ...st.expanded, [path]: true } })); void get().loadDir(path) },
  collapse: (path) => set((st) => { const e = { ...st.expanded }; delete e[path]; return { expanded: e } }),
  collapseAll: () => set({ expanded: {} }),
  expandTo: async (path, root) => {
    if (!path.startsWith(root)) return
    const rel = path.slice(root.length).replace(/^\//, '')
    const segs = rel.split('/')
    segs.pop()
    let cur = root
    const exp: Record<string, true | undefined> = { ...get().expanded }
    for (const seg of segs) { cur = `${cur}/${seg}`; exp[cur] = true; await get().loadDir(cur) }
    set({ expanded: exp, focused: path, selected: { [path]: true }, anchor: path })
  },
  refreshPaths: (paths) => {
    const dirs = new Set<string>()
    const s = get()
    for (const p of paths) {
      const parent = parentOf(p)
      if (s.children[parent] !== undefined) dirs.add(parent)
      if (s.children[p] !== undefined) dirs.add(p)
    }
    for (const d of dirs) void s.loadDir(d, true)
  },
  select: (path, opts) => {
    const s = get()
    if (opts?.range && s.anchor && opts.rows) {
      const a = opts.rows.indexOf(s.anchor); const b = opts.rows.indexOf(path)
      if (a >= 0 && b >= 0) { const [lo, hi] = a < b ? [a, b] : [b, a]; const sel: Record<string, true> = {}; for (let i = lo; i <= hi; i++) sel[opts.rows[i]] = true; set({ selected: sel, focused: path }); return }
    }
    if (opts?.toggle) { const sel = { ...s.selected }; if (sel[path]) delete sel[path]; else sel[path] = true; set({ selected: sel, focused: path, anchor: path }); return }
    set({ selected: { [path]: true }, focused: path, anchor: path })
  },
  setFocused: (path) => set({ focused: path }),
  set: (patch) => set(patch),
  reset: () => set({ children: {}, loading: {}, errors: {}, expanded: {}, selected: {}, focused: null, anchor: null, filter: '', filterOpen: false, editing: null, clipboard: null }),
}))

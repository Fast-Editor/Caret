import { create } from 'zustand'
import type { SearchMatch, SearchQuery } from '@/types'
import { api } from '@/lib/api'

export interface FileResults { path: string; matches: SearchMatch[] }
interface SearchStore {
  query: string; replacement: string; isRegex: boolean; caseSensitive: boolean; wholeWord: boolean
  includes: string; excludes: string; useIgnoreFiles: boolean; contextLines: number; showReplace: boolean; showDetails: boolean
  results: Record<string, SearchMatch[]>; order: string[]; total: number; fileCount: number
  running: boolean; searchId: string | null; elapsedMs: number; limited: boolean; error: string | null
  collapsed: Record<string, true | undefined>
  history: string[]
  selectedKey: string | null
  set: (patch: Partial<SearchStore>) => void
  run: () => Promise<void>
  cancel: () => Promise<void>
  dismissFile: (path: string) => void
  dismissMatch: (path: string, idx: number) => void
  toggleCollapsed: (path: string) => void
  collapseAll: (collapsed: boolean) => void
  buildQuery: () => SearchQuery
  replaceAll: (paths?: string[]) => Promise<{ files: number; replacements: number }>
  clear: () => void
}

function splitGlobs(s: string): string[] { return s.split(',').map((x) => x.trim()).filter(Boolean) }
const pending: SearchMatch[] = []
let flushTimer: number | null = null

export const useSearch = create<SearchStore>((set, get) => ({
  query: '', replacement: '', isRegex: false, caseSensitive: false, wholeWord: false, includes: '', excludes: '', useIgnoreFiles: true, contextLines: 0, showReplace: false, showDetails: false,
  results: {}, order: [], total: 0, fileCount: 0, running: false, searchId: null, elapsedMs: 0, limited: false, error: null, collapsed: {}, history: JSON.parse(localStorage.getItem('caret.search.history') ?? '[]') as string[], selectedKey: null,
  set: (patch) => set(patch),
  buildQuery: () => {
    const s = get()
    return { query: s.query, isRegex: s.isRegex, caseSensitive: s.caseSensitive, wholeWord: s.wholeWord, includes: splitGlobs(s.includes), excludes: splitGlobs(s.excludes), useIgnoreFiles: s.useIgnoreFiles, contextLines: s.contextLines, maxResults: 20000 }
  },
  run: async () => {
    const s = get()
    if (s.running) await s.cancel()
    if (!s.query) { set({ results: {}, order: [], total: 0, fileCount: 0, error: null }); return }
    const history = [s.query, ...s.history.filter((h) => h !== s.query)].slice(0, 10)
    localStorage.setItem('caret.search.history', JSON.stringify(history))
    set({ running: true, results: {}, order: [], total: 0, fileCount: 0, error: null, limited: false, elapsedMs: 0, history, collapsed: {} })
    pending.length = 0
    const flush = () => {
      flushTimer = null
      if (!pending.length) return
      const batch = pending.splice(0, pending.length)
      set((st) => {
        const results = { ...st.results }
        const order = st.order.slice()
        for (const m of batch) {
          if (!results[m.path]) { results[m.path] = []; order.push(m.path) }
          results[m.path] = [...results[m.path], m]
        }
        return { results, order, total: st.total + batch.length, fileCount: order.length }
      })
    }
    try {
      const id = await api.search.text(s.buildQuery(), (ev) => {
        if (ev.type === 'match') { pending.push(ev.match); if (flushTimer == null) flushTimer = window.setTimeout(flush, 40) }
        else if (ev.type === 'done') { if (flushTimer != null) { clearTimeout(flushTimer); flushTimer = null } flush(); set({ running: false, searchId: null, limited: ev.limited, elapsedMs: ev.elapsedMs }) }
        else if (ev.type === 'error') { flush(); set({ running: false, searchId: null, error: ev.message }) }
      })
      set({ searchId: id })
    } catch (e) { set({ running: false, error: String((e as Error).message ?? e) }) }
  },
  cancel: async () => { const id = get().searchId; set({ running: false, searchId: null }); if (id) { try { await api.search.cancel(id) } catch { /* ignore */ } } },
  dismissFile: (path) => set((st) => { const results = { ...st.results }; const n = results[path]?.length ?? 0; delete results[path]; return { results, order: st.order.filter((p) => p !== path), total: st.total - n, fileCount: st.fileCount - 1 } }),
  dismissMatch: (path, idx) => set((st) => { const arr = (st.results[path] ?? []).filter((_, i) => i !== idx); const results = { ...st.results, [path]: arr }; if (!arr.length) { delete results[path]; return { results, order: st.order.filter((p) => p !== path), total: st.total - 1, fileCount: st.fileCount - 1 } } return { results, total: st.total - 1 } }),
  toggleCollapsed: (path) => set((st) => { const c = { ...st.collapsed }; if (c[path]) delete c[path]; else c[path] = true; return { collapsed: c } }),
  collapseAll: (collapsed) => set((st) => ({ collapsed: collapsed ? Object.fromEntries(st.order.map((p) => [p, true as const])) : {} })),
  replaceAll: async (paths) => {
    const s = get()
    const res = await api.search.replace({ query: s.buildQuery(), replacement: s.replacement, paths })
    await s.run()
    return res
  },
  clear: () => set({ query: '', replacement: '', results: {}, order: [], total: 0, fileCount: 0, error: null }),
}))

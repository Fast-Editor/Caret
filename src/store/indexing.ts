import { create } from 'zustand'
import type { IndexStatus, DocSource } from '@/types'
import { api } from '@/lib/api'
import { Events, on } from '@/lib/tauri'

interface IndexingStore {
  status: IndexStatus | null
  docs: DocSource[]
  loaded: boolean
  load: () => Promise<void>
  loadDocs: () => Promise<void>
}

export const useIndexing = create<IndexingStore>((set) => ({
  status: null, docs: [], loaded: false,
  load: async () => {
    try { set({ status: await api.index.status(), loaded: true }) } catch { set({ loaded: true }) }
  },
  loadDocs: async () => { try { set({ docs: await api.docs.list() }) } catch { /* backend not ready */ } },
}))

let subscribed = false
export function initIndexingStore(): void {
  if (subscribed) return
  subscribed = true
  const s = useIndexing.getState()
  s.load().catch(() => {}); s.loadDocs().catch(() => {})
  on<IndexStatus>(Events.indexStatus, (status) => useIndexing.setState({ status }))
  on(Events.docsChanged, () => { useIndexing.getState().loadDocs().catch(() => {}) })
  on(Events.workspaceChanged, () => { const st = useIndexing.getState(); st.load().catch(() => {}); st.loadDocs().catch(() => {}) })
}

import { create } from 'zustand'
import type { WorkspaceInfo, RecentWorkspace } from '@/types'
import { api } from '@/lib/api'
import { Events, on } from '@/lib/tauri'

interface WorkspaceStore {
  workspace: WorkspaceInfo | null
  recent: RecentWorkspace[]
  loaded: boolean
  load: () => Promise<void>
  open: (path: string) => Promise<WorkspaceInfo>
  close: () => Promise<void>
  refreshRecent: () => Promise<void>
}

export const useWorkspace = create<WorkspaceStore>((set) => ({
  workspace: null,
  recent: [],
  loaded: false,
  load: async () => {
    const [workspace, recent] = await Promise.all([api.workspace.current(), api.workspace.recent()])
    set({ workspace, recent, loaded: true })
  },
  open: async (path) => {
    const ws = await api.workspace.open(path)
    set({ workspace: ws })
    api.workspace.recent().then((recent) => set({ recent })).catch(() => {})
    return ws
  },
  close: async () => { await api.workspace.close(); set({ workspace: null }) },
  refreshRecent: async () => set({ recent: await api.workspace.recent() }),
}))

let subscribed = false
export function initWorkspaceStore(): void {
  if (subscribed) return
  subscribed = true
  useWorkspace.getState().load().catch((e) => console.error('workspace load failed', e))
  on<WorkspaceInfo | null>(Events.workspaceChanged, (ws) => useWorkspace.setState({ workspace: ws }))
}

/** Absolute → workspace-relative display path. */
export function relPath(path: string): string {
  const root = useWorkspace.getState().workspace?.root
  if (root && path.startsWith(root)) return path.slice(root.length).replace(/^\//, '') || '.'
  return path
}
export function baseName(path: string): string { return path.split('/').pop() ?? path }

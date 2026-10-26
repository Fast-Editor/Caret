import { create } from 'zustand'
import type { LspServerConfig, LspServerStatus } from '@/types'

export interface LspServerEntry { config: LspServerConfig; installed: boolean; status?: LspServerStatus; restarts: number }
interface LspStore {
  servers: Record<string, LspServerEntry>
  /** running server id by config id */
  running: Record<string, string>
  setConfigs: (list: { config: LspServerConfig; installed: boolean }[]) => void
  setStatus: (configId: string, status: LspServerStatus | undefined) => void
  bumpRestarts: (configId: string) => number
  reset: () => void
}
export const useLsp = create<LspStore>((set, get) => ({
  servers: {}, running: {},
  setConfigs: (list) => {
    const servers: Record<string, LspServerEntry> = {}
    for (const e of list) servers[e.config.id] = { ...(get().servers[e.config.id] ?? { restarts: 0 }), config: e.config, installed: e.installed }
    set({ servers })
  },
  setStatus: (configId, status) => {
    const servers = { ...get().servers }
    if (!servers[configId]) return
    servers[configId] = { ...servers[configId], status }
    const running = { ...get().running }
    if (status && (status.state === 'running' || status.state === 'starting')) running[configId] = status.serverId; else delete running[configId]
    set({ servers, running })
  },
  bumpRestarts: (configId) => { const servers = { ...get().servers }; const n = (servers[configId]?.restarts ?? 0) + 1; if (servers[configId]) servers[configId] = { ...servers[configId], restarts: n }; set({ servers }); return n },
  reset: () => set({ servers: {}, running: {} }),
}))
export function lspSummary(): { running: number; label: string } {
  const s = useLsp.getState()
  const running = Object.values(s.servers).filter((e) => e.status?.state === 'running').length
  const starting = Object.values(s.servers).filter((e) => e.status?.state === 'starting').length
  return { running, label: running ? `LSP: ${running} running${starting ? ` · ${starting} starting` : ''}` : starting ? 'LSP: starting…' : 'LSP: idle' }
}

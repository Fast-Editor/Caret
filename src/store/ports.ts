import { create } from 'zustand'
import type { ListeningPort } from '@/types'
import { api } from '@/lib/api'

/** Sort by port, then by pid, so the list is stable across refreshes. */
export function sortPorts(ports: ListeningPort[]): ListeningPort[] { return [...ports].sort((a, b) => a.port - b.port || a.pid - b.pid) }
/** URL to open in a browser; wildcard/IPv6 binds are reachable via localhost. */
export function portUrl(p: Pick<ListeningPort, 'port' | 'address'>): string {
  const host = p.address === '*' || p.address === '0.0.0.0' || p.address === '::' || p.address === '' || p.address.includes(':') ? 'localhost' : p.address
  return `http://${host}:${p.port}`
}
export function originLabel(p: Pick<ListeningPort, 'ownedByCaret'>): string { return p.ownedByCaret ? 'Caret terminal' : 'Other' }
/** Short, readable address for the table (`*` → "all interfaces"). */
export function addressLabel(address: string): string { return address === '*' || address === '0.0.0.0' || address === '::' ? 'all interfaces' : address }

interface PortsStore {
  ports: ListeningPort[]
  loading: boolean
  error: string | null
  lastUpdated: number | null
  refresh: () => Promise<void>
  kill: (pid: number) => Promise<void>
}

export const usePorts = create<PortsStore>((set, get) => ({
  ports: [], loading: false, error: null, lastUpdated: null,
  refresh: async () => {
    if (get().loading) return
    set({ loading: true })
    try { const ports = await api.ports.list(); set({ ports: sortPorts(ports), error: null, lastUpdated: Date.now(), loading: false }) }
    catch (e) { set({ error: String((e as Error)?.message ?? e), loading: false }) }
  },
  kill: async (pid) => { await api.ports.kill(pid); await get().refresh() },
}))

/** Number of local servers currently listening (for a tab badge). */
export const usePortsCount = (): number => usePorts((s) => s.ports.length)

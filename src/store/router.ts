import { create } from 'zustand'
import type { RouterStatus } from '@/types'
import { api } from '@/lib/api'
import { Events, on } from '@/lib/tauri'

interface RouterStore {
  status: RouterStatus | null
  loaded: boolean
  load: () => Promise<void>
}

/** Live state of the Auto router (Lynkr sidecar): pushed by the backend, polled once on first use. */
export const useRouter = create<RouterStore>((set) => ({
  status: null, loaded: false,
  load: async () => { try { set({ status: await api.router.status(), loaded: true }) } catch { set({ loaded: true }) } },
}))

let subscribed = false
export function initRouterStore(): void {
  if (subscribed) return
  subscribed = true
  useRouter.getState().load().catch(() => {})
  on<RouterStatus>(Events.routerStatus, (status) => useRouter.setState({ status, loaded: true }))
}

/** What "Auto" means right now, for the composer's model picker. */
export function autoSummary(status: RouterStatus | null): { routed: boolean; text: string } {
  if (!status || status.state === 'disabled') return { routed: false, text: 'Default model · enable Auto routing in Settings → Models' }
  if (status.state === 'healthy') return { routed: true, text: 'Routes each request across your models (Lynkr)' }
  const why = status.reason ?? ({ 'not-installed': 'router not installed', installing: 'installing router…', starting: 'router starting…', degraded: 'router degraded', error: 'router error' } as Record<string, string>)[status.state] ?? status.state
  return { routed: false, text: `Default model · ${why}` }
}

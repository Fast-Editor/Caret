/**
 * Ordered fan-out for `Events.workspaceChanged`.
 *
 * Opening a folder used to fire a dozen reloads at once (git status, branches, worktrees, remotes, chat threads and
 * metadata, reviews, MCP, rules, index status), all competing with the explorer tree and editor for IPC and CPU.
 * Stores now register with a tier instead of subscribing to the raw event:
 *
 * - `now`   runs synchronously inside the event: store resets and anything the explorer/editor need to render.
 * - `soon`  runs after the new tree has had a frame to paint: git status (status bar, explorer decorations), the
 *           chat thread list.
 * - `later` runs once the window is settled: branches/worktrees/remotes, MCP, rules, index status, chat metadata.
 *
 * A second workspace change before a tier has run cancels the pending tiers of the first one, so clicking through
 * the recent list does not queue a reload per folder.
 */
import type { WorkspaceInfo } from '@/types'
import { Events, on } from './tauri'

export type ReloadTier = 'now' | 'soon' | 'later'
export const TIERS: readonly ReloadTier[] = ['now', 'soon', 'later']
export const TIER_DELAY_MS: Record<ReloadTier, number> = { now: 0, soon: 150, later: 600 }

export type ReloadHandler = (ws: WorkspaceInfo | null) => void

export interface ReloadTimer {
  set: (fn: () => void, ms: number) => unknown
  clear: (id: unknown) => void
}

const realTimer: ReloadTimer = { set: (fn, ms) => setTimeout(fn, ms), clear: (id) => clearTimeout(id as ReturnType<typeof setTimeout>) }

export interface ReloadScheduler {
  /** Register a handler for a tier; returns the unsubscribe function. */
  add: (tier: ReloadTier, fn: ReloadHandler) => () => void
  /** Deliver a workspace change: `now` handlers run before this returns, the other tiers are scheduled. */
  dispatch: (ws: WorkspaceInfo | null) => void
  /** Number of tiers still waiting to run for the latest dispatch (for tests). */
  pending: () => number
}

/** Pure scheduler: handlers run in tier order, delayed tiers are cancelled by a newer dispatch. */
export function createReloadScheduler(timer: ReloadTimer = realTimer, delays: Record<ReloadTier, number> = TIER_DELAY_MS): ReloadScheduler {
  const handlers: { tier: ReloadTier; fn: ReloadHandler }[] = []
  let timers: unknown[] = []
  let generation = 0
  const run = (tier: ReloadTier, ws: WorkspaceInfo | null) => {
    // copy: a handler may unsubscribe (or subscribe another) while we iterate
    for (const h of [...handlers]) if (h.tier === tier) { try { h.fn(ws) } catch (e) { console.error(`[workspace] ${tier} reload failed`, e) } }
  }
  return {
    add: (tier, fn) => { const h = { tier, fn }; handlers.push(h); return () => { const i = handlers.indexOf(h); if (i >= 0) handlers.splice(i, 1) } },
    dispatch: (ws) => {
      generation += 1
      const g = generation
      for (const id of timers) timer.clear(id)
      timers = []
      run('now', ws)
      for (const tier of TIERS) {
        if (tier === 'now') continue
        const id = timer.set(() => { timers = timers.filter((t) => t !== id); if (g === generation) run(tier, ws) }, delays[tier])
        timers.push(id)
      }
    },
    pending: () => timers.length,
  }
}

const scheduler = createReloadScheduler()
let subscribed = false

/** Subscribe to workspace changes in the given tier. Lazily attaches the single backend listener. */
export function onWorkspaceChanged(tier: ReloadTier, fn: ReloadHandler): () => void {
  if (!subscribed) { subscribed = true; on<WorkspaceInfo | null>(Events.workspaceChanged, (ws) => scheduler.dispatch(ws)) }
  return scheduler.add(tier, fn)
}

import { create } from 'zustand'
import type { Notification } from '@/types'

export interface Toast extends Notification { createdAt: number; progress?: number; sticky?: boolean; read?: boolean; leaving?: boolean }
export type OutputLine = { at: number; level: 'log' | 'info' | 'warn' | 'error' | 'debug'; text: string; source: string }

interface NotificationStore {
  toasts: Toast[]
  history: Toast[]
  centerOpen: boolean
  unread: number
  actionHandlers: Map<string, (actionId: string) => void>
  add: (n: Omit<Notification, 'id'> & { id?: string; sticky?: boolean; progress?: number; onAction?: (actionId: string) => void }) => string
  update: (id: string, patch: Partial<Toast>) => void
  dismiss: (id: string) => void
  dismissAll: () => void
  clearHistory: () => void
  setCenterOpen: (o: boolean) => void
  markRead: () => void
}

let counter = 0
const timers = new Map<string, number>()

/** How long a dismissed toast stays mounted (fading) before removal: the fast motion token plus a frame, 0 when motion is off. */
export function leaveDelayMs(root: { dataset?: Record<string, string | undefined>; className?: string } | undefined = typeof document === 'undefined' ? undefined : document.documentElement): number {
  if (!root) return 0
  const level = root.dataset?.motion
  if (level === 'off' || root.className?.split(/\s+/).includes('reduce-motion')) return 0
  return (level === 'expressive' ? 160 : 120) + 16
}

export const useNotifications = create<NotificationStore>((set, get) => ({
  toasts: [], history: [], centerOpen: false, unread: 0, actionHandlers: new Map(),
  add: (n) => {
    const id = n.id ?? `n${++counter}-${Date.now()}`
    const toast: Toast = { id, level: n.level, title: n.title, message: n.message, actions: n.actions, source: n.source, timeoutMs: n.timeoutMs, createdAt: Date.now(), progress: n.progress, sticky: n.sticky }
    if (n.onAction) get().actionHandlers.set(id, n.onAction)
    set((s) => ({ toasts: [...s.toasts.filter((t) => t.id !== id), toast].slice(-6), history: [toast, ...s.history.filter((t) => t.id !== id)].slice(0, 200), unread: s.centerOpen ? s.unread : s.unread + 1 }))
    const timeout = n.timeoutMs ?? (n.level === 'error' ? 10000 : n.sticky ? 0 : 5000)
    if (timeout > 0 && !n.sticky && !n.actions?.length) {
      const t = window.setTimeout(() => get().dismiss(id), timeout)
      timers.set(id, t)
    }
    return id
  },
  update: (id, patch) => set((s) => ({ toasts: s.toasts.map((t) => (t.id === id ? { ...t, ...patch } : t)), history: s.history.map((t) => (t.id === id ? { ...t, ...patch } : t)) })),
  dismiss: (id) => {
    const t = timers.get(id); if (t) { clearTimeout(t); timers.delete(id) }
    const drop = () => set((s) => ({ toasts: s.toasts.filter((x) => x.id !== id) }))
    const ms = leaveDelayMs()
    if (ms === 0 || get().toasts.find((x) => x.id === id)?.leaving) { drop(); return }
    // Mark as leaving so the card fades out, then drop it once the exit motion has played.
    set((s) => ({ toasts: s.toasts.map((x) => (x.id === id ? { ...x, leaving: true } : x)) }))
    setTimeout(drop, ms)
  },
  dismissAll: () => {
    timers.forEach((t) => clearTimeout(t)); timers.clear()
    const ms = leaveDelayMs()
    if (ms === 0) { set({ toasts: [] }); return }
    set((s) => ({ toasts: s.toasts.map((x) => ({ ...x, leaving: true })) }))
    setTimeout(() => set((s) => ({ toasts: s.toasts.filter((x) => !x.leaving) })), ms)
  },
  clearHistory: () => set({ history: [], unread: 0 }),
  setCenterOpen: (o) => set({ centerOpen: o, unread: o ? 0 : get().unread }),
  markRead: () => set({ unread: 0 }),
}))

export function notify(level: Notification['level'], title: string, message?: string, opts?: { timeoutMs?: number; actions?: Notification['actions']; onAction?: (id: string) => void; sticky?: boolean }): string {
  return useNotifications.getState().add({ level, title, message, ...opts })
}

// ───────────── Output channel (console capture + app events) ─────────────
const MAX_LINES = 2000
interface OutputStore { lines: OutputLine[]; append: (l: OutputLine) => void; clear: () => void }
export const useOutput = create<OutputStore>((set) => ({
  lines: [],
  append: (l) => set((s) => ({ lines: s.lines.length >= MAX_LINES ? [...s.lines.slice(s.lines.length - MAX_LINES + 1), l] : [...s.lines, l] })),
  clear: () => set({ lines: [] }),
}))

let consoleHooked = false
export function hookConsole(): void {
  if (consoleHooked) return
  consoleHooked = true
  const fmt = (args: unknown[]) => args.map((a) => (typeof a === 'string' ? a : a instanceof Error ? `${a.name}: ${a.message}` : safeJson(a))).join(' ')
  for (const level of ['log', 'info', 'warn', 'error', 'debug'] as const) {
    const orig = console[level].bind(console)
    console[level] = (...args: unknown[]) => {
      orig(...args)
      try { useOutput.getState().append({ at: Date.now(), level, text: fmt(args).slice(0, 4000), source: 'console' }) } catch { /* ignore */ }
    }
  }
  window.addEventListener('error', (e) => useOutput.getState().append({ at: Date.now(), level: 'error', text: `Uncaught: ${e.message} (${e.filename}:${e.lineno})`, source: 'window' }))
  window.addEventListener('unhandledrejection', (e) => useOutput.getState().append({ at: Date.now(), level: 'error', text: `Unhandled rejection: ${String((e.reason as Error)?.message ?? e.reason)}`, source: 'window' }))
}
function safeJson(v: unknown): string { try { return JSON.stringify(v) ?? String(v) } catch { return String(v) } }

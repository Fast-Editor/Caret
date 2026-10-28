import { create } from 'zustand'
import type { DapScope, DapStackFrame, DapThread, DapVariable, DebugAdapterStatus, DebugBreakpoint, DebugEvent, DebugSessionInfo, DebugSessionState, LaunchConfig } from '@/types'
import { api } from '@/lib/api'
import { Events, on } from '@/lib/tauri'
import { runCommand } from '@/lib/commands'
import { notify } from '@/store/notifications'
import { activeTab } from '@/store/editor'

export interface ConsoleLine { id: number; category: string; text: string; source?: string; line?: number }
export interface DebugLocation { path: string; line: number; column: number }

interface DebugStore {
  sessions: Record<string, DebugSessionInfo>
  activeSessionId: string | null
  state: DebugSessionState | 'idle'
  threads: DapThread[]
  stoppedThreadId: number | null
  stopReason: string | null
  frames: DapStackFrame[]
  selectedFrameId: number | null
  scopes: DapScope[]
  /** variablesReference → children (also used for scope roots) */
  variables: Record<number, DapVariable[]>
  expanded: Record<number, boolean>
  watches: string[]
  watchValues: Record<string, string>
  /** absolute path → 1-based lines the user set */
  breakpoints: Record<string, number[]>
  /** absolute path → adapter verification for the current session */
  verified: Record<string, DebugBreakpoint[]>
  currentLocation: DebugLocation | null
  console: ConsoleLine[]
  configs: LaunchConfig[]
  selectedConfig: string | null
  adapters: DebugAdapterStatus[]
  installing: string | null
  loaded: boolean
  // actions
  init: () => void
  loadConfigs: () => Promise<void>
  loadAdapters: () => Promise<void>
  installAdapter: (id: string) => Promise<void>
  selectConfig: (name: string) => void
  start: (config?: LaunchConfig) => Promise<void>
  stop: () => Promise<void>
  continue: () => Promise<void>
  next: () => Promise<void>
  stepIn: () => Promise<void>
  stepOut: () => Promise<void>
  pause: () => Promise<void>
  restart: () => Promise<void>
  selectFrame: (frameId: number, reveal?: boolean) => Promise<void>
  toggleExpanded: (ref: number) => Promise<void>
  toggleBreakpoint: (file: string, line: number) => Promise<void>
  setBreakpoints: (file: string, lines: number[]) => Promise<void>
  removeAllBreakpoints: () => Promise<void>
  addWatch: (expr: string) => void
  removeWatch: (expr: string) => void
  evaluate: (expr: string) => Promise<void>
  clearConsole: () => void
  onEvent: (ev: DebugEvent) => void
}

const BP_KEY = 'caret.debug.breakpoints'
let lineSeq = 1
const ANSI = /\u001b\[[0-9;?]*[ -/]*[@-~]/g
function loadBreakpoints(): Record<string, number[]> { try { return JSON.parse(localStorage.getItem(BP_KEY) ?? '{}') } catch { return {} } }
function saveBreakpoints(b: Record<string, number[]>) { try { localStorage.setItem(BP_KEY, JSON.stringify(b)) } catch { /* ignore */ } }

export const useDebug = create<DebugStore>((set, get) => ({
  sessions: {}, activeSessionId: null, state: 'idle', threads: [], stoppedThreadId: null, stopReason: null, frames: [], selectedFrameId: null, scopes: [], variables: {}, expanded: {},
  watches: [], watchValues: {}, breakpoints: loadBreakpoints(), verified: {}, currentLocation: null, console: [], configs: [], selectedConfig: null, adapters: [], installing: null, loaded: false,

  init: () => {
    if (get().loaded) return
    set({ loaded: true })
    void get().loadAdapters()
    void get().loadConfigs()
    // push persisted breakpoints to the backend so the first session receives them
    for (const [file, lines] of Object.entries(get().breakpoints)) { if (lines.length) api.debug.setBreakpoints(file, lines).catch(() => {}) }
  },
  loadConfigs: async () => {
    const file = activeTab()?.path ?? null
    try {
      const configs = await api.debug.listConfigs(file ?? undefined)
      const sel = get().selectedConfig
      set({ configs, selectedConfig: configs.some((c) => c.name === sel) ? sel : configs[0]?.name ?? null })
    } catch { set({ configs: [] }) }
  },
  loadAdapters: async () => { try { set({ adapters: await api.debug.adapters() }) } catch { /* backend missing */ } },
  installAdapter: async (id) => {
    set({ installing: id })
    try { await api.debug.installAdapter(id); notify('success', 'Debug adapter installed', id) }
    catch (e) { notify('error', `Could not install ${id}`, String((e as Error)?.message ?? e)) }
    finally { set({ installing: null }); await get().loadAdapters() }
  },
  selectConfig: (name) => set({ selectedConfig: name }),

  start: async (config) => {
    const cfg = config ?? get().configs.find((c) => c.name === get().selectedConfig) ?? get().configs[0]
    if (!cfg) { notify('warn', 'Nothing to run', 'Open a JavaScript, TypeScript or Python file, or add a .vscode/launch.json.'); return }
    const adapter = get().adapters.find((a) => a.types.includes(cfg.type))
    if (adapter && !adapter.installed) { notify('warn', `${adapter.name} is not installed`, 'Install it from the Run and Debug view.'); return }
    set({ console: [], frames: [], scopes: [], variables: {}, threads: [], currentLocation: null, verified: {} })
    try {
      const info = await api.debug.start(cfg)
      set((s) => ({ sessions: { ...s.sessions, [info.id]: info }, activeSessionId: info.id, state: info.state }))
    } catch (e) { notify('error', 'Could not start debugging', String((e as Error)?.message ?? e)); set({ state: 'idle' }) }
  },
  stop: async () => { const id = get().activeSessionId; if (!id) return; try { await api.debug.stop(id) } catch { /* already gone */ } finish(id) },
  continue: async () => { const { activeSessionId: id, stoppedThreadId: t } = get(); if (!id || t === null) return; await api.debug.continue(id, t).catch(err) },
  next: async () => { const { activeSessionId: id, stoppedThreadId: t } = get(); if (!id || t === null) return; await api.debug.next(id, t).catch(err) },
  stepIn: async () => { const { activeSessionId: id, stoppedThreadId: t } = get(); if (!id || t === null) return; await api.debug.stepIn(id, t).catch(err) },
  stepOut: async () => { const { activeSessionId: id, stoppedThreadId: t } = get(); if (!id || t === null) return; await api.debug.stepOut(id, t).catch(err) },
  pause: async () => { const { activeSessionId: id, threads } = get(); if (!id) return; const t = threads[0]?.id ?? 1; await api.debug.pause(id, t).catch(err) },
  restart: async () => { const id = get().activeSessionId; if (!id) return; const cfg = get().configs.find((c) => c.name === get().selectedConfig); await api.debug.restart(id).catch(async () => { await get().stop(); await get().start(cfg) }) },

  selectFrame: async (frameId, reveal = true) => {
    const id = get().activeSessionId; if (!id) return
    const frame = get().frames.find((f) => f.id === frameId)
    set({ selectedFrameId: frameId, scopes: [], variables: {}, expanded: {} })
    if (frame?.path) { set({ currentLocation: { path: frame.path, line: frame.line, column: frame.column } }); if (reveal) void runCommand('editor.openFile', { path: frame.path, line: frame.line, column: frame.column, preview: true, focus: false }) }
    try {
      const scopes = await api.debug.scopes(id, frameId)
      set({ scopes })
      const first = scopes.find((s) => !s.expensive) ?? scopes[0]
      if (first && first.variablesReference > 0) { const vars = await api.debug.variables(id, first.variablesReference); set((s) => ({ variables: { ...s.variables, [first.variablesReference]: vars }, expanded: { ...s.expanded, [first.variablesReference]: true } })) }
    } catch { /* adapter may be running again */ }
    await refreshWatches()
  },
  toggleExpanded: async (ref) => {
    const id = get().activeSessionId; if (!id || ref <= 0) return
    const open = !get().expanded[ref]
    set((s) => ({ expanded: { ...s.expanded, [ref]: open } }))
    if (open && !get().variables[ref]) { try { const vars = await api.debug.variables(id, ref); set((s) => ({ variables: { ...s.variables, [ref]: vars } })) } catch { /* ignore */ } }
  },
  toggleBreakpoint: async (file, line) => {
    const cur = get().breakpoints[file] ?? []
    const lines = cur.includes(line) ? cur.filter((l) => l !== line) : [...cur, line].sort((a, b) => a - b)
    await get().setBreakpoints(file, lines)
  },
  setBreakpoints: async (file, lines) => {
    const breakpoints = { ...get().breakpoints }
    if (lines.length) breakpoints[file] = [...lines].sort((a, b) => a - b); else delete breakpoints[file]
    set({ breakpoints }); saveBreakpoints(breakpoints)
    try { const verified = await api.debug.setBreakpoints(file, lines); set((s) => ({ verified: { ...s.verified, [file]: verified } })) } catch { /* backend unavailable */ }
  },
  removeAllBreakpoints: async () => { for (const file of Object.keys(get().breakpoints)) await get().setBreakpoints(file, []) },
  addWatch: (expr) => { const e = expr.trim(); if (!e || get().watches.includes(e)) return; set((s) => ({ watches: [...s.watches, e] })); void refreshWatches() },
  removeWatch: (expr) => set((s) => { const wv = { ...s.watchValues }; delete wv[expr]; return { watches: s.watches.filter((w) => w !== expr), watchValues: wv } }),
  evaluate: async (expr) => {
    const id = get().activeSessionId; const e = expr.trim(); if (!e) return
    push({ category: 'input', text: e })
    if (!id) { push({ category: 'stderr', text: 'No debug session is running.' }); return }
    try { const r = await api.debug.evaluate(id, e, get().selectedFrameId ?? undefined, 'repl'); push({ category: 'result', text: r.result }) }
    catch (err) { push({ category: 'stderr', text: String((err as Error)?.message ?? err) }) }
  },
  clearConsole: () => set({ console: [] }),

  onEvent: (ev) => {
    const s = get()
    if (ev.type === 'sessionStarted') { set((st) => ({ activeSessionId: st.activeSessionId ?? ev.sessionId, state: 'starting' })); return }
    if (ev.sessionId !== s.activeSessionId && s.activeSessionId !== null) return
    switch (ev.type) {
      case 'initialized': break
      case 'state': set({ state: ev.state }); break
      case 'stopped': {
        set({ state: 'stopped', stoppedThreadId: ev.threadId, stopReason: ev.description ?? ev.reason })
        if (ev.text) push({ category: 'stderr', text: ev.text })
        void (async () => {
          const id = ev.sessionId
          try {
            const threads = await api.debug.threads(id); set({ threads })
            const frames = await api.debug.stackTrace(id, ev.threadId); set({ frames })
            const top = frames.find((f) => f.path) ?? frames[0]
            if (top) await get().selectFrame(top.id, true)
          } catch { /* session may have ended */ }
        })()
        break
      }
      case 'continued': set({ state: 'running', stoppedThreadId: null, stopReason: null, frames: [], scopes: [], variables: {}, currentLocation: null }); break
      case 'output': push({ category: ev.category, text: ev.output.replace(ANSI, ''), source: ev.source, line: ev.line }); break
      case 'breakpoint': {
        const bp = ev.breakpoint; if (!bp.path) break
        set((st) => { const list = (st.verified[bp.path!] ?? []).filter((b) => b.id !== bp.id && b.line !== bp.line); return { verified: { ...st.verified, [bp.path!]: [...list, bp] } } })
        break
      }
      case 'thread': break
      case 'exited': push({ category: 'console', text: `Process exited with code ${ev.exitCode}` }); break
      case 'error': notify('error', 'Debugger', ev.message); push({ category: 'stderr', text: ev.message }); break
      case 'terminated': finish(ev.sessionId); break
    }
  },
}))

function err(e: unknown) { notify('warn', 'Debugger', String((e as Error)?.message ?? e)) }
function push(l: Omit<ConsoleLine, 'id'>) { useDebug.setState((s) => ({ console: [...s.console.slice(-1999), { id: lineSeq++, ...l }] })) }
function finish(sessionId: string) {
  useDebug.setState((s) => { const sessions = { ...s.sessions }; delete sessions[sessionId]; const active = s.activeSessionId === sessionId ? null : s.activeSessionId; return { sessions, activeSessionId: active, state: active ? s.state : 'terminated', stoppedThreadId: null, frames: [], scopes: [], variables: {}, currentLocation: null, threads: [], verified: {} } })
  window.setTimeout(() => { if (useDebug.getState().state === 'terminated') useDebug.setState({ state: 'idle' }) }, 1500)
}
async function refreshWatches() {
  const { activeSessionId: id, watches, selectedFrameId } = useDebug.getState(); if (!id || !watches.length) return
  const values: Record<string, string> = {}
  for (const w of watches) { try { values[w] = (await api.debug.evaluate(id, w, selectedFrameId ?? undefined, 'watch')).result } catch (e) { values[w] = `⚠ ${String((e as Error)?.message ?? e)}` } }
  useDebug.setState({ watchValues: values })
}

let subscribed = false
/** Subscribe to backend debug events once (called from the debug module init). */
export function initDebugStore(): () => void {
  if (subscribed) return () => {}
  subscribed = true
  useDebug.getState().init()
  return on<DebugEvent>(Events.debugEvent, (ev) => useDebug.getState().onEvent(ev))
}

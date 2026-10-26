/**
 * `caret` command-line launcher glue: drains the backend's pending-open queue (argv, Finder "Open with",
 * `caret://open?…`) and routes each request to the workspace/editor; registers the install/uninstall commands.
 * The backend pokes us with `caret://open-paths`; we always drain the queue so each request is handled once.
 */
import { api } from './api'
import { on } from './tauri'
import { registerCommands, runCommand, hasCommand } from './commands'
import { useWorkspace } from '@/store/workspace'
import type { OpenRequest, CliStatus } from '@/types'

export const OPEN_PATHS_EVENT = 'caret://open-paths'

function parentDir(p: string): string { const i = p.lastIndexOf('/'); return i <= 0 ? '/' : p.slice(0, i) }

async function focusWindow(): Promise<void> {
  try { const { getCurrentWindow } = await import('@tauri-apps/api/window'); const w = getCurrentWindow(); await w.show(); await w.setFocus() } catch { /* not fatal */ }
}

function workspaceReady(): Promise<void> {
  return new Promise((res) => {
    if (useWorkspace.getState().loaded) return res()
    const u = useWorkspace.subscribe((s) => { if (s.loaded) { u(); res() } })
  })
}

/** Open one request: folders become the workspace; files open in the editor (their folder becomes the workspace when none is open). */
export async function handleOpenRequest(r: OpenRequest): Promise<void> {
  const st = await api.fs.stat(r.path).catch(() => null)
  if (!st?.exists) { void runCommand('app.notify', { level: 'warn', title: 'Cannot open', message: `${r.path} does not exist` }); return }
  const path = st.path || r.path
  const ws = useWorkspace.getState()
  if (st.kind === 'dir') {
    if (ws.workspace?.root !== path) await ws.open(path)
    return
  }
  if (!ws.workspace) await ws.open(parentDir(path))
  if (hasCommand('editor.openFile')) await runCommand('editor.openFile', { path, line: r.line, column: r.col, preview: false, focus: true })
}

let chain: Promise<void> = Promise.resolve()
/** Drain the backend queue and handle every request in order (serialised across pokes). */
export function pumpPendingOpens(): Promise<void> {
  chain = chain.then(async () => {
    let reqs: OpenRequest[] = []
    try { reqs = await api.cli.takePendingOpens() } catch { return }
    if (reqs.length === 0) return
    await workspaceReady()
    for (const r of reqs) { try { await handleOpenRequest(r) } catch (e) { console.error('[cli] open failed', r, e) } }
    void focusWindow()
  }).catch(() => {})
  return chain
}

function describe(s: CliStatus): string {
  if (s.installed) return `${s.path} → ${s.scriptPath}`
  if (s.onPath) return `a different 'caret' is on your PATH${s.target ? ` (${s.target})` : ''}`
  return `not installed (script at ${s.scriptPath})`
}

export function initCliOpen(): () => void {
  const offEvent = on<unknown>(OPEN_PATHS_EVENT, () => { void pumpPendingOpens() })
  void pumpPendingOpens()
  const offCommands = registerCommands([
    { id: 'cli.install', title: "Shell Command: Install 'caret' command in PATH", category: 'Shell', run: async () => {
      const s = await api.cli.install()
      if (s.error) void runCommand('app.notify', { level: 'error', title: "Could not install 'caret'", message: s.error })
      else void runCommand('app.notify', { level: 'success', title: s.installed ? "'caret' command installed" : "'caret' script written", message: s.installed ? `Run \`caret .\` from any terminal (${s.path}).` : describe(s) })
      return s
    } },
    { id: 'cli.uninstall', title: "Shell Command: Uninstall 'caret' command from PATH", category: 'Shell', run: async () => {
      const s = await api.cli.uninstall()
      if (s.error) void runCommand('app.notify', { level: 'error', title: "Could not uninstall 'caret'", message: s.error })
      else void runCommand('app.notify', { level: 'info', title: "'caret' command removed", message: `${s.scriptPath} is kept; delete it if you no longer want the launcher.` })
      return s
    } },
    { id: 'cli.status', title: 'Shell Command: Status', hidden: true, run: () => api.cli.status() },
    { id: 'cli.open', title: 'Open Path (CLI)', hidden: true, run: (a) => handleOpenRequest(a as OpenRequest) },
  ])
  return () => { offEvent(); offCommands() }
}

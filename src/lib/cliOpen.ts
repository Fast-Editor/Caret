/**
 * `caret` command-line launcher glue: drains this window's pending-open queue (argv, Finder "Open with",
 * `caret://open?…`) and routes each request to the workspace/editor; registers the install/uninstall commands.
 * The backend router (`core/cli.rs::route_open`, docs/CLI.md "Which window opens it") decides which window a
 * request belongs to and pokes only that window with `caret://open-paths`; `cli_take_pending_opens` hands a window
 * its own requests, so each request is handled once, by the window the router chose.
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

/** Longest wait for the workspace store's first load before requests are handled anyway. */
export const WORKSPACE_READY_TIMEOUT_MS = 5_000

/**
 * Wait for the workspace store's first load, so a folder the backend opened before this window booted (a window
 * created for a `caret <folder>`) is known before its files are opened. Capped: a window may legitimately have no
 * folder (loose files routed to an empty window), and a slow or failed load must not hold the requests forever.
 */
function workspaceReady(timeoutMs = WORKSPACE_READY_TIMEOUT_MS): Promise<void> {
  return new Promise((res) => {
    if (useWorkspace.getState().loaded) return res()
    let done = false
    const finish = () => { if (done) return; done = true; clearTimeout(timer); unsubscribe(); res() }
    const timer = setTimeout(finish, timeoutMs)
    const unsubscribe = useWorkspace.subscribe((s) => { if (s.loaded) finish() })
  })
}

/**
 * Open one request in this window. The backend router only hands a window the requests meant for it, so there is
 * nothing to decide here about *which* window: a folder arrives when this window already shows it (then only the
 * window focus is needed), when it is empty, or when `window.openFoldersIn` is `"current"` (then it opens here);
 * a file arrives when it lives under this window's folder or is a loose file sent to the focused window.
 */
export async function handleOpenRequest(r: OpenRequest): Promise<void> {
  const st = await api.fs.stat(r.path).catch(() => null)
  if (!st?.exists) { void runCommand('app.notify', { level: 'warn', title: 'Cannot open', message: `${r.path} does not exist` }); return }
  const path = st.path || r.path
  const ws = useWorkspace.getState()
  if (st.kind === 'dir') {
    // Already this window's folder: the pump brings the window to the front; nothing to replace.
    if (ws.workspace?.root === path) return
    await ws.open(path)
    return
  }
  // Caret shows the editor only inside a workspace, so a loose file reaching a window without one opens its folder
  // first. The router sent the file here on purpose; this only decides how this window shows it.
  if (!ws.workspace) await ws.open(parentDir(path))
  if (hasCommand('editor.openFile')) await runCommand('editor.openFile', { path, line: r.line, column: r.col, preview: false, focus: true })
}

let chain: Promise<void> = Promise.resolve()
/** Drain this window's backend queue and handle every request in order (serialised across pokes). */
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

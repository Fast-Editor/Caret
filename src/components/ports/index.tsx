import { useEffect } from 'react'
import { ExternalLink, Copy, Square, RefreshCw, Radio } from 'lucide-react'
import { IconButton } from '@/components/common'
import { confirm } from '@/components/dialogs'
import { api } from '@/lib/api'
import { registerCommands } from '@/lib/commands'
import { notify } from '@/store/notifications'
import { useUi } from '@/store/ui'
import { usePorts, portUrl, originLabel, addressLabel } from '@/store/ports'
import type { ListeningPort } from '@/types'

export { usePortsCount } from '@/store/ports'

const REFRESH_MS = 3000

async function copyUrl(p: ListeningPort) {
  try { const { writeText } = await import('@tauri-apps/plugin-clipboard-manager'); await writeText(portUrl(p)); notify('success', 'URL copied', portUrl(p), { timeoutMs: 1500 }) }
  catch { try { await navigator.clipboard.writeText(portUrl(p)) } catch { /* ignore */ } }
}

async function killPort(p: ListeningPort) {
  const ok = await confirm({ title: `Stop ${p.process || 'process'} (pid ${p.pid})?`, message: `This terminates the process listening on port ${p.port}.`, okLabel: 'Stop process', danger: true })
  if (!ok) return
  try { await usePorts.getState().kill(p.pid); notify('info', `Stopped ${p.process || 'process'} on port ${p.port}`) }
  catch (e) { notify('error', 'Could not stop process', String((e as Error)?.message ?? e)) }
}

// Hidden commands (palette-less) so other modules can refresh/open without importing this component.
let registered = false
if (!registered) {
  registered = true
  registerCommands([
    { id: 'ports.refresh', title: 'Ports: Refresh', category: 'View', hidden: true, run: () => usePorts.getState().refresh() },
    { id: 'ports.open', title: 'Ports: Open in Browser', category: 'View', hidden: true, run: (args) => { const a = (args ?? {}) as { port?: number }; if (a.port) void api.app.openExternal(`http://localhost:${a.port}`) } },
  ])
}

/** Bottom-panel view: local servers currently listening, with open / copy / stop actions. Refreshes while visible. */
export function PortsPanel() {
  const { ports, loading, error, lastUpdated, refresh } = usePorts()
  const visible = useUi((s) => s.panelVisible && s.panelView === 'ports')
  useEffect(() => {
    if (!visible) return
    void refresh()
    const id = window.setInterval(() => { if (document.visibilityState === 'visible') void refresh() }, REFRESH_MS)
    return () => window.clearInterval(id)
  }, [visible, refresh])
  return (
    <div className="h-full flex flex-col text-xs" data-testid="ports-panel">
      <div className="flex items-center gap-2 px-3 h-7 border-b border-border text-muted">
        <Radio size={12} className="text-faint" />
        <span>{ports.length === 0 ? 'No local servers listening' : `${ports.length} local server${ports.length === 1 ? '' : 's'} listening`}</span>
        {lastUpdated && <span className="text-faint">· updated {new Date(lastUpdated).toLocaleTimeString()}</span>}
        <span className="flex-1" />
        <IconButton label="Refresh" icon={<RefreshCw size={12} className={loading ? 'spin' : ''} />} onClick={() => void refresh()} />
      </div>
      {error && <div className="px-3 py-2 text-danger">{error}</div>}
      {ports.length > 0 && (
        <div className="flex-1 overflow-auto selectable">
          <table className="w-full border-collapse">
            <thead className="sticky top-0 bg-panel text-faint text-[10.5px] uppercase tracking-wide">
              <tr className="text-left"><th className="px-3 py-1 font-medium w-20">Port</th><th className="px-3 py-1 font-medium">Process</th><th className="px-3 py-1 font-medium w-36">Address</th><th className="px-3 py-1 font-medium w-32">Origin</th><th className="px-3 py-1 w-28" /></tr>
            </thead>
            <tbody>
              {ports.map((p) => (
                <tr key={`${p.pid}:${p.port}:${p.address}`} className="h-7 border-t border-border/60 hover:bg-hover cursor-pointer group" onDoubleClick={() => void api.app.openExternal(portUrl(p))} title={p.command}>
                  <td className="px-3 mono">{p.port}</td>
                  <td className="px-3 truncate max-w-[420px]"><span className="text-fg">{p.process || '—'}</span>{p.command && p.command !== p.process && <span className="text-faint ml-2 truncate">{p.command.length > 80 ? p.command.slice(0, 80) + '…' : p.command}</span>}<span className="text-faint ml-2">pid {p.pid}</span></td>
                  <td className="px-3 mono text-muted">{addressLabel(p.address)}</td>
                  <td className="px-3">{p.ownedByCaret ? <span className="px-1.5 py-0.5 rounded border border-border text-[10.5px] text-fg">{originLabel(p)}</span> : <span className="text-faint">{originLabel(p)}</span>}</td>
                  <td className="px-2">
                    <span className="flex items-center justify-end gap-0.5 opacity-70 group-hover:opacity-100">
                      <IconButton label={`Open ${portUrl(p)} in browser`} icon={<ExternalLink size={12} />} onClick={(e) => { e.stopPropagation(); void api.app.openExternal(portUrl(p)) }} />
                      <IconButton label="Copy URL" icon={<Copy size={12} />} onClick={(e) => { e.stopPropagation(); void copyUrl(p) }} />
                      <IconButton label="Stop process" icon={<Square size={11} />} onClick={(e) => { e.stopPropagation(); void killPort(p) }} />
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {ports.length === 0 && !error && <div className="flex-1 flex items-center justify-center text-faint">Servers started from Caret's terminals (or anywhere on this machine) appear here with their port and process.</div>}
    </div>
  )
}

import { useCallback, useEffect, useState } from 'react'
import { Copy, KeyRound, Laptop, RefreshCw, ShieldAlert, Trash2 } from 'lucide-react'
import { SchemaPage } from '../SchemaPage'
import { Badge, Button, Empty, Row, Section, Toggle, confirmDialog, errorMessage, notify, relativeTime } from '../primitives'
import { api } from '@/lib/api'
import { on } from '@/lib/tauri'
import { STATUS_EVENT } from '@/lib/remoteExecutor'
import { useSettings } from '@/store/settings'
import type { RemotePairingCode, RemoteStatus } from '@/types'

const isRemoteSession = () => typeof window !== 'undefined' && '__CARET_REMOTE__' in window

function fmtCountdown(secs: number): string { const m = Math.floor(secs / 60); const s = secs % 60; return `${m}:${s.toString().padStart(2, '0')}` }

/** Settings → Remote: serve the Caret UI to browsers on the LAN / Tailscale (docs/REMOTE.md). */
export function RemotePage() {
  const [status, setStatus] = useState<RemoteStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [code, setCode] = useState<RemotePairingCode | null>(null)
  const [now, setNow] = useState(() => Date.now())
  const settings = useSettings((s) => s.settings)
  const remoteSession = isRemoteSession()

  const refresh = useCallback(() => api.remote.status().then(setStatus).catch(() => setStatus(null)), [])
  useEffect(() => { void refresh(); return on<RemoteStatus>(STATUS_EVENT, setStatus) }, [refresh])
  useEffect(() => { if (!code) return; const t = window.setInterval(() => setNow(Date.now()), 500); return () => window.clearInterval(t) }, [code])
  const remaining = code ? Math.max(0, Math.floor((code.expiresAt - now) / 1000)) : 0
  useEffect(() => { if (code && remaining === 0) setCode(null) }, [code, remaining])

  const toggle = async (enabled: boolean) => {
    setBusy(true)
    try { setStatus(await api.remote.setEnabled(enabled)) }
    catch (e) { notify('error', enabled ? 'Could not start remote access' : 'Could not stop remote access', errorMessage(e)); void refresh() }
    finally { setBusy(false) }
  }
  const restart = async () => {
    setBusy(true)
    try { setStatus(await api.remote.restart()); notify('success', 'Remote server restarted') }
    catch (e) { notify('error', 'Could not restart the remote server', errorMessage(e)); void refresh() }
    finally { setBusy(false) }
  }
  const showCode = async () => {
    try { setNow(Date.now()); setCode(await api.remote.pairingCode()) }
    catch (e) { notify('error', 'Could not create a pairing code', errorMessage(e)) }
  }
  const revoke = async (id: string, name: string) => {
    const ok = await confirmDialog({ title: `Revoke ${name}?`, message: 'The browser is disconnected immediately and has to pair again with a new code.', confirmLabel: 'Revoke', danger: true })
    if (!ok) return
    try { setStatus(await api.remote.revokeDevice(id)) } catch (e) { notify('error', 'Could not revoke the device', errorMessage(e)) }
  }
  const copy = (text: string) => { void navigator.clipboard?.writeText(text).then(() => notify('success', 'Copied', text)).catch(() => notify('warn', 'Could not copy', text)) }

  const running = !!status?.running
  const needsRestart = running && !!settings && !status?.serving && (status!.host !== settings.remote.host || status!.port !== settings.remote.port)
  const stateBadge = !status ? null : status.error ? <Badge tone="danger">Error</Badge> : running ? <Badge tone="success">Running</Badge> : <Badge>Stopped</Badge>

  return (
    <SchemaPage page="remote"
      before={
        <Section title="Remote access" description="Serve this Caret to a browser on another device. Everything runs on this machine; the browser is a thin client relayed through this window, so keep Caret open." actions={stateBadge}>
          <Row title="Enable remote access" description={status?.serving ? 'Started from the command line (caret serve); stop it with Ctrl-C in that terminal.' : 'Starts the HTTP and WebSocket server on the address and port below.'}>
            <Toggle checked={!!status?.enabled} onChange={(v) => void toggle(v)} disabled={busy || !status || !!status.serving || remoteSession} label="Enable remote access" />
          </Row>
          {status?.error && <div className="px-3 py-2 text-[11px] text-danger">{status.error}</div>}
          {running && (
            <Row title="Open in a browser" description={status!.host === '127.0.0.1' ? 'Bound to 127.0.0.1: reachable from this Mac only. Choose 0.0.0.0 below for LAN and Tailscale.' : 'Any of these addresses works from a device on the same network or tailnet.'} vertical>
              <ul className="flex flex-col gap-1">
                {status!.urls.map((u) => (
                  <li key={u} className="flex items-center gap-2">
                    <span className="mono text-xs selectable">{u}</span>
                    <button type="button" className="text-muted hover:text-fg" title="Copy URL" aria-label={`Copy ${u}`} onClick={() => copy(u)}><Copy size={12} /></button>
                  </li>
                ))}
              </ul>
            </Row>
          )}
          {running && (
            <Row title="Status" description={`Desktop executor ${status!.executorConnected ? 'connected' : 'not connected: relayed commands fail until this window reconnects'}. ${status!.clients} browser${status!.clients === 1 ? '' : 's'} connected.`}>
              <span className={`text-xs ${status!.executorConnected ? 'text-success' : 'text-warning'}`}>{status!.executorConnected ? 'Ready' : 'Waiting for executor'}</span>
            </Row>
          )}
          {needsRestart && (
            <Row title="Apply new address or port" description={`Running on ${status!.host}:${status!.port}; settings say ${settings!.remote.host}:${settings!.remote.port}.`}>
              <Button onClick={() => void restart()} loading={busy}><RefreshCw size={12} />Restart server</Button>
            </Row>
          )}
        </Section>
      }
      after={
        <>
          <Section title="Pairing" description="A browser pairs once with a six digit code (valid five minutes, single use) and then keeps a token." actions={<Button variant="primary" onClick={() => void showCode()} disabled={!running || remoteSession}><KeyRound size={12} />Show pairing code</Button>}>
            {code ? (
              <div className="px-3 py-4 flex flex-col items-center gap-2">
                <div className="mono text-[40px] font-semibold tracking-[0.35em] selectable" aria-label="Pairing code">{code.code}</div>
                <div className="text-[11px] text-muted">Expires in <span className="mono">{fmtCountdown(remaining)}</span>. Open {status?.urls[0] ?? 'the URL above'} on the other device and enter the code.</div>
                <div className="flex gap-2"><Button size="sm" onClick={() => copy(code.code)}><Copy size={12} />Copy code</Button><Button size="sm" onClick={() => void showCode()}><RefreshCw size={12} />New code</Button></div>
              </div>
            ) : (
              <div className="px-3 py-2.5 text-[11px] text-muted">{running ? 'Press Show pairing code, then enter it on the pairing page the browser shows.' : remoteSession ? 'Pairing is managed from the Caret desktop, not from a remote session.' : 'Enable remote access first.'}</div>
            )}
          </Section>
          <Section title="Paired devices" description="Each device holds a token with full control of this machine. Revoke anything you do not recognise.">
            {!status || status.devices.length === 0 ? (
              <Empty icon={<Laptop size={18} />} title="No paired devices" description="Devices appear here after they pair with a code." />
            ) : (
              <table className="w-full text-xs">
                <thead><tr className="text-[10px] uppercase tracking-wide text-faint text-left"><th className="px-3 py-1.5 font-medium">Device</th><th className="px-3 py-1.5 font-medium">Browser</th><th className="px-3 py-1.5 font-medium">Paired</th><th className="px-3 py-1.5 font-medium">Last seen</th><th className="px-3 py-1.5" /></tr></thead>
                <tbody>
                  {status.devices.map((d) => (
                    <tr key={d.id} className="border-t border-border">
                      <td className="px-3 py-2 font-medium">{d.name}</td>
                      <td className="px-3 py-2 text-muted max-w-[260px]"><span className="block truncate" title={d.userAgent}>{d.userAgent || '…'}</span></td>
                      <td className="px-3 py-2 text-muted">{relativeTime(d.createdAt)}</td>
                      <td className="px-3 py-2 text-muted">{relativeTime(d.lastSeen)}</td>
                      <td className="px-3 py-2 text-right"><Button size="sm" variant="danger" onClick={() => void revoke(d.id, d.name)} disabled={remoteSession}><Trash2 size={12} />Revoke</Button></td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </Section>
          <Section title="Security">
            <div className="m-3 rounded-md border border-warning/40 bg-warning/10 p-3 flex gap-3 text-[11px]">
              <ShieldAlert size={16} className="text-warning shrink-0" />
              <ul className="flex flex-col gap-1 list-disc pl-4">
                <li>Anyone holding a paired token has full control of this machine: files, terminals, git and the agent. Pair only devices you own and revoke them when done.</li>
                <li>Prefer Tailscale (or another VPN): bind to 0.0.0.0, open the tailnet address, and nothing is exposed to the public internet.</li>
                <li>The connection is plain HTTP. Use it on trusted networks only, or put a TLS proxy or an SSH tunnel in front of it.</li>
                <li>Pairing codes expire after five minutes, work once and are limited to five attempts per minute per address. Tokens are stored hashed in ~/.caret/remote/devices.json.</li>
              </ul>
            </div>
          </Section>
        </>
      }
    />
  )
}

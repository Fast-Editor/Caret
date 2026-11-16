import { useCallback, useEffect, useMemo, useState } from 'react'
import { Cloud, CloudOff, KeyRound, MoreHorizontal, Play, Plug, Plus, RefreshCw, SquareTerminal, Trash2 } from 'lucide-react'
import { SchemaPage } from '../SchemaPage'
import { FieldControl } from '../FieldControl'
import { Badge, Button, Empty, IconButton, Menu, Row, Section, Select, TextInput, confirmDialog, errorMessage, formatBytes, notify, promptDialog } from '../primitives'
import { useField } from '../useField'
import { api } from '@/lib/api'
import { on } from '@/lib/tauri'
import { runCommand } from '@/lib/commands'
import { fieldsForPage } from '@/lib/settingsSchema'
import { WARREN_CHANGED_EVENT, availableActions, formatMicros, isTransitional, parseCommaList, relativeTime, shapeLabel, sortInstances, statusTone } from '@/lib/warren'
import { useSettings } from '@/store/settings'
import type { WarrenExecResult, WarrenInstance, WarrenInstanceAction, WarrenStatus } from '@/types'

const POLL_MS = 5000
const BADGE_TONE = { success: 'success', warning: 'warning', danger: 'danger', muted: 'default' } as const
const ACTION_LABEL: Record<WarrenInstanceAction, string> = { start: 'Start', stop: 'Stop', sleep: 'Sleep', restart: 'Restart', delete: 'Delete…' }

/** Settings → Sandboxes: Warren connection, instance defaults and a live view of the workspace (docs/WARREN.md). */
export function SandboxesPage() {
  const warren = useSettings((s) => s.settings?.warren)
  const enabled = !!warren?.enabled
  const [status, setStatus] = useState<WarrenStatus | null>(null)
  const [testing, setTesting] = useState(false)
  const [tested, setTested] = useState(false)
  const [keyDraft, setKeyDraft] = useState('')
  const [savingKey, setSavingKey] = useState(false)
  const [instances, setInstances] = useState<WarrenInstance[] | null>(null)
  const [listError, setListError] = useState<string | null>(null)
  const [busy, setBusy] = useState<Record<string, WarrenInstanceAction | 'terminal'>>({})
  const [creating, setCreating] = useState(false)
  const [now, setNow] = useState(() => Date.now())

  const configured = !!status?.configured
  const live = enabled && configured

  // Status (is a key stored? does the daemon answer?) follows the connection settings; the Test button refreshes it on demand.
  const loadStatus = useCallback(() => api.warren.status().then(setStatus).catch((e) => setStatus({ configured: false, reachable: false, error: errorMessage(e) })), [])
  useEffect(() => { void loadStatus() }, [loadStatus, warren?.apiUrl, warren?.connectVia, warren?.apiKeyRef])

  const refresh = useCallback(async () => {
    if (!live) return
    try { setInstances(sortInstances(await api.warren.listInstances())); setListError(null) }
    catch (e) { setListError(errorMessage(e)) }
  }, [live])
  // Poll while the page is on screen (the tab unmounts it otherwise) and refetch on the backend's change event.
  useEffect(() => {
    if (!live) { setInstances(null); return }
    void refresh()
    const t = window.setInterval(() => { if (document.visibilityState === 'visible') void refresh() }, POLL_MS)
    const off = on(WARREN_CHANGED_EVENT, () => void refresh())
    return () => { window.clearInterval(t); off() }
  }, [live, refresh])
  useEffect(() => { const t = window.setInterval(() => setNow(Date.now()), 30_000); return () => window.clearInterval(t) }, [])

  const testConnection = async () => {
    setTesting(true); setTested(true)
    try { setStatus(await api.warren.status()) }
    catch (e) { setStatus({ configured, reachable: false, error: errorMessage(e) }) }
    finally { setTesting(false) }
  }
  const saveKey = async () => {
    const key = keyDraft.trim()
    if (!key) return
    setSavingKey(true)
    try { setStatus(await api.warren.setKey(key)); setKeyDraft(''); setTested(true); notify('success', 'Warren API key saved') }
    catch (e) { notify('error', 'Could not save the API key', errorMessage(e)) }
    finally { setSavingKey(false) }
  }
  const act = async (inst: WarrenInstance, action: WarrenInstanceAction) => {
    if (action === 'delete') {
      const ok = await confirmDialog({ title: `Delete ${inst.name || inst.id}?`, message: 'The instance and everything on its disk are destroyed. Threads bound to it lose their sandbox.', confirmLabel: 'Delete', danger: true })
      if (!ok) return
    }
    setBusy((b) => ({ ...b, [inst.id]: action }))
    try {
      const r = await api.warren.instanceAction(inst.id, action)
      setInstances((list) => list ? (action === 'delete' ? list.filter((i) => i.id !== inst.id) : sortInstances(list.map((i) => (i.id === inst.id ? { ...i, status: r.status } : i)))) : list)
      void refresh()
    } catch (e) { notify('error', `Could not ${action} ${inst.name || inst.id}`, errorMessage(e)) }
    finally { setBusy((b) => { const n = { ...b }; delete n[inst.id]; return n }) }
  }
  const openTerminal = async (inst: WarrenInstance) => {
    setBusy((b) => ({ ...b, [inst.id]: 'terminal' }))
    try { const { url } = await api.warren.terminalUrl(inst.id); await runCommand('editor.openBrowser', { url }) }
    catch (e) { notify('error', 'Could not open the terminal', errorMessage(e)) }
    finally { setBusy((b) => { const n = { ...b }; delete n[inst.id]; return n }) }
  }
  const createInstance = async () => {
    const name = await promptDialog({ title: 'New Warren instance', message: `Template ${warren?.template ?? 'warren-base'}, ${shapeLabel(warren?.shape)}. Change the defaults below.`, placeholder: 'Name (optional)', confirmLabel: 'Create' })
    if (name === null) return
    setCreating(true)
    try { const inst = await api.warren.createInstance(name.trim() ? { name: name.trim() } : {}); setInstances((list) => sortInstances([inst, ...(list ?? []).filter((i) => i.id !== inst.id)])); notify('success', 'Instance created', `${inst.name || inst.id} is provisioning`) }
    catch (e) { notify('error', 'Could not create the instance', errorMessage(e)) }
    finally { setCreating(false) }
  }

  const fields = useMemo(() => fieldsForPage('sandboxes'), [])
  const connectionFields = fields.filter((f) => f.group === 'Connection')
  const defaultFields = fields.filter((f) => f.group !== 'Connection' && f.id !== 'warren.syncExcludes')

  const stateBadge = !enabled ? <Badge>Off</Badge> : !status ? null : !configured ? <Badge tone="warning">No key</Badge> : status.reachable ? <Badge tone="success">Connected</Badge> : <Badge tone="danger">Unreachable</Badge>

  return (
    <SchemaPage page="sandboxes" fields={[]}
      before={
        <>
          <Section title="Connection" description="Warren gives every thread a persistent Linux computer. Files stay on this Mac and go through the usual review; the commands the agent runs happen in the thread's instance on a synced copy of the workspace." actions={stateBadge}>
            {connectionFields.map((f) => <FieldControl key={f.id} field={f} />)}
            <Row title="API key" description={configured ? 'A key is stored in secrets.json. Paste a new one to replace it; the stored key is never shown.' : 'Paste the sk_live_… key from POST /v1/signup or the Warren dashboard. Stored in secrets.json, never in settings.'} vertical>
              <form className="flex w-full gap-1.5" onSubmit={(e) => { e.preventDefault(); void saveKey() }}>
                <TextInput type="password" autoComplete="off" mono value={keyDraft} onChange={setKeyDraft} placeholder={configured ? '••••••••••••  (stored)' : 'sk_live_…'} aria-label="Warren API key" />
                <Button type="submit" variant="primary" loading={savingKey} disabled={!keyDraft.trim()}><KeyRound size={12} />{configured ? 'Replace key' : 'Save key'}</Button>
              </form>
            </Row>
            <Row title="Test connection" description="Calls the API with the stored key and shows the daemon version and your workspace." vertical>
              <div className="flex w-full items-center gap-3 flex-wrap">
                <Button onClick={() => void testConnection()} loading={testing} disabled={!configured}><Plug size={12} />Test connection</Button>
                {tested && status && <StatusLine status={status} />}
              </div>
            </Row>
          </Section>
          <Section title="Defaults for new instances" description="Applied when a thread binds to a new instance or you create one here. Existing instances keep their shape.">
            {defaultFields.map((f) => <FieldControl key={f.id} field={f} />)}
            <SyncExcludesRow />
          </Section>
        </>
      }
      after={
        <>
          <Section title="Instances" description={status?.workspace ? `Workspace ${status.workspace.name || status.workspace.id}: wallet ${formatMicros(status.workspace.walletMicros)}, ${status.workspace.awakeCount} of ${status.workspace.awakeLimit} awake.` : 'Every instance in the workspace, including ones bound to threads in other folders.'}
            actions={live ? <><Button size="sm" onClick={() => void refresh()} title="Refresh now"><RefreshCw size={12} />Refresh</Button><Button size="sm" variant="primary" onClick={() => void createInstance()} loading={creating}><Plus size={12} />New instance</Button></> : undefined}>
            {!enabled ? (
              <Empty icon={<CloudOff size={18} />} title="Warren is off" description="Turn on Enable Warren sandboxes above to see and manage instances." />
            ) : !configured ? (
              <Empty icon={<KeyRound size={18} />} title="No API key" description="Save a Warren API key above. The instance list, the Sandbox picker and remote commands need it." />
            ) : listError && !instances ? (
              <Empty icon={<CloudOff size={18} />} title="Cannot reach Warren" description={listError} action={<Button onClick={() => void refresh()}><RefreshCw size={12} />Retry</Button>} />
            ) : !instances ? (
              <div className="px-3 py-3 text-[11px] text-muted">Loading instances…</div>
            ) : instances.length === 0 ? (
              <Empty icon={<Cloud size={18} />} title="No instances yet" description="Pick Warren in a thread's Sandbox picker or create one here." action={<Button variant="primary" onClick={() => void createInstance()} loading={creating}><Plus size={12} />New instance</Button>} />
            ) : (
              <>
                {listError && <div className="px-3 py-1.5 text-[11px] text-warning border-b border-border">Last refresh failed: {listError}</div>}
                <table className="w-full text-xs" data-testid="warren-instances">
                  <thead><tr className="text-[10px] uppercase tracking-wide text-faint text-left"><th className="px-3 py-1.5 font-medium">Name</th><th className="px-3 py-1.5 font-medium">Status</th><th className="px-3 py-1.5 font-medium">Template</th><th className="px-3 py-1.5 font-medium">Shape</th><th className="px-3 py-1.5 font-medium">Last activity</th><th className="px-3 py-1.5 font-medium text-right">Disk</th><th className="px-3 py-1.5" /></tr></thead>
                  <tbody>
                    {instances.map((inst) => {
                      const pending = busy[inst.id]
                      const actions = availableActions(inst.status)
                      return (
                        <tr key={inst.id} className="border-t border-border">
                          <td className="px-3 py-2"><div className="font-medium truncate max-w-[220px]" title={inst.name}>{inst.name || inst.id}</div><div className="mono text-[10.5px] text-faint selectable">{inst.id}</div></td>
                          <td className="px-3 py-2"><Badge tone={BADGE_TONE[statusTone(inst.status)]} className={isTransitional(inst.status) ? 'animate-breathe' : undefined}>{pending && pending !== 'terminal' ? `${pending}…` : inst.status}</Badge>{inst.statusReason && <div className="text-[10px] text-muted mt-0.5">{inst.statusReason}</div>}</td>
                          <td className="px-3 py-2 text-muted mono text-[11px]">{inst.template}{inst.templateRevision ? <span className="text-faint">@{inst.templateRevision}</span> : null}</td>
                          <td className="px-3 py-2 text-muted whitespace-nowrap">{shapeLabel(inst.resources)}</td>
                          <td className="px-3 py-2 text-muted whitespace-nowrap" title={inst.lastActivityAt ? new Date(inst.lastActivityAt * 1000).toLocaleString() : undefined}>{relativeTime(inst.lastActivityAt, now)}</td>
                          <td className="px-3 py-2 text-muted text-right whitespace-nowrap">{inst.diskUsedBytes !== undefined ? formatBytes(inst.diskUsedBytes) : '—'}</td>
                          <td className="px-3 py-2">
                            <div className="flex items-center justify-end gap-0.5">
                              <IconButton title="Open terminal" onClick={() => void openTerminal(inst)} disabled={!!pending || inst.status === 'stopped' || inst.status === 'deleting'}><SquareTerminal size={13} /></IconButton>
                              <Menu trigger={<button type="button" aria-label={`Actions for ${inst.name || inst.id}`} disabled={!!pending} className="inline-flex items-center justify-center h-6 w-6 rounded text-muted hover:text-fg hover:bg-hover disabled:opacity-40"><MoreHorizontal size={14} /></button>}
                                items={[
                                  ...(['start', 'stop', 'sleep', 'restart'] as WarrenInstanceAction[]).map((a) => ({ id: a, label: ACTION_LABEL[a], icon: a === 'start' ? <Play size={12} /> : undefined, disabled: !actions.includes(a), onSelect: () => void act(inst, a) })),
                                  'sep' as const,
                                  { id: 'terminal', label: 'Open terminal', icon: <SquareTerminal size={12} />, disabled: inst.status === 'stopped', onSelect: () => void openTerminal(inst) },
                                  'sep' as const,
                                  { id: 'delete', label: ACTION_LABEL.delete, icon: <Trash2 size={12} />, danger: true, disabled: !actions.includes('delete'), onSelect: () => void act(inst, 'delete') },
                                ]} />
                            </div>
                          </td>
                        </tr>
                      )
                    })}
                  </tbody>
                </table>
              </>
            )}
          </Section>
          {live && instances && instances.length > 0 && <RunCommand instances={instances} />}
        </>
      }
    />
  )
}

/** Result of Test connection: version, dev mode, wallet, awake count — or the error. */
function StatusLine({ status }: { status: WarrenStatus }) {
  if (!status.configured) return <span className="text-[11px] text-warning">No API key stored.</span>
  if (!status.reachable) return <span className="text-[11px] text-danger selectable" data-testid="warren-status-error">{status.error || 'Warren did not answer.'}</span>
  const ws = status.workspace
  const bits = [status.version ? `Warren ${status.version}` : 'Warren', status.devMode ? 'dev mode' : null, ws ? `wallet ${formatMicros(ws.walletMicros)}` : null, ws ? `${ws.awakeCount}/${ws.awakeLimit} awake` : null].filter(Boolean)
  return <span className="text-[11px] text-success selectable" data-testid="warren-status-ok">{bits.join(' · ')}{ws?.name ? <span className="text-muted"> · {ws.name}</span> : null}</span>
}

/** `warren.syncExcludes` edited as one comma-separated line (the schema keeps it a string list for search and reset). */
function SyncExcludesRow() {
  const { value, set, reset, modified } = useField<string[]>('warren.syncExcludes')
  const joined = (value ?? []).join(', ')
  const [text, setText] = useState(joined)
  useEffect(() => { setText(joined) }, [joined])
  const commit = () => { const next = parseCommaList(text); if (next.join('\u0000') !== (value ?? []).join('\u0000')) void set(next); else setText(next.join(', ')) }
  return (
    <Row title="Sync excludes" description="Directory names never pushed or pulled, on top of .gitignore (comma separated). .git is pushed so git works remotely, but never pulled back." modified={modified} onReset={reset} vertical>
      <TextInput mono value={text} onChange={setText} onBlur={commit} onKeyDown={(e) => { if (e.key === 'Enter') commit() }} placeholder="node_modules, target, dist" aria-label="Sync excludes" />
    </Row>
  )
}

/** Run one shell command in an instance and show stdout, stderr and the exit code. */
function RunCommand({ instances }: { instances: WarrenInstance[] }) {
  const [target, setTarget] = useState<string | undefined>(undefined)
  const [command, setCommand] = useState('')
  const [running, setRunning] = useState(false)
  const [result, setResult] = useState<{ command: string; instance: string; res?: WarrenExecResult; error?: string } | null>(null)
  const options = useMemo(() => instances.filter((i) => i.status !== 'deleting' && i.status !== 'deleted').map((i) => ({ value: i.id, label: i.name || i.id, description: `${i.id} · ${i.status}` })), [instances])
  const selected = target && options.some((o) => o.value === target) ? target : options[0]?.value
  const run = async () => {
    const cmd = command.trim()
    if (!cmd || !selected || running) return
    setRunning(true)
    try { setResult({ command: cmd, instance: selected, res: await api.warren.exec(selected, cmd) }) }
    catch (e) { setResult({ command: cmd, instance: selected, error: errorMessage(e) }) }
    finally { setRunning(false) }
  }
  return (
    <Section title="Run a command" description="Executes in the instance's persistent home as the agent user (wakes a sleeping instance; a stopped one must be started first). Output is capped at 1 MiB per stream.">
      <form className="px-3 py-3 flex items-center gap-1.5" onSubmit={(e) => { e.preventDefault(); void run() }}>
        <Select value={selected} onChange={(v) => setTarget(v)} options={options} placeholder="Instance" className="min-w-[200px]" />
        <TextInput mono value={command} onChange={setCommand} placeholder="uname -a && df -h ~" aria-label="Command" />
        <Button type="submit" variant="primary" loading={running} disabled={!command.trim() || !selected}><Play size={12} />Run</Button>
      </form>
      {result && (
        <div className="px-3 pb-3 text-[11px]" data-testid="warren-exec-result">
          <div className="flex items-center gap-2 mb-1.5 text-muted">
            <span className="mono truncate flex-1">$ {result.command}</span>
            {result.res && <span className={`mono px-1 rounded ${result.res.exitCode === 0 ? 'text-success bg-success/10' : 'text-danger bg-danger/10'}`}>exit {result.res.exitCode}</span>}
            {result.res && <span className="text-faint">{result.res.durationMs} ms</span>}
            {result.res?.truncated && <Badge tone="warning">truncated</Badge>}
          </div>
          {result.error && <div className="text-danger whitespace-pre-wrap break-words selectable">{result.error}</div>}
          {result.res && result.res.stdout && <pre className="mono p-2 rounded bg-editor border border-border overflow-auto max-h-72 whitespace-pre-wrap break-words selectable">{result.res.stdout}</pre>}
          {result.res && result.res.stderr && <pre className="mono mt-1.5 p-2 rounded bg-editor border border-danger/30 text-danger overflow-auto max-h-48 whitespace-pre-wrap break-words selectable">{result.res.stderr}</pre>}
          {result.res && !result.res.stdout && !result.res.stderr && <div className="text-faint">(no output)</div>}
        </div>
      )}
    </Section>
  )
}

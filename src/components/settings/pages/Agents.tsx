import { useCallback, useEffect, useMemo, useState } from 'react'
import { Bot, Download, ExternalLink, KeyRound, Plus, RefreshCw, ScrollText, Trash2, Pencil } from 'lucide-react'
import type { AcpAgentConfig, AcpAgentStatus, AcpRegistryEntry } from '@/types'
import { api } from '@/lib/api'
import { emit as busEmit } from '@/lib/bus'
import { fieldsForPage } from '@/lib/settingsSchema'
import { FieldControl } from '../FieldControl'
import { Section, Button, IconButton, Toggle, Badge, Dialog, Field, TextInput, KeyValueEditor, StringList, Empty, confirmDialog, notify, errorMessage, Tooltip, cn } from '../primitives'

type Tone = 'default' | 'accent' | 'success' | 'warning' | 'danger' | 'info'
const STATE_TONE: Record<AcpAgentStatus['state'], Tone> = { idle: 'default', starting: 'info', ready: 'success', error: 'danger', unsupported: 'warning' }
const STATE_LABEL: Record<AcpAgentStatus['state'], string> = { idle: 'Idle', starting: 'Starting…', ready: 'Connected', error: 'Error', unsupported: 'Not installed' }

function emptyConfig(): AcpAgentConfig { return { id: '', name: '', command: '', args: [], env: {}, enabled: true, source: 'user' } }

export function AgentsPage() {
  const [configs, setConfigs] = useState<AcpAgentConfig[]>([])
  const [statuses, setStatuses] = useState<Record<string, AcpAgentStatus>>({})
  const [registry, setRegistry] = useState<AcpRegistryEntry[]>([])
  const [busy, setBusy] = useState<Record<string, string>>({})
  const [loaded, setLoaded] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [dialog, setDialog] = useState<{ open: boolean; config: AcpAgentConfig; isNew: boolean }>({ open: false, config: emptyConfig(), isNew: true })
  const [logsFor, setLogsFor] = useState<{ id: string; name: string; lines: string[] } | null>(null)
  const fields = fieldsForPage('agents')

  const load = useCallback(async () => {
    try {
      const [cfgs, sts, reg] = await Promise.all([api.acp.configs(), api.acp.agents(), api.acp.registry()])
      setConfigs(cfgs); setStatuses(Object.fromEntries(sts.map((s) => [s.id, s]))); setRegistry(reg); setError(null)
    } catch (e) { setError(errorMessage(e)) } finally { setLoaded(true) }
  }, [])
  useEffect(() => { void load() }, [load])

  const withBusy = async (id: string, label: string, fn: () => Promise<void>) => {
    setBusy((b) => ({ ...b, [id]: label }))
    try { await fn() } catch (e) { notify('error', `${label} failed`, errorMessage(e)) } finally { setBusy((b) => { const n = { ...b }; delete n[id]; return n }); busEmit('acp:changed', null) }
  }
  const probe = (id: string) => withBusy(id, 'Connect', async () => { const s = await api.acp.probe(id); setStatuses((m) => ({ ...m, [id]: s })); if (s.state === 'error') notify('warn', `${s.name}: ${s.error ?? 'error'}`); else notify('success', `${s.name} ready${s.version ? ` (v${s.version})` : ''}`) })
  const install = (id: string) => withBusy(id, 'Install', async () => { const e = await api.acp.install(id); notify(e.installed ? 'success' : 'warn', e.installed ? `${e.name} installed` : `${e.name}: install finished but binary not found`); await load() })
  const toggle = (id: string, enabled: boolean) => withBusy(id, enabled ? 'Enable' : 'Disable', async () => { setConfigs(await api.acp.toggle(id, enabled)); await load() })
  const remove = async (c: AcpAgentConfig) => { if (!(await confirmDialog({ title: `Remove ${c.name}?`, message: 'Removes this custom agent from your settings.', confirmLabel: 'Remove', danger: true }))) return; await withBusy(c.id, 'Remove', async () => { setConfigs(await api.acp.remove(c.id)); await load() }) }
  const auth = (id: string, methodId: string) => withBusy(id, 'Sign in', async () => { const s = await api.acp.authenticate(id, methodId); setStatuses((m) => ({ ...m, [id]: s })); notify('success', `${s.name}: signed in`) })
  const showLogs = async (c: AcpAgentConfig) => { try { setLogsFor({ id: c.id, name: c.name, lines: await api.acp.logs(c.id) }) } catch (e) { notify('error', 'Logs unavailable', errorMessage(e)) } }
  const save = async () => {
    const c = dialog.config
    if (!c.name.trim()) return notify('warn', 'Name is required')
    if (!c.command.trim() && c.source !== 'builtin') return notify('warn', 'Command is required')
    await withBusy(c.id || c.name, 'Save', async () => { setConfigs(await api.acp.add({ ...c, env: c.env && Object.keys(c.env).length ? c.env : undefined })); setDialog((d) => ({ ...d, open: false })); await load() })
  }

  const rows = useMemo(() => configs.map((c) => ({ config: c, status: statuses[c.id], reg: registry.find((r) => r.id === c.id) })), [configs, statuses, registry])

  return (
    <div className="space-y-6">
      <Section title="Agent Client Protocol" description="Run external coding agents inside Caret chat. Each agent keeps its own login and billing (Claude Code uses your Claude subscription or API key; Codex uses ChatGPT or OPENAI_API_KEY). Tool calls, approvals, diffs and terminals from these agents flow through Caret's review bar, proof panel and checkpoints.">
        <div className="space-y-3">{fields.map((f) => <FieldControl key={f.id} field={f} />)}</div>
      </Section>

      <Section title="Agents" description="Pick an agent per chat from the composer (next to the mode selector). Agents install into ~/.caret/acp and start on the first message." actions={<div className="flex items-center gap-1.5"><IconButton title="Refresh" onClick={() => void load()}><RefreshCw size={14} /></IconButton><Button size="sm" onClick={() => setDialog({ open: true, config: emptyConfig(), isNew: true })}><Plus size={13} /> Add custom agent</Button></div>}>
        {error && <div className="text-[12px] text-danger mb-2">{error}</div>}
        {loaded && rows.length === 0 && <Empty icon={<Bot size={20} />} title="No agents" description="Add a custom ACP agent to get started." />}
        <div className="divide-y divide-border rounded-md border border-border">
          {rows.map(({ config: c, status: s, reg }) => {
            const b = busy[c.id]
            const installed = s ? s.installed : reg?.installed ?? true
            const state: AcpAgentStatus['state'] = !c.enabled ? 'idle' : s?.state ?? (installed ? 'idle' : 'unsupported')
            return (
              <div key={c.id} className="p-3 space-y-2" data-testid={`acp-agent-${c.id}`}>
                <div className="flex items-start gap-3">
                  <Bot size={16} className={cn('mt-0.5', c.enabled ? 'text-accent' : 'text-faint')} />
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className="text-[13px] font-medium">{c.name}</span>
                      <Badge tone={c.enabled ? STATE_TONE[state] : 'default'}>{c.enabled ? STATE_LABEL[state] : 'Disabled'}</Badge>
                      {s?.version && <Badge tone="default">v{s.version}</Badge>}
                      {s?.protocolVersion != null && <Badge tone="default">ACP v{s.protocolVersion}</Badge>}
                      {s?.authRequired && <Badge tone="warning">Sign-in required</Badge>}
                      <Badge tone="default">{c.source}</Badge>
                    </div>
                    {c.description && <div className="text-[11.5px] text-muted mt-0.5">{c.description}</div>}
                    <div className="text-[11px] text-faint font-mono mt-1 truncate">{c.command} {c.args.join(' ')}</div>
                    {s?.error && <div className="text-[11.5px] text-danger mt-1">{s.error}</div>}
                    {s?.capabilities && <div className="text-[10.5px] text-faint mt-1">{[s.capabilities.loadSession && 'resume sessions', s.capabilities.promptImages && 'images', s.capabilities.mcp && 'MCP passthrough'].filter(Boolean).join(' · ')}</div>}
                    {s?.modes && s.modes.length > 0 && <div className="text-[10.5px] text-faint">Modes: {s.modes.map((m) => (m.id === s.currentMode ? `[${m.name}]` : m.name)).join(', ')}</div>}
                  </div>
                  <div className="flex items-center gap-1.5 shrink-0">
                    {!installed && reg?.install && <Button size="sm" loading={b === 'Install'} disabled={!!b} onClick={() => void install(c.id)}><Download size={13} /> Install</Button>}
                    {installed && c.enabled && <Button size="sm" variant="default" loading={b === 'Connect'} disabled={!!b} onClick={() => void probe(c.id)}>{s?.state === 'ready' ? 'Reconnect' : 'Connect'}</Button>}
                    {installed && c.enabled && s?.authMethods && s.authMethods.length > 0 && s.authMethods.map((m) => <Tooltip key={m.id} label={m.description ?? m.name}><Button size="sm" loading={b === 'Sign in'} disabled={!!b} onClick={() => void auth(c.id, m.id)}><KeyRound size={13} /> {m.name}</Button></Tooltip>)}
                    <IconButton title="Logs" onClick={() => void showLogs(c)}><ScrollText size={14} /></IconButton>
                    <IconButton title="Edit" onClick={() => setDialog({ open: true, config: { ...c, env: c.env ?? {} }, isNew: false })}><Pencil size={14} /></IconButton>
                    {c.website && <IconButton title="Website" onClick={() => void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(c.website!)).catch(() => window.open(c.website, '_blank'))}><ExternalLink size={14} /></IconButton>}
                    {c.source === 'user' && <IconButton title="Remove" onClick={() => void remove(c)}><Trash2 size={14} /></IconButton>}
                    <Toggle checked={c.enabled} disabled={!!b} onChange={(v) => void toggle(c.id, v)} label={c.enabled ? 'Enabled' : 'Disabled'} />
                  </div>
                </div>
                {!installed && c.installHint && <div className="text-[11px] text-muted">Install manually: <code className="font-mono text-[10.5px] bg-hover px-1 rounded">{c.installHint}</code></div>}
              </div>
            )
          })}
        </div>
      </Section>

      <Dialog open={dialog.open} onOpenChange={(o) => setDialog((d) => ({ ...d, open: o }))} title={dialog.isNew ? 'Add custom agent' : `Edit ${dialog.config.name}`} description="Any program that speaks the Agent Client Protocol over stdio (newline-delimited JSON-RPC)." footer={<><Button onClick={() => setDialog((d) => ({ ...d, open: false }))}>Cancel</Button><Button variant="primary" onClick={() => void save()}>Save</Button></>}>
        <div className="space-y-3">
          <Field label="Name"><TextInput value={dialog.config.name} onChange={(v) => setDialog((d) => ({ ...d, config: { ...d.config, name: v } }))} placeholder="My agent" /></Field>
          {dialog.isNew && <Field label="Id" hint="Optional; derived from the name"><TextInput value={dialog.config.id} onChange={(v) => setDialog((d) => ({ ...d, config: { ...d.config, id: v } }))} placeholder="my-agent" mono /></Field>}
          <Field label="Command" hint={dialog.config.source === 'builtin' ? 'Leave empty to use the default binary' : 'Executable on PATH or an absolute path'}><TextInput value={dialog.config.command} onChange={(v) => setDialog((d) => ({ ...d, config: { ...d.config, command: v } }))} placeholder="claude-agent-acp" mono /></Field>
          <Field label="Arguments"><StringList value={dialog.config.args} onChange={(v) => setDialog((d) => ({ ...d, config: { ...d.config, args: v } }))} placeholder="--experimental-acp" /></Field>
          <Field label="Environment" hint="API keys and flags passed to the agent process"><KeyValueEditor value={dialog.config.env ?? {}} onChange={(v) => setDialog((d) => ({ ...d, config: { ...d.config, env: v } }))} keyPlaceholder="ANTHROPIC_API_KEY" valuePlaceholder="sk-…" secret /></Field>
        </div>
      </Dialog>

      <Dialog open={!!logsFor} onOpenChange={(o) => { if (!o) setLogsFor(null) }} title={`${logsFor?.name ?? ''} logs`} width={720} footer={<Button onClick={() => logsFor && void api.acp.logs(logsFor.id).then((lines) => setLogsFor({ ...logsFor, lines }))}><RefreshCw size={13} /> Refresh</Button>}>
        {logsFor && (logsFor.lines.length === 0 ? <Empty title="No log output yet" description="Logs appear after the agent starts (protocol diagnostics and stderr)." /> : <pre className="max-h-[50vh] overflow-auto text-[11px] font-mono whitespace-pre-wrap bg-hover rounded p-2">{logsFor.lines.join('\n')}</pre>)}
      </Dialog>
    </div>
  )
}

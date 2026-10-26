import { useEffect, useMemo, useState } from 'react'
import { ChevronDown, ChevronRight, ExternalLink, Pencil, Play, Plus, RefreshCw, ScrollText, Server, Trash2, Unplug } from 'lucide-react'
import { initMcpStore, useMcp } from '@/store/mcp'
import { useWorkspace } from '@/store/workspace'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import type { McpServerConfig, McpServerStatus, McpToolInfo, McpTransport } from '@/types'
import { fieldsForPage } from '@/lib/settingsSchema'
import { FieldControl } from '../FieldControl'
import { skeletonFromSchema, parseArgs } from '../mcpSchema'
import { Section, Button, IconButton, Toggle, Badge, Dialog, Field, TextInput, TextArea, Select, KeyValueEditor, StringList, Empty, confirmDialog, notify, errorMessage, Tooltip, cn } from '../primitives'

const STATE_TONE: Record<McpServerStatus['state'], 'default' | 'success' | 'danger' | 'info' | 'warning'> = { connected: 'success', connecting: 'info', error: 'danger', disabled: 'default', disconnected: 'warning' }
const SOURCE_LABEL: Record<McpServerConfig['source'], string> = { project: '.caret/mcp.json', user: '~/.caret/mcp.json', 'cursor-project': '.cursor/mcp.json', 'cursor-user': '~/.cursor/mcp.json' }

export function McpPage() {
  const { servers, configs, tools, resources, prompts, loaded, error } = useMcp()
  const load = useMcp((s) => s.load)
  const ws = useWorkspace((s) => s.workspace)
  const [dialog, setDialog] = useState<{ open: boolean; config?: McpServerConfig }>({ open: false })
  const [logsFor, setLogsFor] = useState<McpServerStatus | null>(null)
  const [tester, setTester] = useState<McpToolInfo | null>(null)
  useEffect(() => { initMcpStore() }, [])
  const enabledField = fieldsForPage('mcp')[0]
  const rows = useMemo(() => configs.map((c) => ({ config: c, status: servers.find((s) => s.id === c.id) })), [configs, servers])

  const openJson = async (scope: 'project' | 'user') => {
    const p = scope === 'project' ? (ws ? `${ws.root}/.caret/mcp.json` : null) : null
    try {
      if (scope === 'project') { if (!p) return; if (!(await api.fs.exists(p))) await api.fs.writeFile(p, JSON.stringify({ mcpServers: {} }, null, 2) + '\n'); await runCommand('editor.openFile', { path: p }) }
      else { const info = await api.app.info(); const up = `${info.configDir}/mcp.json`; if (!(await api.fs.exists(up))) await api.fs.writeFile(up, JSON.stringify({ mcpServers: {} }, null, 2) + '\n'); await runCommand('editor.openFile', { path: up }) }
    } catch (e) { notify('error', 'Cannot open mcp.json', errorMessage(e)) }
  }

  return (
    <>
      <Section title="Model Context Protocol" description="MCP servers add tools, resources and prompts to the agent. Config files use the standard mcpServers format and are watched for changes." actions={<><Button onClick={() => openJson('user')}><ExternalLink size={12} />User mcp.json</Button><Button disabled={!ws} onClick={() => openJson('project')}><ExternalLink size={12} />Project mcp.json</Button><Button variant="primary" onClick={() => setDialog({ open: true })}><Plus size={12} />Add server</Button></>}>
        <FieldControl field={enabledField} />
      </Section>
      <Section title={`Servers (${rows.length})`} actions={<IconButton title="Refresh" onClick={() => void load()}><RefreshCw size={12} /></IconButton>}>
        {error && <div className="px-3 py-2 text-[11px] text-danger">{error}</div>}
        {loaded && rows.length === 0 && <Empty icon={<Server size={22} />} title="No MCP servers configured" description={<>Add a server here or create <code className="mono">.caret/mcp.json</code>. Cursor's <code className="mono">.cursor/mcp.json</code> is read too.</>} action={<Button variant="primary" onClick={() => setDialog({ open: true })}><Plus size={12} />Add server</Button>} />}
        {rows.map(({ config, status }) => <ServerRow key={config.id} config={config} status={status} tools={tools.filter((t) => t.serverId === config.id)} resourceCount={resources.filter((r) => r.serverId === config.id).length} promptCount={prompts.filter((p) => p.serverId === config.id).length}
          onEdit={() => setDialog({ open: true, config })} onLogs={() => status && setLogsFor(status)} onTest={setTester} />)}
      </Section>
      <ServerDialog state={dialog} onClose={() => setDialog({ open: false })} />
      {logsFor && <LogsDialog server={logsFor} onClose={() => setLogsFor(null)} />}
      {tester && <ToolTester tool={tester} onClose={() => setTester(null)} />}
    </>
  )
}

function ServerRow({ config, status, tools, resourceCount, promptCount, onEdit, onLogs, onTest }: { config: McpServerConfig; status?: McpServerStatus; tools: McpToolInfo[]; resourceCount: number; promptCount: number; onEdit: () => void; onLogs: () => void; onTest: (t: McpToolInfo) => void }) {
  const [open, setOpen] = useState(false)
  const { toggleServer, toggleTool, restart, remove } = useMcp()
  const state = status?.state ?? (config.enabled ? 'disconnected' : 'disabled')
  const act = async (f: () => Promise<void>, label: string) => { try { await f() } catch (e) { notify('error', `${label} failed`, errorMessage(e)) } }
  return (
    <div>
      <div className="px-3 py-2 flex items-center gap-3">
        <button onClick={() => setOpen(!open)} aria-label={open ? 'Collapse' : 'Expand'} className="text-muted hover:text-fg">{open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}</button>
        <Toggle checked={config.enabled} onChange={(v) => act(() => toggleServer(config.id, v), 'Toggle')} label="Enabled" />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <Tooltip label={status?.error ?? state}><span className={cn('h-2 w-2 rounded-full', state === 'connected' ? 'bg-success' : state === 'connecting' ? 'bg-info pulse' : state === 'error' ? 'bg-danger' : 'bg-faint')} /></Tooltip>
            <span className="text-xs font-semibold">{config.name}</span>
            <Badge>{config.transport}</Badge><Badge tone="default">{SOURCE_LABEL[config.source]}</Badge>
            {status?.state === 'error' && <Badge tone="danger">error</Badge>}
            <span className="text-[10px] text-muted">{status?.toolCount ?? tools.length} tools · {resourceCount} resources · {promptCount} prompts</span>
          </div>
          <div className="text-[11px] text-muted mono truncate-1">{config.transport === 'stdio' ? [config.command, ...(config.args ?? [])].join(' ') : config.url}</div>
          {status?.error && <div className="text-[11px] text-danger truncate-1">{status.error}</div>}
        </div>
        <div className="flex gap-0.5 shrink-0">
          <IconButton title="Restart" onClick={() => act(() => restart(config.id), 'Restart')}><RefreshCw size={12} /></IconButton>
          <IconButton title="Logs" onClick={onLogs} disabled={!status}><ScrollText size={12} /></IconButton>
          <IconButton title="Edit" onClick={onEdit}><Pencil size={12} /></IconButton>
          <IconButton title="Remove" onClick={async () => { if (await confirmDialog({ title: `Remove MCP server “${config.name}”?`, message: `This edits ${SOURCE_LABEL[config.source]}.`, danger: true, confirmLabel: 'Remove' })) await act(() => remove(config.id), 'Remove') }}><Trash2 size={12} /></IconButton>
        </div>
      </div>
      {open && (
        <div className="pl-12 pr-3 pb-3">
          {tools.length === 0 && <div className="text-[11px] text-muted py-1 flex items-center gap-1"><Unplug size={11} />No tools reported{state !== 'connected' ? ' (server not connected)' : ''}.</div>}
          {tools.map((t) => (
            <div key={t.name} className="flex items-center gap-2 py-1 border-t border-border/60 first:border-0">
              <Toggle checked={t.enabled} onChange={(v) => act(() => toggleTool(config.id, t.name, v), 'Toggle tool')} label={t.name} />
              <div className="flex-1 min-w-0"><div className="text-[11px] mono">{t.name}</div>{t.description && <div className="text-[10px] text-muted truncate-1">{t.description}</div>}</div>
              <IconButton title="Run tool" onClick={() => onTest(t)}><Play size={11} /></IconButton>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

function ServerDialog({ state, onClose }: { state: { open: boolean; config?: McpServerConfig }; onClose: () => void }) {
  const editing = !!state.config
  const hasWs = useWorkspace((s) => !!s.workspace)
  const { add, update } = useMcp()
  const blank = (): McpServerConfig => ({ id: '', name: '', transport: 'stdio', command: '', args: [], env: {}, enabled: true, source: hasWs ? 'project' : 'user' })
  const [c, setC] = useState<McpServerConfig>(state.config ?? blank())
  const [scope, setScope] = useState<'project' | 'user'>(hasWs ? 'project' : 'user')
  const [argsText, setArgsText] = useState('')
  const [busy, setBusy] = useState(false)
  useEffect(() => { if (state.open) { const cc = state.config ?? blank(); setC(cc); setArgsText((cc.args ?? []).join(' ')); setScope(cc.source.includes('user') ? 'user' : 'project') } }, [state.open, state.config]) // eslint-disable-line react-hooks/exhaustive-deps
  const valid = c.name.trim() && (c.transport === 'stdio' ? !!c.command?.trim() : /^https?:\/\//.test(c.url ?? ''))
  const save = async () => {
    setBusy(true)
    const args = parseArgs(argsText)
    const cfg: McpServerConfig = { ...c, id: editing ? c.id : c.name.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-'), name: c.name.trim(), args, env: c.env && Object.keys(c.env).length ? c.env : undefined, headers: c.headers && Object.keys(c.headers).length ? c.headers : undefined }
    try { if (editing) await update(cfg); else await add(cfg, scope); onClose(); notify('success', editing ? 'Server updated' : 'Server added', cfg.name) } catch (e) { notify('error', 'Could not save server', errorMessage(e)) } finally { setBusy(false) }
  }
  return (
    <Dialog open={state.open} onOpenChange={(o) => { if (!o) onClose() }} title={editing ? `Edit ${state.config?.name}` : 'Add MCP server'} width={600} footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!valid} loading={busy} onClick={save}>{editing ? 'Save' : 'Add'}</Button></>}>
      <div className="grid grid-cols-2 gap-x-4">
        <Field label="Name"><TextInput value={c.name} onChange={(v) => setC({ ...c, name: v })} placeholder="github" autoFocus /></Field>
        <Field label="Transport"><Select value={c.transport} onChange={(v) => v && setC({ ...c, transport: v as McpTransport })} options={[{ value: 'stdio', label: 'stdio', description: 'Spawn a local command' }, { value: 'http', label: 'Streamable HTTP', description: 'Remote server URL' }, { value: 'sse', label: 'SSE (legacy)', description: 'Remote server URL' }]} className="w-full" /></Field>
      </div>
      {c.transport === 'stdio' ? (
        <>
          <Field label="Command"><TextInput mono value={c.command ?? ''} onChange={(v) => setC({ ...c, command: v })} placeholder="npx" /></Field>
          <Field label="Arguments" hint="Space separated; quote arguments containing spaces."><TextInput mono value={argsText} onChange={setArgsText} placeholder="-y @modelcontextprotocol/server-github" /></Field>
          <Field label="Working directory (optional)"><TextInput mono value={c.cwd ?? ''} onChange={(v) => setC({ ...c, cwd: v || undefined })} placeholder="${workspaceFolder}" /></Field>
          <Field label="Environment" hint="Use ${env:NAME} to forward a variable from your environment."><KeyValueEditor value={c.env ?? {}} onChange={(env) => setC({ ...c, env })} keyPlaceholder="GITHUB_TOKEN" valuePlaceholder="${env:GITHUB_TOKEN}" /></Field>
        </>
      ) : (
        <>
          <Field label="URL"><TextInput mono value={c.url ?? ''} onChange={(v) => setC({ ...c, url: v })} placeholder="https://mcp.example.com/mcp" /></Field>
          <Field label="Headers"><KeyValueEditor value={c.headers ?? {}} onChange={(headers) => setC({ ...c, headers })} keyPlaceholder="Authorization" valuePlaceholder="Bearer ${env:TOKEN}" /></Field>
        </>
      )}
      <Field label="Disabled tools"><StringList value={c.disabledTools ?? []} onChange={(v) => setC({ ...c, disabledTools: v.length ? v : undefined })} placeholder="tool_name" /></Field>
      {!editing && <Field label="Save to"><Select value={scope} onChange={(v) => setScope((v ?? 'user') as 'project' | 'user')} options={[{ value: 'project', label: 'Project — .caret/mcp.json', description: hasWs ? undefined : 'Open a folder first' }, { value: 'user', label: 'User — ~/.caret/mcp.json' }]} className="w-full" /></Field>}
    </Dialog>
  )
}

function LogsDialog({ server, onClose }: { server: McpServerStatus; onClose: () => void }) {
  const [lines, setLines] = useState<string[] | null>(null)
  const load = () => api.mcp.logs(server.id).then(setLines).catch((e) => setLines([`Failed to load logs: ${errorMessage(e)}`]))
  useEffect(() => { void load() }, [server.id]) // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose() }} title={`Logs — ${server.name}`} width={720} footer={<><Button onClick={load}><RefreshCw size={12} />Refresh</Button><Button onClick={onClose}>Close</Button></>}>
      <pre className="mono text-[11px] whitespace-pre-wrap break-words max-h-[50vh] overflow-auto bg-input rounded border border-border p-2 selectable">{lines === null ? 'Loading…' : lines.length ? lines.join('\n') : 'No log output yet.'}</pre>
    </Dialog>
  )
}

function ToolTester({ tool, onClose }: { tool: McpToolInfo; onClose: () => void }) {
  const [args, setArgs] = useState(() => JSON.stringify(skeletonFromSchema(tool.inputSchema), null, 2))
  const [out, setOut] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  let parseErr: string | null = null
  try { JSON.parse(args) } catch (e) { parseErr = (e as Error).message }
  const run = async () => { setBusy(true); try { const r = await api.mcp.callTool(tool.serverId, tool.name, JSON.parse(args)); setOut(r.content.map((c) => c.text ?? (c.data ? `[${c.mimeType ?? 'binary'} ${Math.round(c.data.length * 0.75)} bytes]` : c.uri ?? '')).join('\n') + (r.isError ? '\n(isError)' : '')) } catch (e) { setOut(`Error: ${errorMessage(e)}`) } finally { setBusy(false) } }
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose() }} title={`Run ${tool.qualifiedName}`} description={tool.description} width={680} footer={<><Button onClick={onClose}>Close</Button><Button variant="primary" disabled={!!parseErr} loading={busy} onClick={run}><Play size={12} />Run</Button></>}>
      <Field label="Arguments (JSON)" hint={parseErr ?? undefined}><TextArea mono value={args} onChange={setArgs} rows={8} /></Field>
      {out !== null && <Field label="Result"><pre className="mono text-[11px] whitespace-pre-wrap max-h-[30vh] overflow-auto bg-input rounded border border-border p-2 selectable">{out}</pre></Field>}
    </Dialog>
  )
}

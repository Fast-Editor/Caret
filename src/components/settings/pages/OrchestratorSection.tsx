import { useCallback, useEffect, useState } from 'react'
import { Copy, Eye, EyeOff, RefreshCw, RotateCcw } from 'lucide-react'
import { useSettings } from '@/store/settings'
import { api } from '@/lib/api'
import { fieldsForPage } from '@/lib/settingsSchema'
import { maskToken } from '@/lib/orchestrator'
import type { OrchestratorSnippets, OrchestratorStatus } from '@/types'
import { FieldControl } from '../FieldControl'
import { Badge, Button, IconButton, Section, Toggle, errorMessage, notify } from '../primitives'

const SNIPPETS: { key: keyof OrchestratorSnippets; label: string; hint: string }[] = [
  { key: 'claudeCode', label: 'Claude Code', hint: 'Run once in a terminal, then ask Claude to use the caret tools' },
  { key: 'codexToml', label: 'Codex', hint: 'Append to ~/.codex/config.toml and export the token variable' },
  { key: 'cursorJson', label: 'Cursor / Caret', hint: '.cursor/mcp.json or .caret/mcp.json in the project that should drive this Caret' },
]

/** Settings → MCP → "Caret as an MCP server": enable the loopback orchestrator endpoint, see its URL and token, copy client snippets. */
export function OrchestratorSection() {
  const settings = useSettings((s) => s.settings)
  const update = useSettings((s) => s.update)
  const enabled = settings?.orchestrator?.enabled ?? false
  const port = settings?.orchestrator?.port
  const [status, setStatus] = useState<OrchestratorStatus | null>(null)
  const [snippets, setSnippets] = useState<OrchestratorSnippets | null>(null)
  const [showToken, setShowToken] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)
  const portField = fieldsForPage('mcp').find((f) => f.id === 'orchestrator.port')

  const refresh = useCallback(async () => {
    try { setStatus(await api.orchestrator.status()); setSnippets(await api.orchestrator.snippets()) } catch (e) { notify('error', 'Orchestrator status failed', errorMessage(e)) }
  }, [])
  useEffect(() => { const t = setTimeout(() => void refresh(), 400); return () => clearTimeout(t) }, [refresh, enabled, port])

  const copy = async (label: string, text: string) => {
    try { await navigator.clipboard.writeText(text); notify('success', `${label} copied`) } catch (e) { notify('error', 'Copy failed', errorMessage(e)) }
  }
  const rotate = async () => {
    setBusy('rotate')
    try { setStatus(await api.orchestrator.rotateToken()); setSnippets(await api.orchestrator.snippets()); notify('success', 'Token rotated', 'Update every connected client with the new token.') } catch (e) { notify('error', 'Rotate failed', errorMessage(e)) } finally { setBusy(null) }
  }
  const token = status?.token ?? ''
  const tone = status?.running ? 'success' : enabled ? 'warning' : 'default'
  const state = status?.running ? 'listening' : enabled ? 'starting' : 'off'

  return (
    <Section title="Caret as an MCP server (orchestrator)"
      description="Serve Caret itself over the Model Context Protocol. Any connected agent (Claude Code in a terminal, Codex, Cursor, another Caret) can create threads here, send them work, wait for replies, read transcripts and resolve approvals, with each thread running on whichever provider, model or external agent you have configured. Threads show up live in this window. The endpoint listens on 127.0.0.1 only and every request needs the bearer token below."
      actions={<Toggle checked={enabled} onChange={(v) => void update({ orchestrator: { enabled: v } })} label="Enabled" />}>
      <div className="flex flex-wrap items-center gap-2 px-3 py-2 text-xs border-b border-border">
        <Badge tone={tone}>{state}</Badge>
        <span className="mono text-muted selectable">{status?.url ?? ''}</span>
        {status?.error && <span className="text-danger">{status.error}</span>}
        <span className="flex-1" />
        <IconButton title="Refresh" onClick={() => void refresh()}><RefreshCw size={12} /></IconButton>
      </div>
      <div className="flex flex-wrap items-center gap-2 px-3 py-2 text-xs border-b border-border">
        <span className="text-muted shrink-0">Token</span>
        <code className="mono truncate flex-1 min-w-0 selectable">{maskToken(token, token, showToken)}</code>
        <IconButton title={showToken ? 'Hide token' : 'Show token'} onClick={() => setShowToken(!showToken)}>{showToken ? <EyeOff size={12} /> : <Eye size={12} />}</IconButton>
        <IconButton title="Copy token" disabled={!token} onClick={() => void copy('Token', token)}><Copy size={12} /></IconButton>
        <Button size="sm" loading={busy === 'rotate'} onClick={() => void rotate()}><RotateCcw size={11} /> Rotate</Button>
      </div>
      {status && <div className="px-3 py-1.5 text-[11px] text-faint border-b border-border">Token file <span className="mono">{status.tokenPath}</span> (mode 0600). Discovery file <span className="mono">{status.endpointPath}</span> while listening.</div>}
      {portField && <FieldControl field={portField} />}
      {snippets && SNIPPETS.map(({ key, label, hint }) => (
        <div key={key} className="px-3 py-2 border-t border-border">
          <div className="flex items-center gap-2 mb-1">
            <span className="text-xs font-semibold">{label}</span>
            <span className="text-[10px] text-muted">{hint}</span>
            <span className="flex-1" />
            <Button size="sm" onClick={() => void copy(`${label} snippet`, snippets[key])}><Copy size={11} /> Copy</Button>
          </div>
          <pre className="mono text-[11px] whitespace-pre-wrap break-all bg-input rounded border border-border p-2 selectable">{maskToken(snippets[key], token, showToken)}</pre>
        </div>
      ))}
    </Section>
  )
}

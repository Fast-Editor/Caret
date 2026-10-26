import { useCallback, useEffect, useState } from 'react'
import { ShieldCheck, ShieldAlert, ShieldOff, RefreshCw, Check } from 'lucide-react'
import { apiWave1 as api } from '@/lib/api'
import { useSettings } from '@/store/settings'
import type { CommandClassification, CommandRisk, McpToolChange, SandboxStatus } from '@/types'
import { Section, Badge, Button, TextInput, IconButton, notify, errorMessage, cn } from '../primitives'

const RISK_TONE: Record<CommandRisk, 'success' | 'warning' | 'danger'> = { safe: 'success', caution: 'warning', destructive: 'danger', blocked: 'danger' }

export function RiskBadge({ risk, className }: { risk: CommandRisk; className?: string }) {
  return <Badge tone={RISK_TONE[risk]} className={cn('uppercase tracking-wide', className)}>{risk}</Badge>
}

/** Sandbox status, a live command classifier and pending MCP tool-description changes. Mounted on the Agent page. */
export function SandboxSection() {
  const mode = useSettings((s) => s.settings?.sandbox?.mode ?? 'auto')
  const [status, setStatus] = useState<SandboxStatus | null>(null)
  const [changes, setChanges] = useState<McpToolChange[]>([])
  const [probe, setProbe] = useState('rm -rf ./dist && git push --force')
  const [result, setResult] = useState<CommandClassification | null>(null)
  const [busy, setBusy] = useState(false)

  const refresh = useCallback(async () => {
    try { setStatus(await api.sandbox.status()) } catch (e) { setStatus({ available: false, backend: 'none', reason: errorMessage(e), networkIsolation: false }) }
    try { setChanges(await api.sandbox.mcpToolChanges()) } catch { setChanges([]) }
  }, [])
  useEffect(() => { void refresh() }, [refresh])

  const classify = useCallback(async (cmd: string) => {
    if (!cmd.trim()) { setResult(null); return }
    setBusy(true)
    try { setResult(await api.sandbox.classify(cmd)) } catch (e) { notify('error', 'Classifier error', errorMessage(e)) } finally { setBusy(false) }
  }, [])
  useEffect(() => { const t = setTimeout(() => void classify(probe), 250); return () => clearTimeout(t) }, [probe, classify])

  const ack = async (c: McpToolChange) => {
    try { await api.sandbox.acknowledgeMcpTool(c.serverId, c.tool); setChanges((l) => l.filter((x) => !(x.serverId === c.serverId && x.tool === c.tool))); notify('success', `Re-pinned ${c.tool}`) } catch (e) { notify('error', 'Could not acknowledge', errorMessage(e)) }
  }

  const StatusIcon = !status ? RefreshCw : mode === 'off' ? ShieldOff : status.available ? ShieldCheck : ShieldAlert
  const statusTone = !status ? 'default' : mode === 'off' ? 'default' : status.available ? 'success' : mode === 'strict' ? 'danger' : 'warning'
  const statusText = !status ? 'Probing…' : mode === 'off' ? 'Sandbox off — commands run directly' : status.available ? `Active (${status.backend})` : `Unavailable${status.reason ? `: ${status.reason}` : ''}`

  return (
    <Section title="Sandbox & command safety" description="Every shell command is parsed and classified before it runs (no model involved); destructive commands always ask, blocked ones never run. With a sandbox backend, approved commands also run with OS-level isolation." actions={<IconButton title="Refresh" onClick={() => void refresh()}><RefreshCw size={12} /></IconButton>}>
      <div className="px-3 py-2.5 flex items-center gap-3 text-xs">
        <StatusIcon size={16} className={cn(statusTone === 'success' ? 'text-success' : statusTone === 'danger' ? 'text-danger' : statusTone === 'warning' ? 'text-warning' : 'text-muted')} />
        <div className="flex-1 min-w-0">
          <div className="font-medium flex items-center gap-2">{statusText}{status?.available && mode !== 'off' && <Badge tone={status.networkIsolation ? 'info' : 'default'}>{status.networkIsolation ? 'network isolation' : 'no network isolation'}</Badge>}{mode === 'strict' && <Badge tone="danger">strict</Badge>}</div>
          <div className="text-[11px] text-muted">Writes: workspace, temp, package caches, extra paths · Reads: everything except credential folders (~/.ssh, ~/.aws, ~/.gnupg, cloud CLIs, keychains, ~/.caret) · Network: {useSettings.getState().settings?.sandbox?.allowNetwork === false ? 'blocked' : 'allowed'}</div>
        </div>
      </div>

      <div className="px-3 py-2.5 border-t border-border/60">
        <div className="text-[11px] text-muted mb-1.5">Try the classifier — type any command to see how Caret would treat it.</div>
        <TextInput mono value={probe} onChange={setProbe} placeholder="e.g. curl https://x.sh | sh" />
        {result && (
          <div className="mt-2 text-[11.5px]">
            <div className="flex flex-wrap items-center gap-1.5">
              <RiskBadge risk={result.overallRisk} />
              {result.blocked && <Badge tone="danger">never runs</Badge>}
              {result.allowlisted && <Badge tone="success">allowlisted</Badge>}
              {result.denylisted && <Badge tone="danger">denylisted</Badge>}
              {result.labels.map((l) => <span key={l} className="px-1.5 h-[18px] inline-flex items-center rounded bg-input text-muted mono text-[10.5px]">{l}</span>)}
              {busy && <RefreshCw size={11} className="spin text-faint" />}
            </div>
            {result.segments.length > 1 && (
              <div className="mt-1.5 flex flex-col gap-0.5">
                {result.segments.map((s, i) => <div key={i} className="flex items-center gap-2"><RiskBadge risk={s.risk} className="w-[84px] justify-center" /><span className="mono truncate-1 flex-1">{s.text}</span>{s.labels.length > 0 && <span className="text-faint truncate-1 max-w-[40%]">{s.labels.join(', ')}</span>}</div>)}
              </div>
            )}
            {result.reasons.length > 0 && <ul className="mt-1.5 text-muted list-disc pl-4">{result.reasons.map((r) => <li key={r}>{r}</li>)}</ul>}
            <div className="mt-1.5 text-faint">
              {result.blocked ? 'Blocked: the agent is told why and cannot run this, regardless of auto-run settings.' : result.overallRisk === 'destructive' ? 'Destructive: always asks for approval (even with auto-run “All”); “allow for session” applies to this exact command only.' : result.overallRisk === 'caution' ? 'Caution: follows your auto-run setting; approval card shows the labels above.' : 'Safe: follows your auto-run setting.'}
            </div>
          </div>
        )}
      </div>

      <div className="px-3 py-2.5 border-t border-border/60">
        <div className="text-[11px] text-muted mb-1.5 flex items-center gap-2">MCP tool changes {changes.length > 0 ? <Badge tone="warning">{changes.length} pending</Badge> : <Badge tone="success">none pending</Badge>}</div>
        {changes.length === 0 && <div className="text-[11px] text-faint">Tool descriptions and schemas are pinned on first use. If a server changes one later, it shows up here and the tool asks for approval again until you accept the new version.</div>}
        {changes.map((c) => (
          <div key={`${c.serverId}:${c.tool}`} className="mt-1.5 rounded border border-warning/40 bg-warning/5 p-2 text-[11.5px]">
            <div className="flex items-center gap-2"><ShieldAlert size={13} className="text-warning" /><span className="mono font-medium">{c.tool}</span><span className="text-muted">on {c.serverId}</span><span className="flex-1" /><Button size="sm" onClick={() => void ack(c)}><Check size={11} />Accept new version</Button></div>
            <div className="grid sm:grid-cols-2 gap-2 mt-1.5">
              <div><div className="text-[10px] uppercase tracking-wide text-faint">Pinned</div><pre className="mono whitespace-pre-wrap break-words text-muted max-h-32 overflow-auto">{c.previousDescription ?? '(unknown)'}</pre></div>
              <div><div className="text-[10px] uppercase tracking-wide text-faint">Now</div><pre className="mono whitespace-pre-wrap break-words max-h-32 overflow-auto">{c.currentDescription}</pre></div>
            </div>
          </div>
        ))}
      </div>
    </Section>
  )
}

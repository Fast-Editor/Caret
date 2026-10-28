import { useEffect, useState } from 'react'
import { Download, RefreshCw, RotateCcw, ScrollText, ShieldAlert, Square } from 'lucide-react'
import { useSettings } from '@/store/settings'
import { autoSummary, initRouterStore, useRouter } from '@/store/router'
import { api } from '@/lib/api'
import { fieldsForPage } from '@/lib/settingsSchema'
import { ROUTER_TIERS, type RouterStatus, type RouterTier } from '@/types'
import { FieldControl, ModelSelect } from '../FieldControl'
import { Badge, Button, Dialog, NumberInput, Row, Section, Toggle, errorMessage, notify } from '../primitives'

const TIER_LABEL: Record<RouterTier, string> = { simple: 'Simple', medium: 'Medium', complex: 'Complex', reasoning: 'Reasoning' }
const TIER_HINT: Record<RouterTier, string> = { simple: 'Short questions, small edits', medium: 'Everyday coding tasks', complex: 'Multi-file changes, tool-heavy work', reasoning: 'Hard design and debugging' }

function stateTone(state: RouterStatus['state']): 'default' | 'accent' | 'success' | 'warning' | 'danger' | 'info' {
  switch (state) { case 'healthy': return 'success'; case 'starting': case 'installing': return 'info'; case 'degraded': return 'warning'; case 'error': return 'danger'; default: return 'default' }
}

/** Settings → Models → Auto routing: enable the Lynkr sidecar, see its state and the tier → model assignment, pin tiers. */
export function AutoRoutingSection() {
  const settings = useSettings((s) => s.settings)
  const update = useSettings((s) => s.update)
  const status = useRouter((s) => s.status)
  useEffect(() => { initRouterStore() }, [])
  const [busy, setBusy] = useState<string | null>(null)
  const [logs, setLogs] = useState<string[] | null>(null)
  const enabled = settings?.router.enabled ?? false
  const models = settings?.models ?? []
  const nameOf = (id: string) => models.find((m) => m.id === id)?.displayName ?? id
  const run = async (what: string, fn: () => Promise<RouterStatus>) => {
    setBusy(what)
    try { useRouter.setState({ status: await fn(), loaded: true }) } catch (e) { notify('error', `Auto routing: ${what} failed`, errorMessage(e)) } finally { setBusy(null) }
  }
  const assignment = status?.assignment
  const warnings = status?.warnings ?? []
  const lan = warnings.find((w) => /LAN|interfaces|unauthenticated|bind/i.test(w))
  const fields = fieldsForPage('models').filter((f) => f.id.startsWith('router.'))
  const summary = autoSummary(status)

  return (
    <Section title="Auto routing" description="With Auto selected in the composer, Lynkr scores every request and picks one of your models per tier. Requests still run through Caret's own providers and keys; nothing leaves your machine except the model call itself."
      actions={<Toggle checked={enabled} onChange={(v) => void update({ router: { enabled: v } })} label="Enabled" />}>
      <div className="flex flex-wrap items-center gap-2 px-3 py-2 text-xs border-b border-border">
        <Badge tone={stateTone(status?.state ?? 'disabled')}>{status?.state ?? 'disabled'}</Badge>
        <span className="text-muted">{status?.reason ?? summary.text}</span>
        {status?.version && <span className="text-faint">Lynkr {status.version}</span>}
        {status?.port && <span className="text-faint">port {status.port}{status.bridgePort ? ` · bridge ${status.bridgePort}` : ''}</span>}
        <span className="flex-1" />
        {status && !status.installed && <Button size="sm" variant="primary" loading={busy === 'install'} onClick={() => void run('install', () => api.router.install())}><Download size={11} /> Install</Button>}
        {status?.installed && <Button size="sm" loading={busy === 'restart'} onClick={() => void run('restart', () => api.router.restart())}><RefreshCw size={11} /> Restart</Button>}
        {status && ['healthy', 'starting', 'degraded'].includes(status.state) && <Button size="sm" loading={busy === 'stop'} onClick={() => void run('stop', () => api.router.stop())}><Square size={11} /> Stop</Button>}
        <Button size="sm" onClick={() => void api.router.logs().then(setLogs).catch((e) => notify('error', 'Could not read router logs', errorMessage(e)))}><ScrollText size={11} /> Logs</Button>
      </div>
      {lan && <div className="flex items-start gap-2 px-3 py-2 text-xs text-warning border-b border-border"><ShieldAlert size={13} className="mt-0.5 shrink-0" /><span>{lan}</span></div>}
      {warnings.filter((w) => w !== lan).length > 0 && (
        <ul className="px-3 py-2 text-xs text-muted border-b border-border space-y-0.5">{warnings.filter((w) => w !== lan).map((w) => <li key={w}>• {w}</li>)}</ul>
      )}
      <div className="px-3 py-2 border-b border-border">
        <div className="text-[10.5px] uppercase tracking-wide text-faint mb-1">Tier → model {assignment && assignment.candidates > 0 && <span className="normal-case tracking-normal">· {assignment.candidates} candidate model{assignment.candidates === 1 ? '' : 's'}</span>}</div>
        <table className="w-full text-xs">
          <tbody>
            {ROUTER_TIERS.map((tier) => {
              const slot = assignment?.slots[tier]
              const override = slot?.source === 'override' ? slot.modelId : undefined
              return (
                <tr key={tier} className="border-t border-border/60">
                  <td className="py-1.5 pr-3 align-top w-[150px]"><div className="font-medium">{TIER_LABEL[tier]}</div><div className="text-[10.5px] text-faint">{TIER_HINT[tier]}</div></td>
                  <td className="py-1.5 pr-3 align-top">
                    {slot ? <div className="flex items-center gap-1.5"><span>{nameOf(slot.modelId)}</span><Badge tone={slot.source === 'override' ? 'accent' : 'default'}>{slot.source}</Badge></div> : <span className="text-faint">—</span>}
                    {slot?.reason && <div className="text-[10.5px] text-faint">{slot.reason}</div>}
                  </td>
                  <td className="py-1.5 align-top text-right">
                    <div className="inline-flex items-center gap-1">
                      <ModelSelect value={override} onChange={(v) => void run('override', () => api.router.setOverride(tier, v ?? null))} allowEmpty="Auto" filter="tools" />
                      {override && <Button size="sm" title="Back to automatic" onClick={() => void run('override', () => api.router.setOverride(tier, null))}><RotateCcw size={11} /></Button>}
                    </div>
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
        {assignment && assignment.slots && Object.keys(assignment.slots).length === 0 && <div className="text-[11px] text-muted mt-1">Enable at least two models with usable providers to get an assignment.</div>}
      </div>
      {fields.map((f) => <FieldControl key={f.id} field={f} />)}
      <Row title="Tokens-per-minute budget" description="Optional Lynkr budget across all tiers. 0 removes the limit.">
        <NumberInput value={settings?.router.tpmLimit ?? 0} onChange={(v) => void update({ router: { tpmLimit: v > 0 ? v : (null as unknown as undefined) } })} min={0} step={1000} />
      </Row>
      <Dialog open={logs !== null} onOpenChange={(o) => { if (!o) setLogs(null) }} title="Auto router logs" width={760} footer={<Button onClick={() => setLogs(null)}>Close</Button>}>
        <pre className="max-h-[60vh] overflow-auto text-[11px] leading-[1.4] whitespace-pre-wrap font-mono select-text">{logs && logs.length ? logs.join('\n') : 'No log lines yet.'}</pre>
      </Dialog>
    </Section>
  )
}

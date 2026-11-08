/**
 * Settings → Usage: tokens, cost and subscription windows for Caret's own ledger (`~/.caret/usage`) and the local
 * histories of external agents (Claude Code, Codex, OpenCode). Charts are plain CSS; data comes from `api.usage`.
 */
import { useCallback, useEffect, useMemo, useState } from 'react'
import { ChartColumn, Download, FolderOpen, Plus, RefreshCw, Trash2, History } from 'lucide-react'
import { api } from '@/lib/api'
import { fieldsForPage } from '@/lib/settingsSchema'
import { useSettings } from '@/store/settings'
import { fmtTokens } from '@/components/chat/util'
import type { ModelPrice, SettingsPatch, UsageDailyPoint, UsageExternalAgent, UsageExternalReport, UsageGroupBy, UsageRange, UsageRow, UsageSummary, UsageTotals, UsageWindow } from '@/types'
import { FieldControl } from '../FieldControl'
import { Section, Button, Badge, Select, TextInput, IconButton, Empty, Spinner, notify, errorMessage, cn } from '../primitives'
import { RANGES, GROUPS, fmtUsd, fmtPct, dayLabel, fmtResetIn, windowTone, barHeights, bucketDaily, csvFilename, priceRowsFromMap, priceMapFromRows, pricePatch, emptyPriceRow, type PriceRow } from './usageFormat'

/** Stable empty map so effects keyed on `customPrices` do not re-run every render before settings load. */
const NO_PRICES: Record<string, ModelPrice> = {}

export function UsagePage() {
  const settings = useSettings((s) => s.settings)
  const track = settings?.usage?.track ?? true
  const includeExternal = settings?.usage?.includeExternal ?? true
  const customPrices = settings?.usage?.customPrices ?? NO_PRICES
  const [range, setRange] = useState<UsageRange>('30d')
  const [groupBy, setGroupBy] = useState<UsageGroupBy>('model')
  const [summary, setSummary] = useState<UsageSummary | null>(null)
  const [daily, setDaily] = useState<UsageDailyPoint[]>([])
  const [external, setExternal] = useState<UsageExternalReport | null>(null)
  const [loading, setLoading] = useState(false)
  const [extLoading, setExtLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [tick, setTick] = useState(0)

  const load = useCallback(async () => {
    setLoading(true); setError(null)
    try {
      const [s, d] = await Promise.all([api.usage.summary(range, groupBy), api.usage.daily(range)])
      setSummary(s); setDaily(d)
    } catch (e) { setError(errorMessage(e)) } finally { setLoading(false) }
  }, [range, groupBy])
  useEffect(() => { void load() }, [load, tick, customPrices])

  useEffect(() => {
    if (!includeExternal) { setExternal(null); return }
    let cancelled = false
    setExtLoading(true)
    api.usage.external(range).then((r) => { if (!cancelled) setExternal(r) }).catch((e) => { if (!cancelled) notify('warn', 'External agent histories unavailable', errorMessage(e)) }).finally(() => { if (!cancelled) setExtLoading(false) })
    return () => { cancelled = true }
  }, [range, includeExternal, customPrices, tick])

  const exportCsv = async () => {
    try {
      const csv = await api.usage.exportCsv(range)
      try {
        const { save } = await import('@tauri-apps/plugin-dialog')
        const path = await save({ defaultPath: csvFilename(range, new Date()), filters: [{ name: 'CSV', extensions: ['csv'] }] })
        if (!path) return
        await api.fs.writeFile(path, csv)
        notify('success', 'Usage exported', path)
      } catch {
        await navigator.clipboard.writeText(csv)
        notify('info', 'CSV copied to the clipboard', 'The save dialog is unavailable in this environment.')
      }
    } catch (e) { notify('error', 'Export failed', errorMessage(e)) }
  }
  const importPast = async () => {
    try {
      const r = await api.usage.backfill()
      notify(r.imported ? 'success' : 'info', r.imported ? `Imported ${r.imported} request${r.imported === 1 ? '' : 's'} from ${r.sessions} saved session${r.sessions === 1 ? '' : 's'}` : 'Nothing new to import', r.skipped ? `${r.skipped} already in the ledger` : undefined)
      setTick((t) => t + 1)
    } catch (e) { notify('error', 'Import failed', errorMessage(e)) }
  }
  const openFolder = async () => { try { await api.app.showInFolder(await api.usage.dir()) } catch (e) { notify('error', 'Cannot open the usage folder', errorMessage(e)) } }

  const totals = summary?.totals
  return (
    <>
      <div className="flex items-center justify-between gap-3 mb-4 flex-wrap">
        <div className="flex items-center gap-1 rounded-md border border-border bg-input p-0.5" role="tablist" aria-label="Range">
          {RANGES.map((r) => <button key={r.id} role="tab" aria-selected={range === r.id} onClick={() => setRange(r.id)} className={cn('h-6 px-2.5 rounded text-xs transition-colors', range === r.id ? 'bg-accent text-accent-fg font-medium' : 'text-muted hover:text-fg hover:bg-hover')}>{r.label}</button>)}
        </div>
        <div className="flex items-center gap-1.5">
          <Button onClick={() => setTick((t) => t + 1)} disabled={loading || extLoading}><RefreshCw size={12} className={loading || extLoading ? 'spin' : ''} />Refresh</Button>
          <Button onClick={() => void importPast()} title="Add the usage saved on older chat sessions to the ledger (safe to repeat)"><History size={12} />Import past sessions</Button>
          <Button onClick={() => void exportCsv()}><Download size={12} />Export CSV</Button>
          <Button onClick={() => void openFolder()}><FolderOpen size={12} />Open usage folder</Button>
        </div>
      </div>

      {!track && <div className="mb-4 rounded-md border border-warning/40 bg-warning/10 p-3 text-[11px]">Usage recording is off: new requests are not added to the ledger. Existing entries are still shown.</div>}
      {error && <div className="mb-4 rounded-md border border-danger/40 bg-danger/10 p-3 text-[11px] text-danger">{error}</div>}

      <div className="grid grid-cols-2 md:grid-cols-5 gap-2 mb-4">
        <StatCard label="Tokens in" value={fmtTokens(totals ? totals.input + totals.cacheRead + totals.cacheWrite : 0)} hint={totals ? `${fmtTokens(totals.input)} fresh · ${fmtTokens(totals.cacheRead)} cached` : undefined} />
        <StatCard label="Tokens out" value={fmtTokens(totals?.output ?? 0)} hint={totals?.thinking ? `${fmtTokens(totals.thinking)} thinking` : undefined} />
        <StatCard label="Cache hit rate" value={fmtPct(totals?.cacheHitRate ?? 0)} hint="cache reads over all prompt tokens" />
        <StatCard label="Cost" value={costLabel(totals)} hint={totals?.unpriced ? `${totals.unpriced} request${totals.unpriced === 1 ? '' : 's'} without a price` : totals && totals.requests > 0 && (totals.costUsd ?? 0) === 0 ? 'your models are priced at $0; set prices in Models or below' : 'at list prices'} accent />
        <StatCard label="Requests" value={String(totals?.requests ?? 0)} hint={summary ? (summary.from === 0 ? 'all time' : `${dayLabel(dayKey(summary.from))} to ${dayLabel(dayKey(summary.to))}`) : undefined} />
      </div>

      <Section title="Cost per day" description="Caret's own requests: agent chat, external agents run inside Caret, inline edits, Tab, review and commit messages.">
        <div className="p-3"><CostChart points={daily} /></div>
      </Section>

      <Section title="Breakdown" actions={<Select<UsageGroupBy> value={groupBy} onChange={(v) => v && setGroupBy(v)} options={GROUPS.map((g) => ({ value: g.id, label: `By ${g.label.toLowerCase()}` }))} className="w-[150px]" />}>
        {summary && summary.rows.length > 0 ? <BreakdownTable rows={summary.rows} firstColumn={GROUPS.find((g) => g.id === groupBy)?.label ?? 'Group'} /> : <Empty icon={<ChartColumn size={22} />} title={loading ? 'Loading usage' : 'No usage recorded in this range'} description={loading ? undefined : 'Entries appear here after the first priced request. Switch to a wider range to see older activity.'} />}
      </Section>

      <Section title="External agents" description="Read-only view of the histories that Claude Code, Codex and OpenCode keep on this machine. Costs are estimates at list prices unless the agent recorded them.">
        {!includeExternal ? <div className="p-3 text-[11px] text-muted">Turn on <b>Include external agents</b> below to scan local histories.</div>
          : !external ? <div className="p-3 flex items-center gap-2 text-[11px] text-muted"><Spinner />Scanning local histories</div>
          : external.agents.map((a) => <ExternalAgentCard key={a.id} agent={a} />)}
      </Section>

      <PricesEditor prices={customPrices} />

      <Section title="Tracking">
        {fieldsForPage('usage').map((f) => <FieldControl key={f.id} field={f} />)}
      </Section>
    </>
  )
}

function dayKey(ts: number): string {
  const d = new Date(ts)
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}
function costLabel(t: UsageTotals | undefined): string {
  if (!t) return '$0.00'
  if (t.costUsd === null) return t.requests > 0 ? 'set a price' : '$0.00'
  return fmtUsd(t.costUsd)
}

function StatCard({ label, value, hint, accent }: { label: string; value: string; hint?: string; accent?: boolean }) {
  return (
    <div className="rounded-md border border-border bg-panel/60 px-3 py-2.5 min-w-0">
      <div className="text-[10.5px] uppercase tracking-wide text-faint">{label}</div>
      <div className={cn('text-lg font-semibold mono mt-0.5 truncate', accent && 'text-accent')} title={value}>{value}</div>
      {hint && <div className="text-[10.5px] text-muted mt-0.5 truncate" title={hint}>{hint}</div>}
    </div>
  )
}

function CostChart({ points }: { points: UsageDailyPoint[] }) {
  const shown = useMemo(() => bucketDaily(points, 120), [points])
  const heights = useMemo(() => barHeights(shown), [shown])
  const total = shown.reduce((a, p) => a + p.costUsd, 0)
  const tokenTotal = shown.reduce((a, p) => a + p.tokens, 0)
  // No priced cost but tokens were used (models priced at $0): draw tokens per day instead of an empty box.
  const byTokens = total <= 0 && tokenTotal > 0
  const series = useMemo(() => (byTokens ? shown.map((p) => ({ ...p, costUsd: p.tokens })) : shown), [byTokens, shown])
  const seriesHeights = useMemo(() => (byTokens ? barHeights(series) : heights), [byTokens, series, heights])
  if (shown.length === 0 || (total <= 0 && tokenTotal <= 0)) return <div className="h-[120px] flex items-center justify-center text-[11px] text-muted">No requests recorded in this range</div>
  const bucketed = shown.length !== points.length
  return (
    <div>
      {byTokens && <div className="text-[10.5px] text-muted mb-1">Tokens per {bucketed ? 'period' : 'day'} ({fmtTokens(tokenTotal)} total); no priced cost in this range.</div>}
      <div className="flex items-end gap-[2px] h-[120px] overflow-hidden" role="img" aria-label={byTokens ? `Tokens per ${bucketed ? 'period' : 'day'}: ${fmtTokens(tokenTotal)} total` : `Cost per ${bucketed ? 'period' : 'day'}: ${fmtUsd(total)} total`}>
        {series.map((p, i) => (
          <div key={p.day} className="flex-1 min-w-[2px] h-full flex items-end" title={`${dayLabel(p.day)}${bucketed ? ' onwards' : ''}: ${byTokens ? '' : `${fmtUsd(shown[i].costUsd)} · `}${fmtTokens(p.tokens)} tokens`}>
            <div className={cn('w-full rounded-t-sm transition-[height]', p.costUsd > 0 ? 'bg-accent/80 hover:bg-accent' : 'bg-border/40')} style={{ height: `${p.costUsd > 0 ? Math.max(seriesHeights[i], 2) : 1}%` }} />
          </div>
        ))}
      </div>
      <div className="flex justify-between text-[10px] text-faint mt-1">
        <span>{dayLabel(shown[0].day)}</span>
        <span>{fmtUsd(total)} total{bucketed ? ` · ${shown.length} periods` : ''}</span>
        <span>{dayLabel(shown[shown.length - 1].day)}</span>
      </div>
    </div>
  )
}

const TH = 'py-1.5 px-3 text-[10.5px] font-medium uppercase tracking-wide text-faint text-right first:text-left whitespace-nowrap'
const TD = 'py-1.5 px-3 text-right mono tabular-nums whitespace-nowrap first:text-left first:font-[inherit]'

function BreakdownTable({ rows, firstColumn }: { rows: UsageRow[]; firstColumn: string }) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-xs">
        <thead><tr className="border-b border-border">
          <th className={TH}>{firstColumn}</th><th className={TH}>Requests</th><th className={TH}>Input</th><th className={TH}>Output</th><th className={TH}>Cache read</th><th className={TH}>Cache write</th><th className={TH}>Cache hit</th><th className={TH}>Cost</th>
        </tr></thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.key} className="border-b border-border/60 last:border-0 hover:bg-hover/50">
              <td className={cn(TD, 'max-w-[260px] truncate')} title={r.key}>{r.label}</td>
              <td className={TD}>{r.requests}</td>
              <td className={TD}>{fmtTokens(r.input)}</td>
              <td className={TD}>{fmtTokens(r.output)}</td>
              <td className={TD}>{fmtTokens(r.cacheRead)}</td>
              <td className={TD}>{fmtTokens(r.cacheWrite)}</td>
              <td className={TD}>{fmtPct(r.cacheHitRate)}</td>
              <td className={TD}>{r.costUsd === null ? <span className="text-warning" title="No price is known for this model. Add one under Model prices.">set a price</span> : <>{fmtUsd(r.costUsd)}{r.unpriced > 0 && <span className="text-warning ml-1" title={`${r.unpriced} request(s) without a price`}>*</span>}</>}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function ExternalAgentCard({ agent }: { agent: UsageExternalAgent }) {
  const t = agent.totals
  return (
    <div className="px-3 py-3">
      <div className="flex items-center gap-2 flex-wrap">
        <span className="text-xs font-medium">{agent.name}</span>
        {agent.available ? <Badge tone="success">found</Badge> : <Badge>not found on this machine</Badge>}
        <code className="mono text-[10px] text-faint truncate max-w-[420px]" title={agent.sourcePathPattern}>{agent.sourcePathPattern}</code>
        {agent.available && <span className="ml-auto text-xs mono tabular-nums">{t.requests} req · {fmtTokens(t.input + t.cacheRead + t.cacheWrite)} in · {fmtTokens(t.output)} out · <span className="text-accent">{costLabel(t)}</span></span>}
      </div>
      {agent.note && <div className="text-[10.5px] text-muted mt-1">{agent.note}</div>}
      {agent.available && agent.windows.length > 0 && (
        <div className="mt-2 grid grid-cols-1 md:grid-cols-2 gap-2">{agent.windows.map((w, i) => <WindowBar key={`${w.label}-${i}`} window={w} />)}</div>
      )}
      {agent.available && agent.byModel.length > 0 && <div className="mt-2 rounded border border-border/60"><BreakdownTable rows={agent.byModel} firstColumn="Model" /></div>}
      {agent.available && agent.byModel.length === 0 && <div className="text-[11px] text-muted mt-1">No requests in this range.</div>}
    </div>
  )
}

function WindowBar({ window: w }: { window: UsageWindow }) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => { const id = window.setInterval(() => setNow(Date.now()), 30_000); return () => window.clearInterval(id) }, [])
  const pct = Math.max(0, Math.min(100, w.usedPercent))
  const tone = windowTone(pct)
  return (
    <div className="rounded-md border border-border/60 px-2.5 py-2">
      <div className="flex items-center justify-between text-[11px]"><span className="font-medium">{w.label}</span><span className="mono tabular-nums">{Math.round(pct)}% used</span></div>
      <div className="h-1.5 mt-1.5 rounded-full bg-border/50 overflow-hidden" role="progressbar" aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100} aria-label={`${w.label} window`}>
        <div className={cn('h-full rounded-full transition-[width]', tone === 'danger' ? 'bg-danger' : tone === 'warning' ? 'bg-warning' : 'bg-success')} style={{ width: `${pct}%` }} />
      </div>
      <div className="text-[10px] text-faint mt-1">{fmtResetIn(w.resetsAt, now) || (w.windowMinutes ? `${w.windowMinutes} minute window` : '')}</div>
    </div>
  )
}

function PricesEditor({ prices }: { prices: Record<string, ModelPrice> }) {
  const update = useSettings((s) => s.update)
  const [rows, setRows] = useState<PriceRow[]>(() => priceRowsFromMap(prices))
  const [dirty, setDirty] = useState(false)
  const [errors, setErrors] = useState<string[]>([])
  const [saving, setSaving] = useState(false)
  useEffect(() => { if (!dirty) setRows(priceRowsFromMap(prices)) }, [prices, dirty])
  const edit = (i: number, patch: Partial<PriceRow>) => { setRows((r) => r.map((row, j) => (j === i ? { ...row, ...patch } : row))); setDirty(true) }
  const remove = (i: number) => { setRows((r) => r.filter((_, j) => j !== i)); setDirty(true) }
  const save = async () => {
    const { prices: next, errors: errs } = priceMapFromRows(rows)
    setErrors(errs)
    if (errs.length) return
    setSaving(true)
    try {
      const patch = pricePatch(prices, next)
      // Removed ids are sent as null so the backend's deep merge deletes them; DeepPartial has no null, hence the cast.
      await update({ usage: { customPrices: patch } } as unknown as SettingsPatch, 'user')
      setDirty(false)
      notify('success', 'Model prices saved')
    } catch (e) { notify('error', 'Failed to save prices', errorMessage(e)) } finally { setSaving(false) }
  }
  const cancel = () => { setRows(priceRowsFromMap(prices)); setErrors([]); setDirty(false) }
  return (
    <Section title="Model prices" description="USD per million tokens for model ids the catalog does not know (external agents, custom ids). A model id matches exactly or as a prefix (claude-sonnet-4-5 also covers claude-sonnet-4-5-20250929). Leave cache prices empty for the defaults: read 10% and write 125% of input."
      actions={<>
        {dirty && <Button variant="ghost" size="sm" onClick={cancel} disabled={saving}>Cancel</Button>}
        <Button variant={dirty ? 'primary' : 'default'} size="sm" onClick={() => void save()} disabled={!dirty} loading={saving}>Save</Button>
        <Button size="sm" onClick={() => { setRows((r) => [...r, emptyPriceRow()]); setDirty(true) }}><Plus size={12} />Add model</Button>
      </>}>
      {rows.length === 0 ? <div className="p-3 text-[11px] text-muted">No custom prices. Rows marked <span className="text-warning">set a price</span> above use the built-in list for well-known models; add anything else here.</div> : (
        <div className="overflow-x-auto">
          <table className="w-full text-xs">
            <thead><tr className="border-b border-border"><th className={cn(TH, 'text-left')}>Model id or prefix</th><th className={TH}>Input</th><th className={TH}>Output</th><th className={TH}>Cache read</th><th className={TH}>Cache write</th><th className={TH} /></tr></thead>
            <tbody>
              {rows.map((r, i) => (
                <tr key={i} className="border-b border-border/60 last:border-0">
                  <td className="py-1 px-2 min-w-[220px]"><TextInput mono value={r.id} onChange={(v) => edit(i, { id: v })} placeholder="e.g. gpt-5-codex" aria-label="Model id" /></td>
                  {(['input', 'output', 'cacheRead', 'cacheWrite'] as const).map((k) => (
                    <td key={k} className="py-1 px-2 w-[110px]"><TextInput mono inputMode="decimal" value={r[k]} onChange={(v) => edit(i, { [k]: v } as Partial<PriceRow>)} placeholder={k === 'cacheRead' || k === 'cacheWrite' ? 'default' : '0.00'} aria-label={`${k} price per million`} className="text-right" /></td>
                  ))}
                  <td className="py-1 px-2 w-8 text-right"><IconButton title="Remove" onClick={() => remove(i)}><Trash2 size={13} /></IconButton></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {errors.length > 0 && <div className="px-3 py-2 text-[11px] text-danger">{errors.map((e) => <div key={e}>{e}</div>)}</div>}
    </Section>
  )
}

export default UsagePage

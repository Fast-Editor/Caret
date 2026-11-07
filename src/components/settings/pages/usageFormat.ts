/**
 * Pure helpers for Settings → Usage (formatting, chart scaling, price-table editing). No React, no IPC: unit-tested.
 */
import type { ModelPrice, UsageDailyPoint, UsageGroupBy, UsageRange } from '@/types'

export const RANGES: { id: UsageRange; label: string }[] = [
  { id: 'today', label: 'Today' }, { id: '7d', label: '7 days' }, { id: '30d', label: '30 days' }, { id: 'all', label: 'All' },
]
export const GROUPS: { id: UsageGroupBy; label: string }[] = [
  { id: 'model', label: 'Model' }, { id: 'provider', label: 'Provider' }, { id: 'agent', label: 'Agent' }, { id: 'source', label: 'Source' }, { id: 'day', label: 'Day' }, { id: 'workspace', label: 'Workspace' },
]

/** `$0.42`, `$0.0042` below a cent, `$1,234.56` with grouping; null/undefined render as a dash. */
export function fmtUsd(v: number | null | undefined): string {
  if (v === null || v === undefined || Number.isNaN(v)) return '--'
  if (v === 0) return '$0.00'
  if (v > 0 && v < 0.01) return `$${v.toFixed(4)}`
  return `$${v.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`
}

/** Ratio (0..1) as a whole percent; tiny non-zero values show `<1%`. */
export function fmtPct(ratio: number): string {
  if (!ratio || ratio <= 0) return '0%'
  const pct = ratio * 100
  if (pct < 1) return '<1%'
  return `${Math.round(pct)}%`
}

/** `2026-03-10` → `Mar 10` (UTC-safe: the key is a calendar day, not an instant). */
export function dayLabel(day: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day)
  if (!m) return day
  const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']
  return `${months[Number(m[2]) - 1] ?? m[2]} ${Number(m[3])}`
}

/** Countdown to a reset timestamp (ms). */
export function fmtResetIn(resetsAt: number | undefined, now: number): string {
  if (!resetsAt) return ''
  const d = resetsAt - now
  if (d <= 0) return 'resets soon'
  const mins = Math.ceil(d / 60_000)
  if (mins < 60) return `resets in ${mins} min`
  const hours = Math.floor(mins / 60)
  if (hours < 24) return `resets in ${hours}h ${String(mins % 60).padStart(2, '0')}m`
  const days = Math.floor(hours / 24)
  return `resets in ${days}d ${hours % 24}h`
}

export function windowTone(usedPercent: number): 'success' | 'warning' | 'danger' {
  if (usedPercent >= 90) return 'danger'
  if (usedPercent >= 70) return 'warning'
  return 'success'
}

/** Bar heights in percent of the tallest bar (0..100); all zero when there is no cost at all. */
export function barHeights(points: UsageDailyPoint[]): number[] {
  const max = points.reduce((m, p) => Math.max(m, p.costUsd), 0)
  if (max <= 0) return points.map(() => 0)
  return points.map((p) => Math.round((p.costUsd / max) * 1000) / 10)
}

/** Merge consecutive days so the chart never draws more than `maxBars` bars; each bucket keeps its first day. */
export function bucketDaily(points: UsageDailyPoint[], maxBars: number): UsageDailyPoint[] {
  if (maxBars < 1 || points.length <= maxBars) return points
  const size = Math.ceil(points.length / maxBars)
  const out: UsageDailyPoint[] = []
  for (let i = 0; i < points.length; i += size) {
    const slice = points.slice(i, i + size)
    out.push({ day: slice[0].day, costUsd: slice.reduce((a, p) => a + p.costUsd, 0), tokens: slice.reduce((a, p) => a + p.tokens, 0) })
  }
  return out
}

export function csvFilename(range: UsageRange, now: Date): string {
  const y = now.getFullYear(); const m = String(now.getMonth() + 1).padStart(2, '0'); const d = String(now.getDate()).padStart(2, '0')
  return `caret-usage-${range}-${y}-${m}-${d}.csv`
}

// ───────────────────────── custom price table editing ─────────────────────────

export interface PriceRow { id: string; input: string; output: string; cacheRead: string; cacheWrite: string }

const num = (v: number | undefined): string => (v === undefined || v === null ? '' : String(v))

export function priceRowsFromMap(prices: Record<string, ModelPrice>): PriceRow[] {
  return Object.entries(prices).sort(([a], [b]) => a.localeCompare(b)).map(([id, p]) => ({ id, input: num(p.input), output: num(p.output), cacheRead: num(p.cacheRead), cacheWrite: num(p.cacheWrite) }))
}

export function emptyPriceRow(): PriceRow { return { id: '', input: '', output: '', cacheRead: '', cacheWrite: '' } }

function parsePrice(raw: string, what: string, id: string, errors: string[], required: boolean): number | undefined {
  const t = raw.trim()
  if (t === '') { if (required) errors.push(`${id}: ${what} price is required`); return undefined }
  const v = Number(t)
  if (!Number.isFinite(v) || v < 0) { errors.push(`${id}: ${what} price must be a number >= 0`); return undefined }
  return v
}

/** Validate editor rows into a price map. Blank rows are skipped; duplicate ids and bad numbers are reported. */
export function priceMapFromRows(rows: PriceRow[]): { prices: Record<string, ModelPrice>; errors: string[] } {
  const prices: Record<string, ModelPrice> = {}
  const errors: string[] = []
  for (const r of rows) {
    const id = r.id.trim()
    const blank = !id && !r.input.trim() && !r.output.trim() && !r.cacheRead.trim() && !r.cacheWrite.trim()
    if (blank) continue
    if (!id) { errors.push('A row is missing its model id'); continue }
    if (id in prices) { errors.push(`${id}: listed twice`); continue }
    const input = parsePrice(r.input, 'input', id, errors, true)
    const output = parsePrice(r.output, 'output', id, errors, true)
    const cacheRead = parsePrice(r.cacheRead, 'cache read', id, errors, false)
    const cacheWrite = parsePrice(r.cacheWrite, 'cache write', id, errors, false)
    if (input === undefined || output === undefined) continue
    const p: ModelPrice = { input, output }
    if (cacheRead !== undefined) p.cacheRead = cacheRead
    if (cacheWrite !== undefined) p.cacheWrite = cacheWrite
    prices[id] = p
  }
  return { prices, errors }
}

/** Patch for `settings.usage.customPrices`: removed ids become `null` so the backend's deep merge deletes them. */
export function pricePatch(prev: Record<string, ModelPrice>, next: Record<string, ModelPrice>): Record<string, ModelPrice | null> {
  const patch: Record<string, ModelPrice | null> = {}
  for (const id of Object.keys(prev)) if (!(id in next)) patch[id] = null
  for (const [id, p] of Object.entries(next)) {
    const before = prev[id]
    if (!before || before.input !== p.input || before.output !== p.output || before.cacheRead !== p.cacheRead || before.cacheWrite !== p.cacheWrite) patch[id] = p
  }
  return patch
}

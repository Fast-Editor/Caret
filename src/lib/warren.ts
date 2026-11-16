/**
 * Pure helpers for the Warren sandbox UI (Settings → Sandboxes, the composer's Sandbox picker, tool cards).
 * No Tauri or store access here so everything is unit-testable; see docs/WARREN.md for the contract.
 */
import type { SandboxBinding, WarrenInstance, WarrenInstanceAction, WarrenInstanceStatus } from '@/types'

/** Backend event: the instance list changed (create, lifecycle action, delete). */
export const WARREN_CHANGED_EVENT = 'caret://warren/changed'

export type StatusTone = 'success' | 'warning' | 'danger' | 'muted'

/** Micro-dollars → `$5.00` (thousands grouped, always two decimals, sign in front of the `$`). */
export function formatMicros(micros: number | undefined | null): string {
  const dollars = (micros ?? 0) / 1e6
  const abs = Math.abs(dollars).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
  return `${dollars < 0 ? '-' : ''}$${abs}`
}

/** `{ cpu: 2, memory: 4, disk: 4 }` → `2 vCPU · 4 GB · 4 GB`. */
export function shapeLabel(shape: { cpu: number; memory: number; disk: number } | undefined | null): string {
  if (!shape) return '—'
  return `${shape.cpu} vCPU · ${shape.memory} GB · ${shape.disk} GB`
}

const TRANSITIONAL = new Set<string>(['provisioning', 'starting', 'restarting', 'updating', 'stopping', 'waking', 'deleting'])

/** Is the instance between two steady states (spinner territory)? */
export function isTransitional(status: WarrenInstanceStatus | undefined): boolean { return !!status && TRANSITIONAL.has(status) }

/** Badge tone for an instance status: running is good, transitions warn, failed is bad, asleep/stopped/unknown are quiet. */
export function statusTone(status: WarrenInstanceStatus | undefined): StatusTone {
  if (status === 'running') return 'success'
  if (status === 'failed') return 'danger'
  if (isTransitional(status)) return 'warning'
  return 'muted'
}

function statusRank(status: WarrenInstanceStatus | undefined): number {
  switch (status) {
    case 'running': return 0
    case 'waking': case 'starting': return 1
    case 'sleeping': return 2
    case 'stopped': return 3
    default: return 4
  }
}

/** Running first, then waking/starting, sleeping, stopped, everything else; newest first inside a group. Does not mutate. */
export function sortInstances(list: readonly WarrenInstance[]): WarrenInstance[] {
  return [...list].sort((a, b) => statusRank(a.status) - statusRank(b.status) || (b.createdAt ?? 0) - (a.createdAt ?? 0) || a.id.localeCompare(b.id))
}

/** Composer trigger text: `Local` or `Warren · <instance id>`. */
export function pickerLabel(binding: SandboxBinding | null | undefined): string {
  return binding ? `Warren · ${binding.instanceId}` : 'Local'
}

/**
 * Human relative time for a unix timestamp. Accepts seconds (Warren) or milliseconds (Caret), `now` is epoch ms.
 * Missing/zero → `never`; the future (signed URL expiry) reads `in 5m`.
 */
export function relativeTime(ts: number | undefined | null, now: number = Date.now()): string {
  if (!ts) return 'never'
  const ms = ts > 1e12 ? ts : ts * 1000
  const diff = now - ms
  const future = diff < 0
  const s = Math.round(Math.abs(diff) / 1000)
  const wrap = (v: string) => (future ? `in ${v}` : `${v} ago`)
  if (s < 45) return future ? 'in a moment' : 'just now'
  const m = Math.round(s / 60); if (m < 60) return wrap(`${m}m`)
  const h = Math.round(m / 60); if (h < 24) return wrap(`${h}h`)
  const d = Math.round(h / 24); if (d < 30) return wrap(`${d}d`)
  const mo = Math.round(d / 30); if (mo < 12) return wrap(`${mo}mo`)
  return wrap(`${Math.round(mo / 12)}y`)
}

/** Which lifecycle actions make sense for a status (the row menu greys out the rest). Delete is always offered. */
export function availableActions(status: WarrenInstanceStatus | undefined): WarrenInstanceAction[] {
  const out: WarrenInstanceAction[] = []
  if (status === 'stopped' || status === 'sleeping' || status === 'failed') out.push('start')
  if (status === 'running') out.push('stop', 'sleep', 'restart')
  if (status === 'sleeping') out.push('stop')
  if (status !== 'deleting' && status !== 'deleted') out.push('delete')
  return out
}

/** `node_modules, target , dist` → `['node_modules', 'target', 'dist']` (trimmed, empty and duplicate entries dropped). */
export function parseCommaList(text: string): string[] {
  const out: string[] = []
  for (const raw of text.split(',')) { const v = raw.trim(); if (v && !out.includes(v)) out.push(v) }
  return out
}

/**
 * Tool-card badge text for `ToolCallRecord.executor`. The Rust side writes `warren:<id>` (also tolerated: `warren <id>`,
 * `warren/<id>`); the local executor (`local`, empty) gets no badge. Rendered as `[warren ab12cd34ef]`.
 */
export function executorLabel(executor: string | undefined | null): string | null {
  if (!executor) return null
  const m = /^([a-z][a-z0-9-]*)(?:[\s:/]+(.+))?$/i.exec(executor.trim())
  if (!m) return null
  const kind = m[1].toLowerCase()
  if (kind === 'local' && !m[2]) return null
  return m[2] ? `${kind} ${m[2].trim()}` : kind
}

/** One-line binding status for the picker: `ready · pushed 2m ago · pulled never`. */
export function bindingStatusLine(binding: SandboxBinding, now: number = Date.now()): string {
  const head = binding.status === 'error' ? `error${binding.error ? `: ${binding.error}` : ''}` : binding.status
  return `${head} · pushed ${relativeTime(binding.lastPushAt, now)} · pulled ${relativeTime(binding.lastPullAt, now)}`
}

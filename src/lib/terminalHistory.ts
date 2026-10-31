/**
 * Pure helpers for the persisted terminal command history palette (no DOM, no Tauri; unit-tested).
 * The store (`~/.caret/terminal/history.jsonl`) and the search live in `src-tauri/src/core/terminal_history.rs`.
 */
import type { HistoryEntry } from '@/types'

export function words(query: string): string[] { return query.trim().split(/\s+/).filter(Boolean) }

/** ≥ 3 words reads like a description ("the docker command that failed") → use the model-assisted search. */
export function isNaturalLanguage(query: string): boolean { return words(query).length >= 3 }

/** Lowercase, deduped search tokens (mirrors the backend tokenizer closely enough for highlighting). */
export function queryTokens(query: string): string[] {
  const out: string[] = []
  for (const raw of query.toLowerCase().split(/[\s,]+/)) {
    const t = raw.replace(/^["'`]+|["'`]+$/g, '').replace(/[?.!:;]+$/, '')
    if (t && !out.includes(t)) out.push(t)
  }
  return out
}

/** Character indices in `text` covered by any token (case-insensitive, all occurrences) — feeds `<Highlight>`. */
export function matchIndices(text: string, tokens: string[]): number[] {
  const lower = text.toLowerCase()
  const set = new Set<number>()
  for (const t of tokens) {
    if (!t) continue
    let from = 0
    for (;;) {
      const i = lower.indexOf(t, from)
      if (i < 0) break
      for (let k = i; k < i + t.length; k++) set.add(k)
      from = i + t.length
    }
  }
  return [...set].sort((a, b) => a - b)
}

/** "just now", "5m ago", "3h ago", "2d ago", "4mo ago", "1y ago" (epoch-ms input). */
export function timeAgo(tsMs: number, nowMs = Date.now()): string {
  const s = Math.max(0, (nowMs - tsMs) / 1000)
  if (s < 60) return 'just now'
  const m = s / 60; if (m < 60) return `${Math.floor(m)}m ago`
  const h = m / 60; if (h < 24) return `${Math.floor(h)}h ago`
  const d = h / 24; if (d < 30) return `${Math.floor(d)}d ago`
  const mo = d / 30; if (mo < 12) return `${Math.floor(mo)}mo ago`
  return `${Math.floor(mo / 12)}y ago`
}

const HOME_RE = /^(\/Users|\/home)\/[^/]+/

/** cwd relative to the workspace root ('.' at the root); otherwise `~`-abbreviated (explicit `home`, else a `/Users|/home` heuristic). */
export function relativeCwd(cwd: string, root?: string | null, home?: string | null): string {
  if (!cwd) return ''
  const norm = cwd.replace(/\/+$/, '') || '/'
  if (root) {
    const r = root.replace(/\/+$/, '')
    if (norm === r) return '.'
    if (norm.startsWith(`${r}/`)) return norm.slice(r.length + 1)
  }
  const h = (home ?? HOME_RE.exec(norm)?.[0] ?? '').replace(/\/+$/, '')
  if (h) {
    if (norm === h) return '~'
    if (norm.startsWith(`${h}/`)) return `~${norm.slice(h.length)}`
  }
  return norm
}

/** Badge text for a failed command, or null when it succeeded / the code is unknown. */
export function exitBadge(entry: Pick<HistoryEntry, 'exitCode'>): string | null {
  return typeof entry.exitCode === 'number' && entry.exitCode !== 0 ? `exit ${entry.exitCode}` : null
}

// List presence: keep removed items rendered (flagged `leaving`) for one slow-duration so they can animate out.
import { useEffect, useRef, useState } from 'react'
import { duration } from '@/lib/motion'

export interface PresenceEntry<T> { key: string; item: T; leaving: boolean }
export interface Leaving<T> { item: T; index: number }

/** Merge the live list with items that are still animating out, re-inserting each at its last known index. */
export function mergePresence<T>(items: T[], leaving: ReadonlyMap<string, Leaving<T>>, keyOf: (t: T) => string): PresenceEntry<T>[] {
  const out: PresenceEntry<T>[] = items.map((item) => ({ key: keyOf(item), item, leaving: false }))
  if (!leaving.size) return out
  const live = new Set(out.map((e) => e.key))
  const gone = [...leaving.entries()].filter(([k]) => !live.has(k)).sort((a, b) => a[1].index - b[1].index)
  for (const [key, { item, index }] of gone) out.splice(Math.min(index, out.length), 0, { key, item, leaving: true })
  return out
}

/** Items removed between two renders, with the index they occupied. */
export function removedBetween<T>(prev: T[], next: T[], keyOf: (t: T) => string): Map<string, Leaving<T>> {
  const nextKeys = new Set(next.map(keyOf))
  const out = new Map<string, Leaving<T>>()
  prev.forEach((item, index) => { const k = keyOf(item); if (!nextKeys.has(k)) out.set(k, { item, index }) })
  return out
}

/**
 * Render `items` plus the ones that just left (for `duration('slow')`), so the caller can apply an exit class.
 * Removals are detected during render, so a removed row never disappears for a frame before its exit starts.
 * With motion off the removed items drop out immediately.
 */
export function usePresenceList<T>(items: T[], keyOf: (t: T) => string): PresenceEntry<T>[] {
  const [leaving, setLeaving] = useState<Map<string, Leaving<T>>>(() => new Map())
  const prev = useRef(items)
  const alive = useRef(true)
  useEffect(() => { alive.current = true; return () => { alive.current = false } }, []) // StrictMode re-runs effects: re-arm on every mount
  const ms = duration('slow')
  const removedNow = prev.current !== items && ms > 0 ? removedBetween(prev.current, items, keyOf) : null
  const merged = removedNow && removedNow.size ? new Map([...leaving, ...removedNow]) : leaving
  useEffect(() => {
    prev.current = items
    if (!removedNow || !removedNow.size) return
    setLeaving((m) => { const n = new Map(m); removedNow.forEach((v, k) => n.set(k, v)); return n })
    window.setTimeout(() => { if (!alive.current) return; setLeaving((m) => { const n = new Map(m); for (const k of removedNow.keys()) n.delete(k); return n }) }, ms)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items])
  return mergePresence(items, merged, keyOf)
}

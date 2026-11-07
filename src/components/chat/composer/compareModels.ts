/**
 * Pure helpers for the model picker's multi-select ("Compare models") and the fan-out send button.
 * No React or Tauri imports so they can be unit-tested.
 */
export const MAX_COMPARE_MODELS = 4

/**
 * Toggle `id` in the compare selection. The first toggle seeds the set with the current primary model (when it is a
 * concrete model) so a shift-click on a second model compares both; `auto` is never part of a comparison.
 */
export function toggleCompareModel(selected: readonly string[], id: string, primary: string, max = MAX_COMPARE_MODELS): string[] {
  if (!id || id === 'auto') return [...selected]
  const base = selected.length === 0 && primary && primary !== 'auto' && primary !== id ? [primary] : [...selected]
  if (base.includes(id)) return base.filter((m) => m !== id)
  if (base.length >= max) return base
  return [...base, id]
}

/** Fan-out needs at least two distinct concrete models. */
export function isFanOut(selected: readonly string[] | undefined): boolean {
  return !!selected && new Set(selected.filter((m) => m && m !== 'auto')).size >= 2
}

/** Label for the composer's send button. */
export function sendLabel(selected: readonly string[] | undefined): string {
  return isFanOut(selected) ? `Send to ${new Set(selected!.filter((m) => m && m !== 'auto')).size}` : 'Send'
}

/** Keep the comparison in sync with the enabled model list (models removed in Settings drop out). */
export function pruneCompare(selected: readonly string[], available: readonly string[]): string[] {
  const ok = new Set(available)
  return selected.filter((m) => ok.has(m))
}

/** Column order for the Compare view: the active session first, then the group in creation order, capped. */
export function compareColumns<T extends { id: string; createdAt: number }>(group: readonly T[], activeId: string | null, max = MAX_COMPARE_MODELS): T[] {
  const sorted = [...group].sort((a, b) => a.createdAt - b.createdAt)
  const active = sorted.find((s) => s.id === activeId)
  const rest = sorted.filter((s) => s.id !== activeId)
  return (active ? [active, ...rest] : rest).slice(0, Math.max(2, max))
}

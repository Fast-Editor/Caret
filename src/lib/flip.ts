// FLIP helpers (First-Last-Invert-Play) for animating reorders with transforms only.

/** Pixel offsets each key must be translated by so it appears at its previous position (keys missing on either side are skipped). */
export function flipDeltas(prev: Record<string, number>, next: Record<string, number>): Record<string, number> {
  const out: Record<string, number> = {}
  for (const k of Object.keys(next)) {
    if (!(k in prev)) continue
    const d = prev[k] - next[k]
    if (Math.abs(d) >= 0.5) out[k] = d
  }
  return out
}

/** Snapshot `getBoundingClientRect().left` (or top) per `data-flip-id` child of `container`. */
export function snapshotPositions(container: Element | null, axis: 'x' | 'y' = 'x'): Record<string, number> {
  const out: Record<string, number> = {}
  if (!container) return out
  container.querySelectorAll<HTMLElement>('[data-flip-id]').forEach((el) => {
    const r = el.getBoundingClientRect()
    out[el.dataset.flipId!] = axis === 'x' ? r.left : r.top
  })
  return out
}

/** Apply inverted transforms then release them on the next frame so the browser transitions to the natural position. */
export function playFlip(container: Element | null, deltas: Record<string, number>, axis: 'x' | 'y' = 'x', durationMs = 180): void {
  if (!container || durationMs <= 0) return
  const moved: HTMLElement[] = []
  container.querySelectorAll<HTMLElement>('[data-flip-id]').forEach((el) => {
    const d = deltas[el.dataset.flipId!]
    if (!d) return
    el.style.transition = 'none'
    el.style.transform = axis === 'x' ? `translateX(${d}px)` : `translateY(${d}px)`
    moved.push(el)
  })
  if (!moved.length) return
  void container.getBoundingClientRect() // flush styles with the inverted transform applied
  requestAnimationFrame(() => {
    for (const el of moved) {
      el.style.transition = `transform ${durationMs}ms var(--ease)`
      el.style.transform = ''
    }
    window.setTimeout(() => { for (const el of moved) { el.style.transition = ''; el.style.transform = '' } }, durationMs + 20)
  })
}

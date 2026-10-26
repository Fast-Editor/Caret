/** Content-hash anchoring so review comments survive small line shifts. */
export function hashLine(text: string): string {
  // FNV-1a 32-bit over the whitespace-normalised line
  const s = text.replace(/\s+/g, ' ').trim()
  let h = 0x811c9dc5
  for (let i = 0; i < s.length; i++) { h ^= s.charCodeAt(i); h = Math.imul(h, 0x01000193) >>> 0 }
  return h.toString(16).padStart(8, '0')
}

export interface Anchor { line: number; hash: string }

/** Encode an anchor into the comment id suffix-free form used by the UI: `line` is 1-based. */
export function makeAnchor(lines: string[], line: number): Anchor { return { line, hash: hashLine(lines[line - 1] ?? '') } }

/**
 * Re-anchor: if the hashed line is still at `line`, keep it; otherwise search ±window for the nearest line with the same hash.
 * Returns null when the line can no longer be found (comment becomes "orphaned").
 */
export function reanchor(lines: string[], anchor: Anchor, window = 20): number | null {
  const at = lines[anchor.line - 1]
  if (at !== undefined && hashLine(at) === anchor.hash) return anchor.line
  for (let d = 1; d <= window; d++) {
    const up = anchor.line - d, down = anchor.line + d
    if (up >= 1 && lines[up - 1] !== undefined && hashLine(lines[up - 1]) === anchor.hash) return up
    if (down <= lines.length && lines[down - 1] !== undefined && hashLine(lines[down - 1]) === anchor.hash) return down
  }
  return null
}

/** Comments store the hash in `text` metadata-free; we keep a side table keyed by comment id in the UI. */
export function anchorKey(path: string, side: string, line: number): string { return `${side}:${path}:${line}` }

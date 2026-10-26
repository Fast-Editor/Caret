/** Minimal Myers line diff producing hunks (pure, for git gutter decorations and tests). */
export interface Hunk { kind: 'added' | 'modified' | 'deleted'; /** 1-based lines in the new text */ start: number; end: number }

export function lineDiff(oldLines: string[], newLines: string[]): Hunk[] {
  const n = oldLines.length, m = newLines.length
  const max = n + m
  if (max === 0) return []
  if (max > 40000) return coarseDiff(oldLines, newLines)
  const v = new Int32Array(2 * max + 2)
  const trace: Int32Array[] = []
  const off = max + 1
  let found = false
  outer: for (let d = 0; d <= max; d++) {
    trace.push(v.slice())
    for (let k = -d; k <= d; k += 2) {
      let x: number
      if (k === -d || (k !== d && v[off + k - 1] < v[off + k + 1])) x = v[off + k + 1]; else x = v[off + k - 1] + 1
      let y = x - k
      while (x < n && y < m && oldLines[x] === newLines[y]) { x++; y++ }
      v[off + k] = x
      if (x >= n && y >= m) { found = true; break outer }
    }
  }
  if (!found) return coarseDiff(oldLines, newLines)
  // backtrack
  const ops: ('eq' | 'del' | 'ins')[] = []
  let x = n, y = m
  for (let d = trace.length - 1; d >= 0; d--) {
    const vd = trace[d]
    const k = x - y
    let prevK: number
    if (k === -d || (k !== d && vd[off + k - 1] < vd[off + k + 1])) prevK = k + 1; else prevK = k - 1
    const prevX = vd[off + prevK]; const prevY = prevX - prevK
    while (x > prevX && y > prevY) { ops.push('eq'); x--; y-- }
    if (d > 0) { if (x === prevX) { ops.push('ins'); y-- } else { ops.push('del'); x-- } }
  }
  ops.reverse()
  return opsToHunks(ops)
}

function opsToHunks(ops: ('eq' | 'del' | 'ins')[]): Hunk[] {
  const hunks: Hunk[] = []
  let newLine = 0
  let i = 0
  while (i < ops.length) {
    if (ops[i] === 'eq') { newLine++; i++; continue }
    let dels = 0, ins = 0
    while (i < ops.length && ops[i] !== 'eq') { if (ops[i] === 'del') dels++; else ins++; i++ }
    if (ins && dels) hunks.push({ kind: 'modified', start: newLine + 1, end: newLine + ins })
    else if (ins) hunks.push({ kind: 'added', start: newLine + 1, end: newLine + ins })
    else hunks.push({ kind: 'deleted', start: Math.max(1, newLine), end: Math.max(1, newLine) })
    newLine += ins
  }
  return hunks
}

/** Fallback for huge inputs: common prefix/suffix only. */
function coarseDiff(a: string[], b: string[]): Hunk[] {
  let p = 0; while (p < a.length && p < b.length && a[p] === b[p]) p++
  let s = 0; while (s < a.length - p && s < b.length - p && a[a.length - 1 - s] === b[b.length - 1 - s]) s++
  const delCount = a.length - p - s, insCount = b.length - p - s
  if (!delCount && !insCount) return []
  if (insCount && delCount) return [{ kind: 'modified', start: p + 1, end: p + insCount }]
  if (insCount) return [{ kind: 'added', start: p + 1, end: p + insCount }]
  return [{ kind: 'deleted', start: Math.max(1, p), end: Math.max(1, p) }]
}

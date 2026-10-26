/** Pure helpers shared with providers.ts (kept Monaco-free for tests). */
import type { LspPosition } from '@/types'
import type { TextEdit } from './types'
export function applyTextEdits(text: string, edits: TextEdit[]): string {
  const lines = text.split('\n')
  const offsets: number[] = []
  let acc = 0; for (const l of lines) { offsets.push(acc); acc += l.length + 1 }
  const off = (p: LspPosition) => (offsets[Math.min(p.line, lines.length - 1)] ?? acc) + p.character
  const sorted = [...edits].sort((a, b) => off(b.range.start) - off(a.range.start))
  let out = text
  for (const e of sorted) out = out.slice(0, off(e.range.start)) + e.newText + out.slice(off(e.range.end))
  return out
}

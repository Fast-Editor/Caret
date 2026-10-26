/**
 * Pure helpers for semantic terminal blocks (OSC 133) and program status (OSC 7501).
 * The Rust side (`core/pty.rs`) owns the authoritative block list and output capture; the UI mirrors
 * it through `PtyBlockEvent`s and derives chip/gutter state here. Everything in this file is side-effect free.
 */
import type { ContextItem, ProgramStatus, PtyBlockEvent, TerminalBlock } from '@/types'

export interface TerminalStatusInfo { status: ProgramStatus; title?: string; detail?: string; at: number }
export interface BlockState { blocks: TerminalBlock[]; status?: TerminalStatusInfo }

/** Apply one block event to the mirrored block list (bounded by `max`, oldest evicted first). */
export function applyBlockEvent(blocks: TerminalBlock[], ev: PtyBlockEvent, max = 200): TerminalBlock[] {
  switch (ev.type) {
    case 'block-start': {
      const next = blocks.filter((b) => b.id !== ev.block.id).map((b) => (b.running ? { ...b, running: false } : b))
      next.push({ ...ev.block, running: true })
      return next.length > Math.max(1, max) ? next.slice(next.length - Math.max(1, max)) : next
    }
    case 'block-command': return blocks.map((b) => (b.id === ev.blockId ? { ...b, command: ev.command } : b))
    case 'block-end': return blocks.map((b) => (b.id === ev.blockId ? { ...b, running: false, exitCode: ev.exitCode, endedAt: ev.endedAt, endLine: ev.endLine } : b))
    default: return blocks
  }
}

/**
 * Reduce a PTY block/status event into per-terminal state.
 * Explicit program status (OSC 7501) is cleared when a new command starts, and `working`/`waiting`
 * are cleared when the command ends (the program is gone); `done`/`failed` linger until the next command.
 */
export function reduceBlockState(s: BlockState, ev: PtyBlockEvent, max = 200, now = Date.now()): BlockState {
  if (ev.type === 'status') return { blocks: s.blocks, status: ev.status === 'idle' ? undefined : { status: ev.status, title: ev.title, detail: ev.detail, at: now } }
  const blocks = applyBlockEvent(s.blocks, ev, max)
  let status = s.status
  if (ev.type === 'block-start') status = undefined
  if (ev.type === 'block-end' && status && (status.status === 'working' || status.status === 'waiting')) status = undefined
  return { blocks, status }
}

/** Chip status for a terminal: explicit program status wins, else derived from the last block. */
export function effectiveStatus(blocks: TerminalBlock[], explicit: TerminalStatusInfo | undefined, exited = false): ProgramStatus {
  if (exited) return 'idle'
  if (explicit) return explicit.status
  const last = blocks[blocks.length - 1]
  if (!last) return 'idle'
  if (last.running) return 'working'
  if (typeof last.exitCode === 'number' && last.exitCode !== 0) return 'failed'
  return 'idle'
}

export interface TerminalStatusEntry {
  sessionId: string; ptyId: string | null; title: string; status: ProgramStatus
  /** Program that reported the status (OSC 7501 `app`), if any. */
  program?: string; detail?: string; command?: string; exitCode?: number | null
}
export interface TerminalStatusSummary { terminals: TerminalStatusEntry[]; waiting: number; working: number; failed: number; text: string }

export function summarizeStatus(entries: TerminalStatusEntry[]): TerminalStatusSummary {
  const waiting = entries.filter((e) => e.status === 'waiting').length
  const working = entries.filter((e) => e.status === 'working').length
  const failed = entries.filter((e) => e.status === 'failed').length
  const parts: string[] = []
  if (waiting) parts.push(`${waiting} waiting for input`)
  if (working) parts.push(`${working} running`)
  if (failed) parts.push(`${failed} failed`)
  return { terminals: entries, waiting, working, failed, text: parts.join(' · ') }
}

/** Pick the neighbouring block index for ⌘↑/⌘↓ navigation. With nothing selected, both directions land on the newest block. */
export function neighborIndex(indices: number[], current: number | null | undefined, dir: 1 | -1): number | null {
  const sorted = [...indices].sort((a, b) => a - b)
  if (!sorted.length) return null
  if (current == null) return sorted[sorted.length - 1] ?? null
  const pos = sorted.indexOf(current)
  if (pos < 0) return sorted[sorted.length - 1] ?? null
  return sorted[Math.min(sorted.length - 1, Math.max(0, pos + dir))] ?? null
}

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${Math.max(0, Math.round(ms))}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 1 : 0)}s`
  const m = Math.floor(ms / 60_000); const s = Math.round((ms % 60_000) / 1000)
  return `${m}m ${s}s`
}

export function blockTitle(b: TerminalBlock): string {
  const head = b.command ? `$ ${b.command}` : 'command'
  if (b.running) return `${head} — running`
  const dur = b.endedAt ? formatDuration(b.endedAt - b.startedAt) : undefined
  const exit = typeof b.exitCode === 'number' ? `exit ${b.exitCode}` : 'finished'
  return `${head} — ${exit}${dur ? ` in ${dur}` : ''}`
}

/** Plain-text rendering of a block for the chat context / clipboard. */
export function formatBlockText(b: TerminalBlock, output: string, opts: { maxChars?: number } = {}): string {
  const max = opts.maxChars ?? 20_000
  let body = output.replace(/\r\n?/g, '\n').replace(/\n+$/, '')
  if (body.length > max) body = `${body.slice(0, Math.floor(max * 0.66))}\n…[${body.length - max} chars omitted]…\n${body.slice(body.length - Math.floor(max * 0.34))}`
  const lines = [] as string[]
  if (b.cwd) lines.push(`# cwd: ${b.cwd}`)
  lines.push(`$ ${b.command ?? '<unknown command>'}`)
  if (body) lines.push(body)
  if (b.running) lines.push('[still running]')
  else if (typeof b.exitCode === 'number') lines.push(`[exit ${b.exitCode}]`)
  return lines.join('\n')
}

/**
 * Chat context item for a block. Uses kind `note` (inline `data`) because the backend `terminal` kind re-reads the
 * live scrollback instead of honouring `data`; `terminalId` is still attached so the UI can link back.
 */
export function blockContextItem(b: TerminalBlock, output: string, terminalId: string | null | undefined, terminalTitle?: string): ContextItem {
  const cmd = (b.command ?? 'command').trim()
  const short = cmd.length > 48 ? `${cmd.slice(0, 45)}…` : cmd
  return { id: `term-block-${b.id}`, kind: 'note', label: `Terminal${terminalTitle ? ` (${terminalTitle})` : ''}: ${short}`, data: formatBlockText(b, output), terminalId: terminalId ?? undefined }
}

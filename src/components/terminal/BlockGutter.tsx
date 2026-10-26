import { useEffect, useReducer, useRef, useState } from 'react'
import { cn } from '@/components/common'
import { useTerminals } from '@/store/terminal'
import { blockTitle } from '@/lib/terminalBlocks'
import { terminalRegistry, type TerminalHandle } from './registry'

/**
 * Gutter overlay that draws one bar per semantic command block, positioned from xterm markers.
 * Click selects the block's lines; right-click opens the terminal menu with that block as target.
 */
export function BlockGutter({ sessionId, onContextRow }: { sessionId: string; onContextRow: (row: number) => void }) {
  const blocks = useTerminals((s) => s.sessions[sessionId]?.blocks)
  const selectedId = useTerminals((s) => s.sessions[sessionId]?.selectedBlockId)
  const [, force] = useReducer((x: number) => x + 1, 0)
  const [handle, setHandle] = useState<TerminalHandle | undefined>(() => terminalRegistry.get(sessionId))
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const h = terminalRegistry.get(sessionId)
    if (h) { setHandle(h); return }
    const t = setInterval(() => { const x = terminalRegistry.get(sessionId); if (x) { clearInterval(t); setHandle(x) } }, 50)
    return () => clearInterval(t)
  }, [sessionId])

  useEffect(() => {
    if (!handle) return
    let raf = 0
    const tick = () => { if (!raf) raf = requestAnimationFrame(() => { raf = 0; force() }) }
    const d1 = handle.term.onScroll(tick); const d2 = handle.term.onRender(tick); const d3 = handle.term.onResize(tick)
    const un = handle.blocks.subscribe(tick)
    return () => { d1.dispose(); d2.dispose(); d3.dispose(); un(); if (raf) cancelAnimationFrame(raf) }
  }, [handle])

  if (!handle || !blocks?.length) return null
  const term = handle.term
  const screen = term.element?.querySelector('.xterm-screen') as HTMLElement | null
  const me = ref.current
  const sr = screen?.getBoundingClientRect(); const mr = me?.getBoundingClientRect()
  const cell = sr && term.rows > 0 ? sr.height / term.rows : 0
  const vy = term.buffer.active.viewportY; const rows = term.rows
  const offsetTop = sr && mr ? sr.top - mr.top : 0
  const items = cell > 0 ? blocks.flatMap((b) => {
    const r = handle.blocks.rangeOf(b.index)
    if (!r) return []
    const [s, e] = r
    if (e < vy || s > vy + rows - 1) return []
    const top = offsetTop + (Math.max(s, vy) - vy) * cell
    const bottom = offsetTop + (Math.min(e, vy + rows - 1) - vy + 1) * cell
    const failed = typeof b.exitCode === 'number' && b.exitCode !== 0
    const selected = b.id === selectedId
    return [(
      <button key={b.id} type="button" aria-label={blockTitle(b)} title={blockTitle(b)} style={{ top, height: Math.max(3, bottom - top - 1) }}
        onClick={(ev) => { ev.stopPropagation(); handle.blocks.selectBlock(b.index) }} onContextMenu={() => onContextRow(s)}
        className={cn('absolute left-[3px] w-[3px] rounded-full transition-colors hover:w-[5px] hover:left-[2px]',
          b.running ? 'bg-accent animate-pulse' : failed ? 'bg-danger/80' : 'bg-success/70', selected && 'ring-1 ring-accent ring-offset-0 w-[5px] left-[2px]')} />
    )]
  }) : []
  return <div ref={ref} className="absolute inset-y-0 left-0 w-2 pointer-events-none [&>button]:pointer-events-auto" data-testid="terminal-block-gutter">{items}</div>
}

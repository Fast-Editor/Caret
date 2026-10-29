import { useCallback, useRef, useState } from 'react'
import { cn } from './cn'

/**
 * Drag handle for pixel-based split panes. `onResize(delta)` receives the pointer delta since the last move
 * (positive = right/down). Caller clamps and persists.
 */
export function ResizeHandle({ direction, onResize, onDoubleClick, className, invert }: { direction: 'horizontal' | 'vertical'; onResize: (delta: number) => void; onDoubleClick?: () => void; className?: string; invert?: boolean }) {
  const last = useRef(0)
  const [dragging, setDragging] = useState(false)
  const onPointerDown = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault()
    const target = e.currentTarget
    target.setPointerCapture(e.pointerId)
    last.current = direction === 'horizontal' ? e.clientX : e.clientY
    document.body.style.cursor = direction === 'horizontal' ? 'col-resize' : 'row-resize'
    document.body.classList.add('is-resizing')
    setDragging(true)
    // Coalesce pointer moves into one store update per frame so the panes (and Monaco relayout) never fall behind the pointer.
    let pending = 0; let raf: number | null = null
    const flush = () => { raf = null; if (pending !== 0) { const d = pending; pending = 0; onResize(invert ? -d : d) } }
    const move = (ev: PointerEvent) => {
      const cur = direction === 'horizontal' ? ev.clientX : ev.clientY
      pending += cur - last.current
      last.current = cur
      if (raf === null) raf = requestAnimationFrame(flush)
    }
    const up = () => {
      target.removeEventListener('pointermove', move)
      target.removeEventListener('pointerup', up)
      if (raf !== null) { cancelAnimationFrame(raf); flush() }
      document.body.style.cursor = ''
      document.body.classList.remove('is-resizing')
      setDragging(false)
    }
    target.addEventListener('pointermove', move)
    target.addEventListener('pointerup', up)
  }, [direction, onResize, invert])
  return (
    <div role="separator" aria-orientation={direction === 'horizontal' ? 'vertical' : 'horizontal'} onPointerDown={onPointerDown} onDoubleClick={onDoubleClick}
      className={cn('shrink-0 z-20 relative group/handle', direction === 'horizontal' ? 'w-0 cursor-col-resize' : 'h-0 cursor-row-resize', className)}>
      {/* wide invisible hit zone */}
      <div className={cn('absolute', direction === 'horizontal' ? 'inset-y-0 -left-[3px] w-[7px]' : 'inset-x-0 -top-[3px] h-[7px]')} />
      {/* 2px line, invisible until hover or drag */}
      <div data-dragging={dragging} className={cn('resize-line absolute pointer-events-none', direction === 'horizontal' ? 'inset-y-0 -left-px w-[2px]' : 'inset-x-0 -top-px h-[2px]')} />
    </div>
  )
}

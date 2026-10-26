import { useCallback, useRef } from 'react'
import { cn } from './cn'

/**
 * Drag handle for pixel-based split panes. `onResize(delta)` receives the pointer delta since the last move
 * (positive = right/down). Caller clamps and persists.
 */
export function ResizeHandle({ direction, onResize, onDoubleClick, className, invert }: { direction: 'horizontal' | 'vertical'; onResize: (delta: number) => void; onDoubleClick?: () => void; className?: string; invert?: boolean }) {
  const last = useRef(0)
  const onPointerDown = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault()
    const target = e.currentTarget
    target.setPointerCapture(e.pointerId)
    last.current = direction === 'horizontal' ? e.clientX : e.clientY
    document.body.style.cursor = direction === 'horizontal' ? 'col-resize' : 'row-resize'
    document.body.classList.add('is-resizing')
    const move = (ev: PointerEvent) => {
      const cur = direction === 'horizontal' ? ev.clientX : ev.clientY
      const d = cur - last.current
      last.current = cur
      if (d !== 0) onResize(invert ? -d : d)
    }
    const up = () => {
      target.removeEventListener('pointermove', move)
      target.removeEventListener('pointerup', up)
      document.body.style.cursor = ''
      document.body.classList.remove('is-resizing')
    }
    target.addEventListener('pointermove', move)
    target.addEventListener('pointerup', up)
  }, [direction, onResize, invert])
  return (
    <div role="separator" aria-orientation={direction === 'horizontal' ? 'vertical' : 'horizontal'} onPointerDown={onPointerDown} onDoubleClick={onDoubleClick}
      className={cn('shrink-0 z-20 relative group/handle', direction === 'horizontal' ? 'w-px cursor-col-resize' : 'h-px cursor-row-resize', className)}>
      <div className={cn('absolute bg-transparent group-hover/handle:bg-accent/70 transition-colors delay-150', direction === 'horizontal' ? 'inset-y-0 -left-[2px] w-[5px]' : 'inset-x-0 -top-[2px] h-[5px]')} />
    </div>
  )
}

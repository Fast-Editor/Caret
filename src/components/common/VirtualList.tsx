import { useCallback, useEffect, useImperativeHandle, useRef, useState, forwardRef, type ReactNode, type HTMLAttributes } from 'react'
import { cn } from './cn'

export interface VirtualListHandle { scrollToIndex: (index: number, align?: 'auto' | 'center') => void; element: HTMLDivElement | null }
export interface VirtualListProps extends Omit<HTMLAttributes<HTMLDivElement>, 'children'> {
  count: number
  rowHeight: number
  overscan?: number
  renderRow: (index: number, style: React.CSSProperties) => ReactNode
  emptyState?: ReactNode
}

/** Minimal fixed-row-height windowing list. Memory-friendly for large trees/results. */
export const VirtualList = forwardRef<VirtualListHandle, VirtualListProps>(function VirtualList({ count, rowHeight, overscan = 8, renderRow, emptyState, className, ...rest }, ref) {
  const el = useRef<HTMLDivElement>(null)
  const [range, setRange] = useState({ start: 0, end: 40 })
  const compute = useCallback(() => {
    const node = el.current
    if (!node) return
    const start = Math.max(0, Math.floor(node.scrollTop / rowHeight) - overscan)
    const visible = Math.ceil(node.clientHeight / rowHeight) + overscan * 2
    setRange((r) => (r.start === start && r.end === start + visible ? r : { start, end: start + visible }))
  }, [rowHeight, overscan])
  useEffect(() => {
    compute()
    const node = el.current
    if (!node) return
    const ro = new ResizeObserver(compute)
    ro.observe(node)
    return () => ro.disconnect()
  }, [compute, count])
  useImperativeHandle(ref, () => ({
    element: el.current,
    scrollToIndex: (index, align = 'auto') => {
      const node = el.current
      if (!node) return
      const top = index * rowHeight
      if (align === 'center') node.scrollTop = top - node.clientHeight / 2 + rowHeight / 2
      else if (top < node.scrollTop) node.scrollTop = top
      else if (top + rowHeight > node.scrollTop + node.clientHeight) node.scrollTop = top + rowHeight - node.clientHeight
    },
  }), [rowHeight])
  const end = Math.min(count, range.end)
  const rows: ReactNode[] = []
  for (let i = range.start; i < end; i++) rows.push(renderRow(i, { position: 'absolute', top: i * rowHeight, left: 0, right: 0, height: rowHeight }))
  return (
    <div ref={el} onScroll={compute} className={cn('relative overflow-auto h-full w-full', className)} {...rest}>
      {count === 0 ? emptyState : <div style={{ height: count * rowHeight, position: 'relative' }}>{rows}</div>}
    </div>
  )
})

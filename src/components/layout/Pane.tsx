import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { cn } from '@/components/common'
import { usePresence } from '@/lib/motion'

/**
 * A side/bottom pane that glides open and closed. Width/height animate only on toggle (never while dragging a resize
 * handle — `body.is-resizing` disables the transition); the content slides a few pixels and fades. Stays mounted for
 * the exit animation via `usePresence`, then unmounts.
 */
export function Pane({ open, size, axis, edge = 'start', className, children, ...rest }: {
  open: boolean; size?: number; axis: 'x' | 'y'; edge?: 'start' | 'end'; className?: string; children: ReactNode
} & Omit<React.HTMLAttributes<HTMLDivElement>, 'children'>) {
  const [mounted, visible] = usePresence(open)
  // First paint after (re)mount starts collapsed so the opening transition has somewhere to come from.
  const [entered, setEntered] = useState(false)
  const raf = useRef<number | null>(null)
  useLayoutEffect(() => { if (!mounted) setEntered(false) }, [mounted])
  useEffect(() => {
    if (!mounted) return
    raf.current = requestAnimationFrame(() => { raf.current = requestAnimationFrame(() => setEntered(true)) })
    return () => { if (raf.current) cancelAnimationFrame(raf.current) }
  }, [mounted])
  if (!mounted) return null
  const shown = visible && entered
  const style = size === undefined ? undefined : axis === 'x' ? { width: shown ? size : 0 } : { height: shown ? size : 0 }
  return (
    <div {...rest} data-open={shown} data-axis={axis} data-edge={edge} style={style} className={cn('pane shrink-0 min-w-0 min-h-0 overflow-hidden', className)}>
      <div className="pane-inner" style={size === undefined ? undefined : axis === 'x' ? { width: size } : { height: size }}>{children}</div>
    </div>
  )
}

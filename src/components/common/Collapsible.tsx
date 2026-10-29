import { useEffect, useState, type ReactNode } from 'react'
import { usePresence } from '@/lib/motion'
import { cn } from './cn'

/**
 * Animated disclosure body: expands/collapses with the grid-rows trick (height) plus a fade, and keeps children
 * mounted just long enough for the exit to play. Children are not mounted while closed, so sections that fetch on
 * mount (Timeline, History) stay lazy. With motion off the tokens are 0ms and it behaves like `{open && children}`.
 */
export function Collapsible({ open, children, className }: { open: boolean; children: ReactNode; className?: string }) {
  const [mounted] = usePresence(open)
  const [collapsed, setCollapsed] = useState(!open)
  useEffect(() => {
    if (!open) { setCollapsed(true); return }
    // Mount collapsed, then expand on the next frame so the transition has a start state.
    const id = requestAnimationFrame(() => setCollapsed(false))
    return () => cancelAnimationFrame(id)
  }, [open])
  if (!mounted) return null
  return (
    <div className={cn('caret-collapse', className)} data-collapsed={collapsed} aria-hidden={!open}>
      <div>{children}</div>
    </div>
  )
}

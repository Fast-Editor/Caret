import { useLayoutEffect, useRef } from 'react'
import { usePresence } from '@/lib/motion'
import './chat.css'

/**
 * Height-animated disclosure (grid-template-rows 0fr → 1fr, no measuring). Children mount lazily on first open and stay
 * mounted until the close transition ends. A section that is already open on first paint renders settled (no entry
 * animation), so restored transcripts do not unfold row by row.
 */
export function Collapse({ open, className = '', children }: { open: boolean; className?: string; children: React.ReactNode }) {
  const [mounted, visible] = usePresence(open)
  const ref = useRef<HTMLDivElement>(null)
  const first = useRef(true)
  const hadEl = useRef(false)
  useLayoutEffect(() => {
    const el = ref.current
    const appeared = !!el && !hadEl.current
    hadEl.current = !!el
    if (first.current) { first.current = false; return }
    if (appeared && visible && el) {
      // Mounted straight into the open state: start folded, flush, then open so the transition runs.
      el.dataset.open = 'false'; void el.offsetHeight; el.dataset.open = 'true'
    }
  })
  if (!mounted) return null
  return (
    <div ref={ref} className={`chat-collapse ${className}`} data-open={visible} aria-hidden={!visible}>
      <div className="chat-collapse-inner">{children}</div>
    </div>
  )
}

/** Right-pointing chevron that rotates to point down when open. */
export function Chevron({ open, size = 12, className = '' }: { open: boolean; size?: number; className?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" className={`chat-chevron shrink-0 ${className}`} data-open={open} aria-hidden>
      <path d="m9 18 6-6-6-6" />
    </svg>
  )
}

/** Three breathing dots for "thinking / working" states. */
export function Dots({ className = '' }: { className?: string }) {
  return <span className={`chat-dots ${className}`} aria-hidden><i style={{ ['--i' as string]: 0 }} /><i style={{ ['--i' as string]: 1 }} /><i style={{ ['--i' as string]: 2 }} /></span>
}

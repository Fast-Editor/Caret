// Motion helpers. The level lives on <html data-motion="off|subtle|expressive"> (set by applyTheme) and drives the
// CSS duration tokens; components that need JS timing (exit animations, scroll anchoring) read it from here.
import { useEffect, useRef, useState } from 'react'

export type MotionLevel = 'off' | 'subtle' | 'expressive'

export function motionLevel(): MotionLevel {
  if (typeof document === 'undefined') return 'subtle'
  const v = document.documentElement.dataset.motion
  return v === 'off' || v === 'expressive' ? v : 'subtle'
}
export function motionEnabled(): boolean { return motionLevel() !== 'off' }

/** Duration in ms for the named token at the current level (mirrors --dur-* in styles.css). */
export function duration(kind: 'fast' | 'base' | 'slow' = 'base'): number {
  const level = motionLevel()
  if (level === 'off') return 0
  const base = kind === 'fast' ? 120 : kind === 'slow' ? 240 : 180
  return level === 'expressive' ? Math.round(base * 1.35) : base
}

/**
 * Keep an element mounted while it animates out. Returns `[mounted, visible]`: render while `mounted`, apply the
 * open/closed classes from `visible`. `delay` defaults to the slow token so exits can finish.
 */
export function usePresence(open: boolean, delayMs?: number): [boolean, boolean] {
  const [mounted, setMounted] = useState(open)
  const timer = useRef<number | null>(null)
  useEffect(() => {
    if (timer.current) { window.clearTimeout(timer.current); timer.current = null }
    if (open) { setMounted(true); return }
    const ms = delayMs ?? duration('slow')
    if (ms === 0) { setMounted(false); return }
    timer.current = window.setTimeout(() => { setMounted(false); timer.current = null }, ms)
    return () => { if (timer.current) window.clearTimeout(timer.current) }
  }, [open, delayMs])
  return [mounted, open]
}

/** Smooth scroll that degrades to an instant jump when motion is off. */
export function scrollTo(el: Element, top: number): void {
  el.scrollTo({ top, behavior: motionEnabled() ? 'smooth' : 'auto' })
}

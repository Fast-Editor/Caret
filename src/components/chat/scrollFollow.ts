// Pure scroll-follow policy for the transcript (MessageList). Kept free of DOM so it is unit-testable.

/** Distance from the bottom (px) under which the list counts as "pinned" and keeps following new content. */
export const STICK_THRESHOLD = 48

export interface ScrollMetrics { scrollTop: number; scrollHeight: number; clientHeight: number }

export function distanceFromBottom(m: ScrollMetrics): number { return Math.max(0, m.scrollHeight - m.scrollTop - m.clientHeight) }
export function isNearBottom(m: ScrollMetrics, threshold = STICK_THRESHOLD): boolean { return distanceFromBottom(m) < threshold }

/**
 * Why the content changed:
 * - `session`: a different transcript was loaded → jump to the end, instantly.
 * - `append`: a message was added at the end (user sent / assistant started) → glide to the end if pinned.
 * - `delta`: streaming text grew → keep the end in view instantly (smooth scrolling fights 60 ms deltas).
 * - `prepend`: older messages were revealed above (windowing) → keep what the reader sees where it is.
 * - `resize`: the viewport changed → re-pin instantly if pinned.
 */
export type FollowReason = 'session' | 'append' | 'delta' | 'prepend' | 'resize'
export type FollowAction = { kind: 'none' } | { kind: 'bottom'; smooth: boolean } | { kind: 'anchor'; top: number }

export function followAction(reason: FollowReason, stick: boolean, m: ScrollMetrics, prevScrollHeight: number): FollowAction {
  switch (reason) {
    case 'session': return { kind: 'bottom', smooth: false }
    case 'prepend': { const grown = m.scrollHeight - prevScrollHeight; return grown > 0 ? { kind: 'anchor', top: m.scrollTop + grown } : { kind: 'none' } }
    case 'append': return stick ? { kind: 'bottom', smooth: true } : { kind: 'none' }
    case 'delta': case 'resize': return stick ? { kind: 'bottom', smooth: false } : { kind: 'none' }
  }
}

/** Classify a transcript change from the previous/next shape. Append wins over prepend when both happen at once. */
export function classifyChange(prev: { sessionId: string; lastId?: string; count: number; start: number }, next: { sessionId: string; lastId?: string; count: number; start: number }): FollowReason | null {
  if (prev.sessionId !== next.sessionId) return 'session'
  if (next.count > prev.count || (next.lastId !== prev.lastId && next.lastId !== undefined)) return 'append'
  if (next.start < prev.start) return 'prepend'
  return null
}

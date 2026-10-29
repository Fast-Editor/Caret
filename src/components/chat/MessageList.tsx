import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { ArrowDown } from 'lucide-react'
import type { ChatMessage } from '@/types'
import { useChat } from '@/store/chat'
import { scrollTo, usePresence } from '@/lib/motion'
import { UserMessage } from './messages/UserMessage'
import { AssistantMessage } from './messages/AssistantMessage'
import { SummaryPart } from './messages/Parts'
import { classifyChange, followAction, isNearBottom, type FollowReason } from './scrollFollow'
import './chat.css'

const Row = memo(function Row({ message, sessionId, isLast, entering }: { message: ChatMessage; sessionId: string; isLast: boolean; version: number; entering: boolean }) {
  // Captured once per row instance: a message fades up when it arrives and never again (streaming deltas re-render the
  // same instance; restored transcripts mount with `entering=false`).
  const [enter] = useState(entering)
  const cls = enter ? 'animate-fade-up' : undefined
  if (message.role === 'user') return <div className={cls}><UserMessage message={message} sessionId={sessionId} isLast={isLast} /></div>
  if (message.role === 'system') return <div className={`px-3 py-1 ${cls ?? ''}`}>{message.parts.map((p, i) => (p.type === 'summary' ? <SummaryPart key={i} text={p.text} /> : p.type === 'text' ? <div key={i} className="text-[11.5px] text-faint italic">{p.text}</div> : null))}</div>
  return <div className={cls}><AssistantMessage message={message} sessionId={sessionId} isLast={isLast} /></div>
}, (a, b) => a.message.id === b.message.id && a.version === b.version && a.isLast === b.isLast)

/**
 * Renders the transcript with auto-follow (glide on new messages, instant on streaming deltas, anchored when older
 * messages are revealed), a "jump to latest" pill with an unread count, and a windowed tail for very long sessions.
 */
export function MessageList({ sessionId, messages, header }: { sessionId: string; messages: ChatMessage[]; header?: React.ReactNode }) {
  const versions = useChat((s) => s.versions)
  const streamingId = useChat((s) => s.streaming[sessionId])
  const ref = useRef<HTMLDivElement>(null)
  const [stick, setStick] = useState(true)
  const [unread, setUnread] = useState(0)
  const [window_, setWindow] = useState(80)
  const stickRef = useRef(true)
  stickRef.current = stick

  const start = Math.max(0, messages.length - window_)
  const shown = messages.slice(start)
  const lastId = messages[messages.length - 1]?.id

  // Ids seen on the previous render (per session) → anything new at the tail is "entering".
  const known = useRef<{ session: string; ids: Set<string>; ready: boolean }>({ session: sessionId, ids: new Set(), ready: false })
  if (known.current.session !== sessionId) known.current = { session: sessionId, ids: new Set(), ready: false }
  const isEntering = (id: string) => known.current.ready && !known.current.ids.has(id)
  useEffect(() => { known.current.ids = new Set(messages.map((m) => m.id)); known.current.ready = true })

  const onScroll = useCallback(() => {
    const el = ref.current; if (!el) return
    const atBottom = isNearBottom(el)
    setStick(atBottom)
    if (atBottom) setUnread(0)
    if (el.scrollTop < 200 && window_ < messages.length) setWindow((w) => Math.min(messages.length, w + 80))
  }, [messages.length, window_])

  // Follow policy — runs before paint so anchoring/pinning never flashes.
  const prevShape = useRef({ sessionId, lastId, count: messages.length, start })
  const prevHeight = useRef(0)
  const apply = useCallback((reason: FollowReason) => {
    const el = ref.current; if (!el) return
    const act = followAction(reason, stickRef.current, el, prevHeight.current)
    if (act.kind === 'bottom') { if (act.smooth) scrollTo(el, el.scrollHeight); else el.scrollTop = el.scrollHeight }
    else if (act.kind === 'anchor') el.scrollTop = act.top
    prevHeight.current = el.scrollHeight
  }, [])
  useLayoutEffect(() => {
    const next = { sessionId, lastId, count: messages.length, start }
    const reason = classifyChange(prevShape.current, next)
    prevShape.current = next
    if (reason === 'session') { setWindow(80); setStick(true); stickRef.current = true; setUnread(0); apply('session'); return }
    if (reason === 'append' && !stickRef.current) setUnread((n) => n + 1)
    if (reason) apply(reason)
    else { const el = ref.current; if (el) prevHeight.current = el.scrollHeight }
  }, [sessionId, lastId, messages.length, start, apply])
  // Streaming deltas: keep the end in view (instant) while pinned.
  useEffect(() => { if (!stick) return; const id = requestAnimationFrame(() => apply('delta')); return () => cancelAnimationFrame(id) }, [versions, stick, streamingId, apply])
  // Viewport resizes (pane drag, keyboard) re-pin instantly.
  useEffect(() => { const el = ref.current; if (!el || typeof ResizeObserver === 'undefined') return; const ro = new ResizeObserver(() => apply('resize')); ro.observe(el); return () => ro.disconnect() }, [apply])

  const [pillMounted, pillVisible] = usePresence(!stick)
  const jump = () => { const el = ref.current; if (el) scrollTo(el, el.scrollHeight); setStick(true); setUnread(0) }
  return (
    <div className="relative flex-1 min-h-0">
      <div ref={ref} onScroll={onScroll} className="absolute inset-0 overflow-y-auto overscroll-contain">
        <div key={sessionId} className="animate-fade">
          {header}
          {start > 0 && <button type="button" onClick={() => setWindow((w) => w + 120)} className="w-full py-2 text-[11px] text-muted hover:text-fg">Show {Math.min(120, start)} earlier messages ({start} hidden)</button>}
          {shown.map((m, i) => <Row key={m.id} message={m} sessionId={sessionId} isLast={start + i === messages.length - 1} version={versions[m.id] ?? 0} entering={isEntering(m.id)} />)}
          <div className="h-3" />
        </div>
      </div>
      {pillMounted && (
        <div className="absolute bottom-3 left-1/2 -translate-x-1/2">
          <button type="button" onClick={jump} className={`flex items-center gap-1 px-2.5 h-6 rounded-full surface-float text-[11px] hover:bg-hover press animate-fade-up ${pillVisible ? '' : 'is-leaving'}`} data-testid="jump-to-latest">
            <ArrowDown size={12} /> {unread > 0 ? `${unread} new` : 'Latest'}
          </button>
        </div>
      )}
    </div>
  )
}

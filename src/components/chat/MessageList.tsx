import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { ArrowDown } from 'lucide-react'
import type { ChatMessage } from '@/types'
import { useChat } from '@/store/chat'
import { UserMessage } from './messages/UserMessage'
import { AssistantMessage } from './messages/AssistantMessage'
import { SummaryPart } from './messages/Parts'

const Row = memo(function Row({ message, sessionId, isLast }: { message: ChatMessage; sessionId: string; isLast: boolean; version: number }) {
  if (message.role === 'user') return <UserMessage message={message} sessionId={sessionId} isLast={isLast} />
  if (message.role === 'system') return <div className="px-3 py-1">{message.parts.map((p, i) => (p.type === 'summary' ? <SummaryPart key={i} text={p.text} /> : p.type === 'text' ? <div key={i} className="text-[11.5px] text-faint italic">{p.text}</div> : null))}</div>
  return <AssistantMessage message={message} sessionId={sessionId} isLast={isLast} />
}, (a, b) => a.message.id === b.message.id && a.version === b.version && a.isLast === b.isLast)

/** Renders the transcript with auto-scroll, a "jump to latest" pill and a windowed tail for very long sessions. */
export function MessageList({ sessionId, messages, header }: { sessionId: string; messages: ChatMessage[]; header?: React.ReactNode }) {
  const versions = useChat((s) => s.versions)
  const streamingId = useChat((s) => s.streaming[sessionId])
  const ref = useRef<HTMLDivElement>(null)
  const [stick, setStick] = useState(true)
  const [window_, setWindow] = useState(80)
  const prevCount = useRef(messages.length)

  const onScroll = useCallback(() => {
    const el = ref.current; if (!el) return
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 48
    setStick(atBottom)
    if (el.scrollTop < 200 && window_ < messages.length) setWindow((w) => Math.min(messages.length, w + 80))
  }, [messages.length, window_])

  useLayoutEffect(() => {
    const el = ref.current; if (!el) return
    if (stick || messages.length > prevCount.current) el.scrollTop = el.scrollHeight
    prevCount.current = messages.length
  })
  useEffect(() => { setWindow(80); setStick(true); const el = ref.current; if (el) el.scrollTop = el.scrollHeight }, [sessionId])
  // keep pinned to bottom while streaming
  useEffect(() => { if (!stick) return; const el = ref.current; if (!el) return; const id = requestAnimationFrame(() => { el.scrollTop = el.scrollHeight }); return () => cancelAnimationFrame(id) }, [versions, stick, streamingId])

  const start = Math.max(0, messages.length - window_)
  const shown = messages.slice(start)
  return (
    <div className="relative flex-1 min-h-0">
      <div ref={ref} onScroll={onScroll} className="absolute inset-0 overflow-y-auto overscroll-contain">
        {header}
        {start > 0 && <button type="button" onClick={() => setWindow((w) => w + 120)} className="w-full py-2 text-[11px] text-muted hover:text-fg">Show {Math.min(120, start)} earlier messages ({start} hidden)</button>}
        {shown.map((m, i) => <Row key={m.id} message={m} sessionId={sessionId} isLast={start + i === messages.length - 1} version={versions[m.id] ?? 0} />)}
        <div className="h-3" />
      </div>
      {!stick && (
        <button type="button" onClick={() => { const el = ref.current; if (el) el.scrollTop = el.scrollHeight; setStick(true) }} className="absolute bottom-3 left-1/2 -translate-x-1/2 flex items-center gap-1 px-2.5 h-6 rounded-full bg-panel border border-border shadow text-[11px] hover:bg-hover"><ArrowDown size={12} /> Latest</button>
      )}
    </div>
  )
}

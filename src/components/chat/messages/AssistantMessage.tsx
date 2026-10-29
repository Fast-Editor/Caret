import { memo, useEffect, useRef, useState } from 'react'
import { Check, Copy, RotateCcw } from 'lucide-react'
import type { ChatMessage } from '@/types'
import { Markdown } from '@/lib/markdown'
import { ToolCallCard } from './ToolCallCard'
import { ErrorPart, PlanCard, QuestionCard, SummaryPart, ThinkingPart, UsageFooter, partKey } from './Parts'
import { useChat } from '@/store/chat'
import { Dots } from '../Collapse'

/** Throttle a rapidly changing string to ~60ms while `active`. */
function useThrottled(value: string, active: boolean, ms = 60): string {
  const [v, setV] = useState(value)
  const last = useRef(0)
  const timer = useRef<number | null>(null)
  useEffect(() => {
    if (!active) { setV(value); return }
    const now = performance.now()
    const due = ms - (now - last.current)
    if (due <= 0) { last.current = now; setV(value); return }
    if (timer.current) window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => { last.current = performance.now(); setV(value) }, due)
    return () => { if (timer.current) window.clearTimeout(timer.current) }
  }, [value, active, ms])
  return v
}

/** Live text: while streaming, `chat-live` draws a breathing caret at the end of the last block (chat.css). */
const StreamingText = memo(function StreamingText({ text, streaming, sessionId }: { text: string; streaming: boolean; sessionId: string }) {
  const t = useThrottled(text, streaming)
  return <Markdown text={t} streaming={streaming} sessionId={sessionId} className={`text-[13px] ${streaming ? 'chat-live' : ''}`} />
})

export const AssistantMessage = memo(function AssistantMessage({ message, sessionId, isLast }: { message: ChatMessage; sessionId: string; isLast: boolean }) {
  const regenerate = useChat((s) => s.regenerate)
  const streaming = message.status === 'streaming'
  const [copied, setCopied] = useState(false)
  const allText = message.parts.filter((p) => p.type === 'text').map((p) => (p.type === 'text' ? p.text : '')).join('\n\n')
  const copy = async () => { try { const { writeText } = await import('@tauri-apps/plugin-clipboard-manager'); await writeText(allText) } catch { await navigator.clipboard.writeText(allText).catch(() => {}) } setCopied(true); setTimeout(() => setCopied(false), 1000) }
  const lastIdx = message.parts.length - 1
  const lastPart = message.parts[lastIdx]
  // Between parts (e.g. after a tool call finished, before the next text arrives) show the working indicator too.
  const working = streaming && (message.parts.length === 0 || (lastPart.type === 'tool' && (lastPart.call.status === 'done' || lastPart.call.status === 'error')))
  return (
    <div className="group/msg px-3 py-1.5">
      {message.parts.map((p, i) => {
        const key = partKey(p, i)
        const isLastPart = i === lastIdx
        switch (p.type) {
          case 'text': return <StreamingText key={key} text={p.text} streaming={streaming && isLastPart} sessionId={sessionId} />
          case 'thinking': return <ThinkingPart key={key} text={p.text} streaming={streaming && isLastPart} durationMs={p.durationMs} />
          case 'tool': return <ToolCallCard key={key} call={p.call} sessionId={sessionId} />
          case 'question': return <QuestionCard key={key} question={p.question} answer={p.answer} sessionId={sessionId} />
          case 'plan': return <PlanCard key={key} path={p.path} title={p.title} sessionId={sessionId} />
          case 'summary': return <SummaryPart key={key} text={p.text} />
          case 'error': return <ErrorPart key={key} message={p.message} onRetry={isLast ? () => void regenerate(sessionId, message.id) : undefined} />
          case 'image': return <img key={key} src={`data:${p.image.mime};base64,${p.image.dataBase64}`} alt={p.image.name} className="my-1.5 max-w-full rounded border border-border animate-pop" />
          case 'context': case 'checkpoint': return null
          default: return null
        }
      })}
      {working && <div className="flex items-center gap-2 h-5 text-muted text-[12px] animate-fade" data-testid="thinking"><Dots />{message.parts.length === 0 ? 'Thinking…' : 'Working…'}</div>}
      {message.status === 'cancelled' && <div className="text-[11px] text-faint mt-1 animate-fade">Stopped</div>}
      {!streaming && (
        <div className="flex items-center gap-2 mt-1 h-4 animate-fade">
          <UsageFooter usage={message.usage} durationMs={message.durationMs} modelId={message.modelId} routing={message.routing} />
          <span className="flex-1" />
          <div className="flex items-center gap-2 opacity-0 group-hover/msg:opacity-100 transition-opacity">
            {isLast && <button type="button" onClick={() => void regenerate(sessionId, message.id)} title="Regenerate" className="text-faint hover:text-fg press"><RotateCcw size={11} /></button>}
            {allText && <button type="button" onClick={() => void copy()} title="Copy response" className="text-faint hover:text-fg press">{copied ? <Check size={11} className="text-success animate-pop" /> : <Copy size={11} />}</button>}
          </div>
        </div>
      )}
    </div>
  )
})

import { memo, useEffect, useRef, useState } from 'react'
import { ArrowDown, ArrowUp, Clock, Send, X, Zap } from 'lucide-react'
import type { QueuedMessage } from '@/types'
import { useChat } from '@/store/chat'
import { usePresenceList } from './presence'

const NONE: QueuedMessage[] = []
const label = (q: QueuedMessage) => q.text || `${q.images.length} image(s)`

/**
 * Pending work below the chat: messages steered into the running turn (read-only, "Steering…" until the runner
 * injects them) and the queue that drains after the turn. Queue rows are editable (click the text; Enter saves,
 * Esc cancels), reorderable (arrows) and keep Send now / Remove.
 */
export const QueuedMessages = memo(function QueuedMessages({ sessionId, queued }: { sessionId: string; queued: QueuedMessage[] }) {
  const steering = useChat((s) => s.steering[sessionId]) ?? NONE
  const rows = usePresenceList(queued, (q) => q.id)
  const steerRows = usePresenceList(steering, (q) => q.id)
  const [editing, setEditing] = useState<string | null>(null)
  if (!rows.length && !steerRows.length) return null
  return (
    <div className="mx-2 mb-1 rounded-md border border-dashed border-border text-[12px] animate-fade-up" data-queued>
      {steerRows.map(({ key, item: q, leaving }) => (
        <div key={key} data-steering className={`flex items-center gap-2 px-2 h-7 ${leaving ? 'chat-row-out' : 'chat-enter-slide'}`} title="Injected before the agent's next step">
          <Zap size={12} className="text-warning" /><span className="truncate-1 flex-1 text-muted">{label(q)}</span><span className="text-[10.5px] text-faint">Steering…</span>
        </div>
      ))}
      {rows.map(({ key, item: q, leaving }) => {
        const index = queued.findIndex((x) => x.id === q.id)
        return <QueuedRow key={key} sessionId={sessionId} q={q} index={index} count={queued.length} leaving={leaving} editing={editing === q.id} onEdit={() => setEditing(q.id)} onDone={() => setEditing((cur) => (cur === q.id ? null : cur))} />
      })}
    </div>
  )
})

function QueuedRow({ sessionId, q, index, count, leaving, editing, onEdit, onDone }: { sessionId: string; q: QueuedMessage; index: number; count: number; leaving: boolean; editing: boolean; onEdit: () => void; onDone: () => void }) {
  const remove = useChat((s) => s.queueRemove)
  const sendNow = useChat((s) => s.queueSendNow)
  const update = useChat((s) => s.queueUpdate)
  const move = useChat((s) => s.queueMove)
  const [text, setText] = useState(q.text)
  const taRef = useRef<HTMLTextAreaElement>(null)
  const closed = useRef(false)
  useEffect(() => {
    if (!editing) return
    closed.current = false
    setText(q.text)
    requestAnimationFrame(() => { const ta = taRef.current; if (ta) { ta.focus(); ta.setSelectionRange(ta.value.length, ta.value.length) } })
  }, [editing, q.text])
  const finish = async (save: boolean) => {
    if (closed.current) return
    closed.current = true
    const t = text.trim()
    if (save && t && t !== q.text) { try { await update(sessionId, q.id, t) } catch (e) { console.error('queue update failed', e) } }
    onDone()
  }
  if (editing) {
    return (
      <div className="flex items-start gap-2 px-2 py-1" data-queued-editing>
        <Clock size={12} className="text-faint mt-1.5 shrink-0" />
        <textarea ref={taRef} value={text} onChange={(e) => setText(e.target.value)} rows={Math.min(6, Math.max(1, text.split('\n').length))} spellCheck
          onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); void finish(true) } else if (e.key === 'Escape') { e.preventDefault(); void finish(false) } }}
          onBlur={() => void finish(true)} aria-label="Edit queued message" title="Enter saves, Shift+Enter newline, Esc cancels"
          className="flex-1 min-w-0 resize-none rounded border border-border bg-input px-1.5 py-0.5 text-[12px] leading-[1.5] outline-none focus:border-border-strong" />
      </div>
    )
  }
  return (
    <div className={`group/q flex items-center gap-1.5 px-2 h-7 ${leaving ? 'chat-row-out' : 'chat-enter-slide'}`}>
      <Clock size={12} className="text-faint shrink-0" />
      <button type="button" onClick={onEdit} title="Click to edit" className="truncate-1 flex-1 min-w-0 text-left text-muted hover:text-fg">{label(q)}</button>
      <span className="hidden group-hover/q:inline-flex items-center gap-1">
        <button type="button" disabled={index <= 0} onClick={() => void move(sessionId, q.id, index - 1)} title="Move up" aria-label="Move up" className="text-muted hover:text-fg disabled:opacity-30 disabled:hover:text-muted press"><ArrowUp size={12} /></button>
        <button type="button" disabled={index < 0 || index >= count - 1} onClick={() => void move(sessionId, q.id, index + 1)} title="Move down" aria-label="Move down" className="text-muted hover:text-fg disabled:opacity-30 disabled:hover:text-muted press"><ArrowDown size={12} /></button>
        <button type="button" onClick={() => void sendNow(sessionId, q.id)} title="Send now (interrupts current turn)" aria-label="Send now" className="text-muted hover:text-fg press"><Send size={12} /></button>
        <button type="button" onClick={() => void remove(sessionId, q.id)} title="Remove" aria-label="Remove" className="text-muted hover:text-danger press"><X size={12} /></button>
      </span>
    </div>
  )
}

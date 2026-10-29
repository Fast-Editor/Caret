import { memo } from 'react'
import { Clock, Send, X } from 'lucide-react'
import type { QueuedMessage } from '@/types'
import { useChat } from '@/store/chat'
import { usePresenceList } from './presence'

export const QueuedMessages = memo(function QueuedMessages({ sessionId, queued }: { sessionId: string; queued: QueuedMessage[] }) {
  const remove = useChat((s) => s.queueRemove)
  const sendNow = useChat((s) => s.queueSendNow)
  const rows = usePresenceList(queued, (q) => q.id)
  if (!rows.length) return null
  return (
    <div className="mx-2 mb-1 rounded-md border border-dashed border-border text-[12px] animate-fade-up" data-queued>
      {rows.map(({ key, item: q, leaving }) => (
        <div key={key} className={`group/q flex items-center gap-2 px-2 h-7 ${leaving ? 'chat-row-out' : 'chat-enter-slide'}`}>
          <Clock size={12} className="text-faint" /><span className="truncate-1 flex-1 text-muted">{q.text || `${q.images.length} image(s)`}</span>
          <button type="button" onClick={() => void sendNow(sessionId, q.id)} title="Send now (interrupts current turn)" className="opacity-0 group-hover/q:opacity-100 text-muted hover:text-fg press"><Send size={12} /></button>
          <button type="button" onClick={() => void remove(sessionId, q.id)} title="Remove" className="opacity-0 group-hover/q:opacity-100 text-muted hover:text-danger press"><X size={12} /></button>
        </div>
      ))}
    </div>
  )
})

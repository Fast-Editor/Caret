import { memo, useState } from 'react'
import { Check, Copy, Pencil } from 'lucide-react'
import type { ChatMessage } from '@/types'
import { ContextPill } from '../composer/ContextPills'
import { CheckpointLink, ImageThumb } from './Parts'
import { useChat } from '@/store/chat'

export const UserMessage = memo(function UserMessage({ message, sessionId, isLast }: { message: ChatMessage; sessionId: string; isLast: boolean }) {
  const editing = useChat((s) => s.editingMessageId === message.id)
  const setEditing = useChat((s) => s.setEditing)
  const editMessage = useChat((s) => s.editMessage)
  const text = message.parts.filter((p) => p.type === 'text').map((p) => (p.type === 'text' ? p.text : '')).join('\n')
  const ctx = message.parts.flatMap((p) => (p.type === 'context' ? p.items : []))
  const images = message.parts.flatMap((p) => (p.type === 'image' ? [p.image] : []))
  const [draft, setDraft] = useState(text)
  const [copied, setCopied] = useState(false)
  const copy = async () => { try { const { writeText } = await import('@tauri-apps/plugin-clipboard-manager'); await writeText(text) } catch { await navigator.clipboard.writeText(text).catch(() => {}) } setCopied(true); setTimeout(() => setCopied(false), 1000) }
  void isLast
  return (
    <div className="group/msg px-3 py-2">
      <div className="rounded-lg bg-chat-user border border-border px-3 py-2 text-[13px]">
        {ctx.length > 0 && <div className="flex flex-wrap gap-1 mb-1.5">{ctx.map((c) => <ContextPill key={c.id} item={c} small />)}</div>}
        {images.length > 0 && <div className="flex flex-wrap gap-1.5 mb-1.5">{images.map((im) => <ImageThumb key={im.id} image={im} size={64} />)}</div>}
        {editing ? (
          <div className="animate-fade">
            <textarea value={draft} onChange={(e) => setDraft(e.target.value)} autoFocus rows={Math.min(12, Math.max(2, draft.split('\n').length))} className="w-full bg-input border border-border rounded p-2 outline-none focus:border-accent resize-y selectable"
              onKeyDown={(e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); void editMessage(sessionId, message.id, draft) } if (e.key === 'Escape') setEditing(null) }} />
            <div className="flex gap-1.5 mt-1.5 justify-end text-[11.5px]"><button type="button" onClick={() => setEditing(null)} className="px-2 h-6 rounded border border-border hover:bg-hover press">Cancel</button><button type="button" onClick={() => void editMessage(sessionId, message.id, draft)} className="px-2 h-6 rounded bg-accent text-accent-fg press">Save & resend ⌘⏎</button></div>
          </div>
        ) : (
          <div className="whitespace-pre-wrap break-words selectable leading-[1.55]">{text}</div>
        )}
      </div>
      <div className="flex items-center gap-2 mt-0.5 h-4 opacity-0 group-hover/msg:opacity-100 transition-opacity">
        {message.checkpointId && <CheckpointLink sessionId={sessionId} checkpointId={message.checkpointId} />}
        <span className="flex-1" />
        <button type="button" onClick={() => { setDraft(text); setEditing(message.id) }} title="Edit and resend" className="text-faint hover:text-fg press"><Pencil size={11} /></button>
        <button type="button" onClick={() => void copy()} title="Copy" className="text-faint hover:text-fg press">{copied ? <Check size={11} className="text-success animate-pop" /> : <Copy size={11} />}</button>
      </div>
    </div>
  )
})

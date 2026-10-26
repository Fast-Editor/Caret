import { memo, useState } from 'react'
import { Brain, ChevronRight, CircleX, FileText, Hammer, ListChecks, RotateCcw, Undo2 } from 'lucide-react'
import type { AgentQuestion, ImageAttachment, MessagePart, Usage } from '@/types'
import { Markdown } from '@/lib/markdown'
import { useChat } from '@/store/chat'
import { api } from '@/lib/api'
import { runCommand, hasCommand } from '@/lib/commands'
import { fmtCost, fmtDuration, fmtTokens } from '../util'

export const ThinkingPart = memo(function ThinkingPart({ text, streaming, durationMs }: { text: string; streaming: boolean; durationMs?: number }) {
  const [open, setOpen] = useState(false)
  const words = text.split(/\s+/).filter(Boolean).length
  return (
    <div className="my-1">
      <button type="button" onClick={() => setOpen((o) => !o)} className={`flex items-center gap-1.5 text-[11.5px] text-muted hover:text-fg ${streaming ? 'pulse' : ''}`}>
        <ChevronRight size={11} className={`transition-transform ${open ? 'rotate-90' : ''}`} />
        <Brain size={12} />
        <span>{streaming ? 'Thinking…' : durationMs ? `Thought for ${fmtDuration(durationMs)}` : `Thought (${words} words)`}</span>
      </button>
      {open && <div className="mt-1 ml-4 pl-2 border-l border-border text-[11.5px] text-muted max-h-72 overflow-auto"><Markdown text={text || '…'} streaming={streaming} compact /></div>}
    </div>
  )
})

export const QuestionCard = memo(function QuestionCard({ question, answer, sessionId }: { question: AgentQuestion; answer?: string; sessionId: string }) {
  const [text, setText] = useState('')
  const reply = useChat((s) => s.answer)
  const pending = useChat((s) => !!s.questions[question.questionId])
  void sessionId
  return (
    <div className="my-1.5 rounded-md border border-info/40 bg-info/5 p-2 text-[12px]">
      <div className="font-medium">{question.question}</div>
      {answer || !pending ? (
        <div className="mt-1 text-muted">↳ {answer ?? 'answered'}</div>
      ) : (
        <div className="mt-2 flex flex-col gap-1.5">
          {question.options?.map((o) => <button key={o} type="button" onClick={() => void reply(question.questionId, o)} className="text-left px-2 h-7 rounded border border-border hover:bg-hover">{o}</button>)}
          {question.allowFreeText !== false && (
            <form className="flex gap-1.5" onSubmit={(e) => { e.preventDefault(); if (text.trim()) void reply(question.questionId, text.trim()) }}>
              <input value={text} onChange={(e) => setText(e.target.value)} placeholder="Type an answer…" className="flex-1 h-7 px-2 rounded border border-border bg-input outline-none focus:border-accent" />
              <button type="submit" className="px-2 h-7 rounded bg-accent text-accent-fg">Answer</button>
            </form>
          )}
        </div>
      )}
    </div>
  )
})

export const PlanCard = memo(function PlanCard({ path, title, sessionId }: { path: string; title: string; sessionId: string }) {
  const [building, setBuilding] = useState(false)
  const open = () => { if (hasCommand('editor.openPlan')) void runCommand('editor.openPlan', { path }); else if (hasCommand('editor.openFile')) void runCommand('editor.openFile', { path }) }
  const build = async () => { setBuilding(true); try { const sid = await api.plans.build(path, { sessionId }); void useChat.getState().selectSession(sid) } finally { setBuilding(false) } }
  return (
    <div className="my-1.5 rounded-md border border-border bg-panel p-2 text-[12px] flex items-center gap-2">
      <ListChecks size={14} className="text-accent" />
      <button type="button" onClick={open} className="flex-1 text-left hover:underline truncate-1" title={path}><span className="font-medium">{title || 'Plan'}</span><span className="text-faint ml-1.5 mono text-[11px]">{path.split('/').pop()}</span></button>
      <button type="button" onClick={open} className="px-2 h-6 rounded border border-border hover:bg-hover">Open</button>
      <button type="button" onClick={() => void build()} disabled={building} className="flex items-center gap-1 px-2 h-6 rounded bg-accent text-accent-fg hover:bg-accent-hover disabled:opacity-50"><Hammer size={12} /> Build</button>
    </div>
  )
})

export const SummaryPart = memo(function SummaryPart({ text }: { text: string }) {
  const [open, setOpen] = useState(false)
  return (
    <div className="my-1.5 text-[11.5px]">
      <button type="button" onClick={() => setOpen((o) => !o)} className="flex items-center gap-1.5 text-muted hover:text-fg"><ChevronRight size={11} className={`transition-transform ${open ? 'rotate-90' : ''}`} /><FileText size={12} /> Context summarized</button>
      {open && <div className="mt-1 ml-4 pl-2 border-l border-border text-muted max-h-60 overflow-auto"><Markdown text={text} compact /></div>}
    </div>
  )
})

export const ErrorPart = memo(function ErrorPart({ message, onRetry }: { message: string; onRetry?: () => void }) {
  return (
    <div className="my-1.5 rounded-md border border-danger/40 bg-danger/5 p-2 text-[12px] text-danger flex items-start gap-2">
      <CircleX size={14} className="mt-0.5 shrink-0" />
      <div className="flex-1 whitespace-pre-wrap break-words selectable">{message}</div>
      {onRetry && <button type="button" onClick={onRetry} className="flex items-center gap-1 px-2 h-6 rounded border border-danger/40 hover:bg-hover shrink-0"><RotateCcw size={12} /> Retry</button>}
    </div>
  )
})

export const CheckpointLink = memo(function CheckpointLink({ sessionId, checkpointId, className }: { sessionId: string; checkpointId: string; className?: string }) {
  const [busy, setBusy] = useState(false)
  const restore = useChat((s) => s.restoreCheckpoint)
  const onClick = async () => {
    if (!window.confirm('Restore files to this checkpoint? Later edits made by the agent will be reverted.')) return
    setBusy(true)
    try { const files = await restore(sessionId, checkpointId); void runCommand('app.notify', { level: 'success', title: 'Checkpoint restored', message: `${files.length} file(s) reverted` }) }
    catch (e) { void runCommand('app.notify', { level: 'error', title: 'Restore failed', message: e instanceof Error ? e.message : String(e) }) }
    finally { setBusy(false) }
  }
  return <button type="button" onClick={() => void onClick()} disabled={busy} title="Restore workspace files to the state before this message" className={`flex items-center gap-1 text-[11px] text-faint hover:text-fg ${className ?? ''}`}><Undo2 size={11} /> Restore checkpoint</button>
})

export const UsageFooter = memo(function UsageFooter({ usage, durationMs, modelId }: { usage?: Usage; durationMs?: number; modelId?: string }) {
  if (!usage && !durationMs) return null
  const bits: string[] = []
  if (usage) {
    bits.push(`${fmtTokens(usage.inputTokens)} in · ${fmtTokens(usage.outputTokens)} out`)
    if (usage.cacheReadTokens) bits.push(`${fmtTokens(usage.cacheReadTokens)} cached`)
    if (usage.costUsd !== undefined) bits.push(fmtCost(usage.costUsd))
  }
  if (durationMs) bits.push(fmtDuration(durationMs))
  if (modelId) bits.push(modelId)
  return <div className="mt-1 text-[10.5px] text-faint" title="Usage for this response">{bits.join(' · ')}</div>
})

export const ImageThumb = memo(function ImageThumb({ image, onRemove, size = 72 }: { image: ImageAttachment; onRemove?: () => void; size?: number }) {
  const [big, setBig] = useState(false)
  const src = `data:${image.mime};base64,${image.dataBase64}`
  return (
    <>
      <div className="relative group/img inline-block">
        <img src={src} alt={image.name} title={image.name} onClick={() => setBig(true)} style={{ width: size, height: size }} className="object-cover rounded border border-border cursor-zoom-in" />
        {onRemove && <button type="button" onClick={onRemove} title="Remove" className="absolute -top-1.5 -right-1.5 w-4 h-4 rounded-full bg-panel border border-border text-[10px] leading-none opacity-0 group-hover/img:opacity-100">×</button>}
      </div>
      {big && <div className="fixed inset-0 z-50 bg-black/70 flex items-center justify-center cursor-zoom-out" onClick={() => setBig(false)}><img src={src} alt={image.name} className="max-w-[90vw] max-h-[90vh] rounded shadow-lg" /></div>}
    </>
  )
})

export function partKey(p: MessagePart, i: number): string {
  if (p.type === 'tool') return `tool-${p.call.id}`
  if (p.type === 'question') return `q-${p.question.questionId}`
  if (p.type === 'checkpoint') return `cp-${p.checkpointId}`
  return `${p.type}-${i}`
}

import { useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { Wand2, MessageCircleQuestion, X, Check, RotateCcw, Loader2, MessageSquarePlus, AtSign } from 'lucide-react'
import { setInlineEditHost, type InlineEditController, type InlineEditState } from '@/lib/monaco/inlineEdit'
import { api } from '@/lib/api'
import { Markdown } from './Markdown'
import type { ContextItem } from '@/types'
import '@/components/chat/chat.css'

const HISTORY_KEY = 'caret.inlineEdit.history'
function loadHistory(): string[] { try { return JSON.parse(localStorage.getItem(HISTORY_KEY) ?? '[]') } catch { return [] } }
function pushHistory(s: string) { const h = [s, ...loadHistory().filter((x) => x !== s)].slice(0, 30); localStorage.setItem(HISTORY_KEY, JSON.stringify(h)) }

export function registerInlineEditHost(): void {
  setInlineEditHost({
    mount(container, ctrl) {
      const root = createRoot(container)
      root.render(<InlineEditWidget ctrl={ctrl} />)
      return () => { setTimeout(() => root.unmount(), 0) }
    },
  })
}

function InlineEditWidget({ ctrl }: { ctrl: InlineEditController }) {
  const [st, setSt] = useState<InlineEditState>(ctrl.state)
  const [text, setText] = useState(ctrl.state.instruction)
  const [histIdx, setHistIdx] = useState(-1)
  const [mention, setMention] = useState<{ query: string; results: { path: string; relPath: string }[] } | null>(null)
  const taRef = useRef<HTMLTextAreaElement>(null)
  const boxRef = useRef<HTMLDivElement>(null)
  useEffect(() => ctrl.subscribe(setSt), [ctrl])
  useEffect(() => { taRef.current?.focus() }, [])
  useEffect(() => {
    const el = boxRef.current; if (!el) return
    const ro = new ResizeObserver(() => ctrl.setHeight(el.offsetHeight + 8))
    ro.observe(el); return () => ro.disconnect()
  }, [ctrl])
  useEffect(() => { if (st.phase === 'input') setTimeout(() => taRef.current?.focus(), 0) }, [st.phase])

  const submit = () => { const t = text.trim(); if (!t) return; pushHistory(t); setHistIdx(-1); void ctrl.submit(t) }
  const onKey = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (mention) {
      if (e.key === 'Escape') { setMention(null); e.preventDefault(); return }
      if (e.key === 'Enter' && mention.results[0]) { e.preventDefault(); pickMention(mention.results[0]); return }
    }
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); if (st.phase === 'preview' && (e.metaKey || e.ctrlKey)) ctrl.accept(); else submit() }
    else if (e.key === 'Escape') { e.preventDefault(); ctrl.close() }
    else if (e.key === 'Backspace' && (e.metaKey || e.ctrlKey) && st.phase === 'preview') { e.preventDefault(); ctrl.reject() }
    else if (e.key === 'ArrowUp' && !text.includes('\n')) { const h = loadHistory(); const i = Math.min(histIdx + 1, h.length - 1); if (h[i] !== undefined) { setHistIdx(i); setText(h[i]) } e.preventDefault() }
    else if (e.key === 'ArrowDown' && histIdx >= 0) { const h = loadHistory(); const i = histIdx - 1; setHistIdx(i); setText(i < 0 ? '' : h[i]); e.preventDefault() }
  }
  const onChange = async (v: string) => {
    setText(v)
    const m = /(?:^|\s)@([\w./-]*)$/.exec(v)
    if (m) { try { const res = await api.fs.quickOpen(m[1], 8); setMention({ query: m[1], results: res.map((r) => ({ path: r.path, relPath: r.relPath })) }) } catch { setMention(null) } }
    else setMention(null)
  }
  const pickMention = (r: { path: string; relPath: string }) => {
    const item: ContextItem = { id: `file-${r.path}`, kind: 'file', label: r.relPath, path: r.path }
    ctrl.setContext([...st.context.filter((c) => c.id !== item.id), item])
    setText(text.replace(/@[\w./-]*$/, ''))
    setMention(null)
  }
  const busy = st.phase === 'streaming'
  const modeBtn = (m: 'edit' | 'question', icon: React.ReactNode, label: string) => (
    <button className={`px-1.5 py-0.5 rounded flex items-center gap-1 press ${st.mode === m || (m === 'edit' && st.mode === 'generate') ? 'bg-active text-fg' : 'text-muted hover:bg-hover'}`} onClick={() => ctrl.setMode(m)} title={label}>{icon}<span>{label}</span></button>
  )
  return (
    <div ref={boxRef} className="mx-2 my-1 rounded-md border border-border-strong bg-input shadow-[var(--shadow)] text-xs text-fg font-[family-name:var(--font-ui)] animate-fade-down" data-inline-edit data-phase={st.phase} onMouseDown={(e) => e.stopPropagation()}>
      <div className="flex items-center gap-1 px-2 pt-1.5">
        {modeBtn('edit', <Wand2 size={12} />, st.mode === 'generate' ? 'Generate' : 'Edit')}
        {modeBtn('question', <MessageCircleQuestion size={12} />, 'Quick question')}
        <span className="text-faint ml-1">L{st.startLine}{st.endLine !== st.startLine ? `–${st.endLine}` : ''}</span>
        {st.context.map((c) => <span key={c.id} className="ml-1 px-1.5 rounded bg-active text-muted flex items-center gap-1 animate-pop">@{c.label}<button onClick={() => ctrl.setContext(st.context.filter((x) => x.id !== c.id))}><X size={10} /></button></span>)}
        <span className="flex-1" />
        {busy && <Loader2 size={12} className="spin text-muted animate-fade" />}
        <button className="text-muted hover:text-fg" onClick={() => ctrl.close()} title="Close (Esc)"><X size={13} /></button>
      </div>
      <div className="relative px-2 py-1.5">
        <textarea ref={taRef} value={text} onChange={(e) => void onChange(e.target.value)} onKeyDown={onKey} rows={Math.min(5, Math.max(1, text.split('\n').length))} disabled={busy}
          placeholder={st.mode === 'question' ? 'Ask a quick question about this code…' : st.mode === 'generate' ? 'Describe the code to generate…' : 'Describe the edit… (↵ submit · Esc close · @ to add files)'}
          className="w-full resize-none bg-transparent outline-none placeholder:text-faint leading-5" />
        {mention && mention.results.length > 0 && (
          <div className="absolute left-2 top-full z-10 mt-1 w-80 rounded border border-border bg-tooltip shadow-[var(--shadow)] animate-fade-down">
            {mention.results.map((r) => <button key={r.path} className="w-full text-left px-2 py-1 hover:bg-hover flex items-center gap-1 truncate" onClick={() => pickMention(r)}><AtSign size={11} className="text-muted" />{r.relPath}</button>)}
          </div>
        )}
      </div>
      {st.phase === 'streaming' && st.mode !== 'question' && st.streamed && <pre className="mx-2 mb-2 max-h-40 overflow-auto rounded bg-editor p-2 mono text-[11px] text-muted whitespace-pre-wrap animate-fade">{st.streamed.slice(-1200)}</pre>}
      {(st.phase === 'streaming' || st.phase === 'answer') && st.mode === 'question' && (st.streamed || st.answer) && <div className={`mx-2 mb-2 max-h-64 overflow-auto rounded bg-editor p-2 caret-md animate-fade ${st.phase === 'streaming' ? 'chat-live' : ''}`}><Markdown text={st.answer || st.streamed} /></div>}
      {st.phase === 'error' && <div className="mx-2 mb-2 text-danger animate-fade-up">{st.error}</div>}
      {(st.phase === 'preview' || st.phase === 'answer' || st.phase === 'error') && (
        <div key={st.phase} className="flex items-center gap-1 px-2 pb-1.5 animate-fade-up">
          {st.phase === 'preview' && <>
            <button className="px-2 py-0.5 rounded bg-accent text-accent-fg flex items-center gap-1 press" onClick={() => ctrl.accept()}><Check size={12} />Accept <span className="opacity-70">⌘↩</span></button>
            <button className="px-2 py-0.5 rounded bg-active hover:bg-hover flex items-center gap-1 press" onClick={() => ctrl.reject()}><RotateCcw size={12} />Reject <span className="opacity-70">⌘⌫</span></button>
            <span className="text-faint ml-1">Type a follow-up and press ↵ to refine</span>
          </>}
          {st.phase === 'answer' && <button className="px-2 py-0.5 rounded bg-active hover:bg-hover flex items-center gap-1 press" onClick={() => ctrl.addAnswerToChat()}><MessageSquarePlus size={12} />Add to Chat</button>}
          {st.phase === 'error' && <button className="px-2 py-0.5 rounded bg-active hover:bg-hover press" onClick={submit}>Retry</button>}
        </div>
      )}
    </div>
  )
}

import { Loader2, Sparkles, X } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { api } from '@/lib/api'
import { platform } from '@tauri-apps/plugin-os'
import { Kbd } from '@/components/common'
import { useTerminals } from '@/store/terminal'
import { useWorkspace } from '@/store/workspace'
import { terminalRegistry } from './registry'

/** Terminal ⌘K: natural language → shell command (Run / Insert / Cancel). */
export function CmdKBar({ sessionId }: { sessionId: string }) {
  const session = useTerminals((s) => s.sessions[sessionId])
  const [text, setText] = useState('')
  const [command, setCommand] = useState<string | null>(null)
  const [explanation, setExplanation] = useState<string | undefined>()
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  useEffect(() => { inputRef.current?.focus() }, [])
  const close = () => { useTerminals.setState({ cmdkFor: null }); terminalRegistry.get(sessionId)?.focus() }
  const generate = async () => {
    if (!text.trim() || busy) return
    setBusy(true); setError(null)
    try {
      const h = terminalRegistry.get(sessionId)
      const recent = h?.ptyId() ? await api.pty.readBuffer(h.ptyId()!, 60).catch(() => '') : ''
      let os = 'macos'; try { os = platform() } catch { /* not in tauri */ }
      const res = await api.ai.terminalCommand({ requestId: `tcmd-${Date.now()}`, instruction: text.trim(), shell: session?.shell ?? 'zsh', cwd: session?.cwd ?? useWorkspace.getState().workspace?.root ?? '~', recentOutput: recent, os })
      setCommand(res.command); setExplanation(res.explanation)
    } catch (e) { setError(String((e as Error).message ?? e)) } finally { setBusy(false) }
  }
  const run = (newline: boolean) => { if (!command) return; terminalRegistry.get(sessionId)?.sendInput(command + (newline ? '\n' : '')); close() }
  return (
    <div className="mx-2 mt-1 rounded-md border border-accent/60 bg-input shadow-[var(--shadow)] text-xs" data-owns-keys onKeyDown={(e) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close() } }}>
      <div className="flex items-center gap-2 px-2 h-8">
        <Sparkles className="size-3.5 text-accent shrink-0" />
        <input ref={inputRef} value={text} onChange={(e) => setText(e.target.value)} placeholder="Describe the command you want to run…" className="flex-1 bg-transparent outline-none placeholder:text-faint"
          onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); if (command && !busy && e.metaKey) run(true); else void generate() } }} />
        {busy ? <Loader2 className="size-3.5 spin text-muted" /> : <span className="text-faint flex items-center gap-1"><Kbd keys="enter" /> generate</span>}
        <button aria-label="Close" onClick={close} className="text-muted hover:text-fg"><X className="size-3.5" /></button>
      </div>
      {error && <div className="px-2 pb-2 text-danger">{error}</div>}
      {command && (
        <div className="border-t border-border px-2 py-1.5 flex flex-col gap-1">
          <input value={command} onChange={(e) => setCommand(e.target.value)} className="mono w-full bg-transparent outline-none text-fg" spellCheck={false}
            onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); run(true) } if (e.key === 'Tab') { e.preventDefault(); run(false) } }} />
          {explanation && <div className="text-muted">{explanation}</div>}
          <div className="flex items-center gap-3 text-faint">
            <button onClick={() => run(true)} className="text-fg hover:underline flex items-center gap-1">Run <Kbd keys="enter" /></button>
            <button onClick={() => run(false)} className="hover:text-fg flex items-center gap-1">Insert <Kbd keys="tab" /></button>
            <button onClick={close} className="hover:text-fg flex items-center gap-1">Cancel <Kbd keys="escape" /></button>
          </div>
        </div>
      )}
    </div>
  )
}

/** In-terminal find widget (search addon). */
export function TerminalFind({ sessionId }: { sessionId: string }) {
  const [q, setQ] = useState('')
  const [caseSensitive, setCase] = useState(false)
  const [regex, setRegex] = useState(false)
  const [count, setCount] = useState<{ index: number; total: number } | null>(null)
  const ref = useRef<HTMLInputElement>(null)
  useEffect(() => { ref.current?.focus(); const h = terminalRegistry.get(sessionId); const d = h?.search.onDidChangeResults((r) => setCount({ index: r.resultIndex + 1, total: r.resultCount })); return () => { d?.dispose(); h?.search.clearDecorations() } }, [sessionId])
  const opts = { caseSensitive, regex, decorations: { matchBackground: '#e5b86a55', activeMatchBackground: '#e5b86aaa', matchOverviewRuler: '#e5b86a', activeMatchColorOverviewRuler: '#e5b86a' } }
  const find = (dir: 'next' | 'prev') => { const h = terminalRegistry.get(sessionId); if (!h || !q) return; if (dir === 'next') h.search.findNext(q, opts); else h.search.findPrevious(q, opts) }
  const close = () => { useTerminals.setState({ findFor: null }); terminalRegistry.get(sessionId)?.focus() }
  return (
    <div className="absolute top-1 right-4 z-10 flex items-center gap-1 rounded-md border border-border bg-tooltip px-1.5 h-7 text-xs shadow-[var(--shadow)]" data-owns-keys onKeyDown={(e) => { if (e.key === 'Escape') { e.preventDefault(); close() } }}>
      <input ref={ref} value={q} onChange={(e) => { setQ(e.target.value) }} onKeyDown={(e) => { if (e.key === 'Enter') find(e.shiftKey ? 'prev' : 'next') }} placeholder="Find" className="w-40 bg-transparent outline-none" />
      <span className="text-faint w-12 text-right">{count ? `${count.index}/${count.total}` : ''}</span>
      <button aria-pressed={caseSensitive} onClick={() => setCase(!caseSensitive)} className={`px-1 rounded ${caseSensitive ? 'bg-selected text-accent' : 'text-muted'}`}>Aa</button>
      <button aria-pressed={regex} onClick={() => setRegex(!regex)} className={`px-1 rounded ${regex ? 'bg-selected text-accent' : 'text-muted'}`}>.*</button>
      <button onClick={() => find('prev')} className="px-1 text-muted hover:text-fg">↑</button>
      <button onClick={() => find('next')} className="px-1 text-muted hover:text-fg">↓</button>
      <button aria-label="Close" onClick={close} className="text-muted hover:text-fg"><X className="size-3.5" /></button>
    </div>
  )
}

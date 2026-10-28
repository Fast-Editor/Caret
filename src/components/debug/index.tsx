import { useEffect, useMemo, useRef, useState } from 'react'
import { Bug, ChevronDown, ChevronRight, CircleDot, Download, Pause, Play, RotateCcw, Square, StepForward, ArrowDownToLine, ArrowUpFromLine, X, Plus, Trash2, Terminal as TerminalIcon } from 'lucide-react'
import { Button, Tooltip, cn } from '@/components/common'
import { registerCommands, runCommand } from '@/lib/commands'
import { useDebug, initDebugStore } from '@/store/debug'
import { useUi } from '@/store/ui'
import { activeEditor } from '@/store/editor'
import { toggleBreakpointAtCursor } from '@/lib/monaco/debugDecorations'
import type { DapVariable, LaunchConfig } from '@/types'

const rowCls = 'flex items-center gap-1.5 h-[22px] px-2 text-[12px] rounded cursor-pointer hover:bg-hover select-none'


/** Registers `debug.*` commands and the event subscription. Returns a disposer. */
export function initDebugModule(): () => void {
  const offEvents = initDebugStore()
  const d = () => useDebug.getState()
  const live = () => d().state !== 'idle' && d().state !== 'terminated'
  const offCommands = registerCommands([
    { id: 'debug.view', title: 'Run and Debug: Show View', category: 'Debug', keybinding: 'mod+shift+d', run: () => useUi.getState().toggleSidebar('debug') },
    { id: 'debug.startOrContinue', title: 'Debug: Start / Continue', category: 'Debug', keybinding: 'f5', run: () => (d().state === 'stopped' ? d().continue() : live() ? undefined : d().start()) },
    { id: 'debug.start', title: 'Debug: Start Debugging', category: 'Debug', run: (a) => d().start((a as { config?: LaunchConfig } | undefined)?.config) },
    { id: 'debug.stop', title: 'Debug: Stop', category: 'Debug', keybinding: 'shift+f5', run: () => d().stop() },
    { id: 'debug.restart', title: 'Debug: Restart', category: 'Debug', keybinding: 'mod+shift+f5', run: () => d().restart() },
    { id: 'debug.pause', title: 'Debug: Pause', category: 'Debug', keybinding: 'f6', run: () => d().pause() },
    { id: 'debug.stepOver', title: 'Debug: Step Over', category: 'Debug', keybinding: 'f10', run: () => d().next() },
    { id: 'debug.stepInto', title: 'Debug: Step Into', category: 'Debug', keybinding: 'f11', run: () => d().stepIn() },
    { id: 'debug.stepOut', title: 'Debug: Step Out', category: 'Debug', keybinding: 'shift+f11', run: () => d().stepOut() },
    { id: 'debug.toggleBreakpoint', title: 'Debug: Toggle Breakpoint', category: 'Debug', keybinding: 'f9', run: (a) => { const x = a as { path?: string; line?: number } | undefined; if (x?.path && x.line) return d().toggleBreakpoint(x.path, x.line); if (!toggleBreakpointAtCursor(activeEditor() ?? null)) return false } },
    { id: 'debug.removeAllBreakpoints', title: 'Debug: Remove All Breakpoints', category: 'Debug', run: () => d().removeAllBreakpoints() },
    { id: 'debug.evaluate', title: 'Debug: Evaluate in Console', category: 'Debug', hidden: true, run: (a) => d().evaluate(String((a as { expression?: string } | undefined)?.expression ?? '')) },
    { id: 'debug.installAdapter', title: 'Debug: Install Adapter', category: 'Debug', hidden: true, run: (a) => d().installAdapter(String((a as { id?: string } | undefined)?.id ?? 'js-debug')) },
    { id: 'debug.openConsole', title: 'Debug: Open Debug Console', category: 'Debug', run: () => { const ui = useUi.getState() as unknown as { set: (p: Record<string, unknown>) => void }; ui.set({ panelVisible: true, panelView: 'debugConsole' }) } },
  ])
  return () => { offEvents(); offCommands() }
}

// ───────────────────────────── sidebar view ─────────────────────────────

function Section({ title, children, actions, defaultOpen = true, count }: { title: string; children: React.ReactNode; actions?: React.ReactNode; defaultOpen?: boolean; count?: number }) {
  const [open, setOpen] = useState(defaultOpen)
  return (
    <div className="border-b border-border">
      <div className="flex items-center h-[26px] px-1.5 text-[11px] uppercase tracking-wide text-muted select-none group">
        <button type="button" onClick={() => setOpen(!open)} className="flex items-center gap-1 flex-1 min-w-0 text-left">{open ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}<span className="truncate font-medium">{title}</span>{count !== undefined && <span className="text-faint ml-1">{count}</span>}</button>
        <div className="flex items-center gap-0.5 opacity-0 group-hover:opacity-100">{actions}</div>
      </div>
      {open && <div className="pb-1">{children}</div>}
    </div>
  )
}

function VariableRow({ v, depth }: { v: DapVariable; depth: number }) {
  const expanded = useDebug((s) => !!s.expanded[v.variablesReference])
  const children = useDebug((s) => s.variables[v.variablesReference])
  const toggle = useDebug((s) => s.toggleExpanded)
  const expandable = v.variablesReference > 0
  return (
    <>
      <div className={rowCls} style={{ paddingLeft: 8 + depth * 12 }} onClick={() => expandable && void toggle(v.variablesReference)} title={v.type ? `${v.name}: ${v.type}` : v.name}>
        <span className="w-3.5 shrink-0 text-faint">{expandable ? (expanded ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />) : null}</span>
        <span className="text-info shrink-0">{v.name}</span><span className="text-faint">=</span><span className="truncate text-fg mono">{v.value}</span>
      </div>
      {expandable && expanded && (children ? children.map((c) => <VariableRow key={`${v.variablesReference}:${c.name}`} v={c} depth={depth + 1} />) : <div className="text-[11px] text-faint" style={{ paddingLeft: 26 + depth * 12 }}>Loading…</div>)}
    </>
  )
}

export function DebugView() {
  const st = useDebug()
  const [watchInput, setWatchInput] = useState('')
  useEffect(() => { st.init(); void st.loadConfigs() // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  const live = st.state !== 'idle' && st.state !== 'terminated'
  const selected = st.configs.find((c) => c.name === st.selectedConfig)
  const missingAdapter = selected ? st.adapters.find((a) => a.types.includes(selected.type) && !a.installed) : undefined
  const bpFiles = useMemo(() => Object.entries(st.breakpoints).filter(([, l]) => l.length).sort(), [st.breakpoints])
  const rel = (p: string) => p.split('/').slice(-2).join('/')
  return (
    <div className="h-full flex flex-col min-h-0 text-fg" data-keyctx="debug">
      <div className="h-9 shrink-0 flex items-center justify-between px-3 text-[11px] uppercase tracking-wide text-muted font-medium border-b border-border"><span>Run and Debug</span>{live && <span className={cn('text-[10px] normal-case tracking-normal px-1.5 rounded', st.state === 'stopped' ? 'bg-warning/20 text-warning' : 'bg-success/20 text-success')}>{st.state === 'stopped' ? `paused · ${st.stopReason ?? ''}` : st.state}</span>}</div>
      <div className="flex items-center gap-1.5 px-2 py-2 border-b border-border">
        {!live ? (
          <Tooltip label="Start debugging (F5)"><button type="button" onClick={() => void st.start()} disabled={!selected} className="flex items-center justify-center size-7 rounded bg-success/20 text-success hover:bg-success/30 disabled:opacity-40"><Play className="size-4" fill="currentColor" /></button></Tooltip>
        ) : (
          <Tooltip label="Stop (⇧F5)"><button type="button" onClick={() => void st.stop()} className="flex items-center justify-center size-7 rounded bg-danger/20 text-danger hover:bg-danger/30"><Square className="size-3.5" fill="currentColor" /></button></Tooltip>
        )}
        <select value={st.selectedConfig ?? ''} onChange={(e) => st.selectConfig(e.target.value)} className="flex-1 min-w-0 h-7 rounded border border-border bg-input text-[12px] px-1.5 outline-none cursor-pointer" title={selected ? `${selected.type} · ${selected.request} · ${selected.source}` : 'No launch configuration'}>
          {st.configs.length === 0 && <option value="">No configurations</option>}
          {st.configs.map((c) => <option key={c.name} value={c.name}>{c.name}{c.source === 'auto' ? '' : ` (${c.source})`}</option>)}
        </select>
        <Tooltip label="Refresh configurations"><button type="button" onClick={() => void st.loadConfigs()} className="flex items-center justify-center size-7 rounded hover:bg-hover text-muted"><RotateCcw className="size-3.5" /></button></Tooltip>
      </div>
      {missingAdapter && (
        <div className="mx-2 my-2 p-2 rounded border border-warning/40 bg-warning/10 text-[11.5px]">
          <div className="flex items-center gap-1.5 text-warning"><Download className="size-3.5" />{missingAdapter.name} is not installed</div>
          <div className="text-muted mt-1">{missingAdapter.runtimeFound ? missingAdapter.installHint : `Needs ${missingAdapter.runtime} on PATH first.`}</div>
          <Button variant="primary" size="sm" className="mt-2" disabled={!missingAdapter.runtimeFound || st.installing === missingAdapter.id} loading={st.installing === missingAdapter.id} onClick={() => void st.installAdapter(missingAdapter.id)}>Install {missingAdapter.id}</Button>
        </div>
      )}
      <div className="flex-1 min-h-0 overflow-y-auto">
        <Section title="Variables">
          {!live && <div className="px-3 py-1 text-[11px] text-faint">Not paused.</div>}
          {live && st.state !== 'stopped' && <div className="px-3 py-1 text-[11px] text-faint">Running… pause or hit a breakpoint to inspect.</div>}
          {st.state === 'stopped' && st.scopes.map((sc) => (
            <div key={`${sc.name}:${sc.variablesReference}`}>
              <div className={cn(rowCls, 'text-muted')} onClick={() => void st.toggleExpanded(sc.variablesReference)}>{st.expanded[sc.variablesReference] ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}<span className="font-medium">{sc.name}</span>{sc.expensive && <span className="text-[10px] text-faint">expensive</span>}</div>
              {st.expanded[sc.variablesReference] && (st.variables[sc.variablesReference] ?? []).map((v) => <VariableRow key={`${sc.variablesReference}:${v.name}`} v={v} depth={1} />)}
            </div>
          ))}
        </Section>
        <Section title="Watch" count={st.watches.length || undefined} actions={<Tooltip label="Add expression"><button type="button" className="p-0.5 rounded hover:bg-hover" onClick={() => document.getElementById('caret-watch-input')?.focus()}><Plus className="size-3.5" /></button></Tooltip>}>
          {st.watches.map((w) => (
            <div key={w} className={cn(rowCls, 'group/w')}><span className="text-info shrink-0">{w}</span><span className="text-faint">=</span><span className="truncate mono">{st.watchValues[w] ?? (live ? '…' : 'not available')}</span><button type="button" onClick={() => st.removeWatch(w)} className="ml-auto opacity-0 group-hover/w:opacity-100 text-faint hover:text-fg"><X className="size-3" /></button></div>
          ))}
          <form className="px-2 pt-1" onSubmit={(e) => { e.preventDefault(); st.addWatch(watchInput); setWatchInput('') }}><input id="caret-watch-input" value={watchInput} onChange={(e) => setWatchInput(e.target.value)} placeholder="Expression to watch…" className="w-full h-6 rounded border border-border bg-input px-1.5 text-[12px] outline-none focus:border-border-strong" /></form>
        </Section>
        <Section title="Call Stack" count={st.frames.length || undefined}>
          {st.state !== 'stopped' && <div className="px-3 py-1 text-[11px] text-faint">{live ? 'Running' : 'Not paused.'}</div>}
          {st.state === 'stopped' && st.threads.map((t) => (
            <div key={t.id}>
              {st.threads.length > 1 && <div className="px-3 h-5 text-[11px] text-muted truncate">{t.name}</div>}
              {t.id === st.stoppedThreadId && st.frames.map((f) => (
                <div key={f.id} className={cn(rowCls, f.id === st.selectedFrameId && 'bg-selected', f.presentationHint === 'subtle' && 'text-faint')} style={{ paddingLeft: st.threads.length > 1 ? 20 : 8 }} onClick={() => void st.selectFrame(f.id, true)} title={f.path ?? ''}>
                  <span className="truncate">{f.name}</span><span className="ml-auto text-faint text-[11px] shrink-0">{f.sourceName ?? (f.path ? f.path.split('/').pop() : '')}{f.line ? `:${f.line}` : ''}</span>
                </div>
              ))}
            </div>
          ))}
        </Section>
        <Section title="Breakpoints" count={bpFiles.reduce((n, [, l]) => n + l.length, 0) || undefined} actions={<Tooltip label="Remove all breakpoints"><button type="button" className="p-0.5 rounded hover:bg-hover" onClick={() => void st.removeAllBreakpoints()}><Trash2 className="size-3.5" /></button></Tooltip>}>
          {bpFiles.length === 0 && <div className="px-3 py-1 text-[11px] text-faint">Click in the editor gutter or press F9 to add one.</div>}
          {bpFiles.flatMap(([file, lines]) => lines.map((line) => {
            const v = (st.verified[file] ?? []).find((b) => b.line === line)
            return (
              <div key={`${file}:${line}`} className={cn(rowCls, 'group/b')} onClick={() => void runCommand('editor.openFile', { path: file, line, focus: true })} title={file}>
                <CircleDot className={cn('size-3.5 shrink-0', live && v && !v.verified ? 'text-faint' : 'text-danger')} fill={live && v && !v.verified ? 'none' : 'currentColor'} />
                <span className="truncate">{rel(file)}</span><span className="text-faint text-[11px]">:{line}</span>
                <button type="button" onClick={(e) => { e.stopPropagation(); void st.toggleBreakpoint(file, line) }} className="ml-auto opacity-0 group-hover/b:opacity-100 text-faint hover:text-fg"><X className="size-3" /></button>
              </div>
            )
          }))}
        </Section>
        <Section title="Adapters" defaultOpen={false}>
          {st.adapters.map((a) => (
            <div key={a.id} className="px-3 py-1 text-[11.5px] flex items-center gap-2">
              <span className={cn('size-2 rounded-full shrink-0', a.installed ? 'bg-success' : 'bg-faint')} />
              <div className="flex-1 min-w-0"><div className="truncate">{a.name}</div><div className="text-[10.5px] text-faint truncate">{a.installed ? `v${a.version ?? '?'} · ${a.types.join(', ')}` : a.runtimeFound ? a.installHint : `needs ${a.runtime}`}</div></div>
              {!a.installed && <Button size="sm" disabled={!a.runtimeFound || st.installing === a.id} loading={st.installing === a.id} onClick={() => void st.installAdapter(a.id)}>Install</Button>}
            </div>
          ))}
        </Section>
      </div>
      <div className="shrink-0 border-t border-border px-2 py-1.5 flex items-center justify-between text-[11px] text-faint">
        <span>F5 start · F9 breakpoint · F10 over · F11 into</span>
        <button type="button" onClick={() => void runCommand('debug.openConsole')} className="flex items-center gap-1 hover:text-fg"><TerminalIcon className="size-3.5" />Console</button>
      </div>
    </div>
  )
}

// ───────────────────────────── floating toolbar ─────────────────────────────

export function DebugToolbar() {
  const state = useDebug((s) => s.state)
  const st = useDebug()
  if (state === 'idle' || state === 'terminated') return null
  const stopped = state === 'stopped'
  const btn = 'flex items-center justify-center size-7 rounded hover:bg-hover disabled:opacity-30 disabled:hover:bg-transparent text-fg'
  return (
    <div className="absolute top-2 left-1/2 -translate-x-1/2 z-20 flex items-center gap-0.5 px-1 h-9 rounded-md border border-border bg-tooltip shadow-[var(--shadow)]" data-testid="debug-toolbar">
      <Tooltip label={stopped ? 'Continue (F5)' : 'Pause (F6)'}><button type="button" className={cn(btn, stopped ? 'text-success' : 'text-warning')} onClick={() => void (stopped ? st.continue() : st.pause())}>{stopped ? <Play className="size-4" fill="currentColor" /> : <Pause className="size-4" fill="currentColor" />}</button></Tooltip>
      <Tooltip label="Step over (F10)"><button type="button" className={btn} disabled={!stopped} onClick={() => void st.next()}><StepForward className="size-4" /></button></Tooltip>
      <Tooltip label="Step into (F11)"><button type="button" className={btn} disabled={!stopped} onClick={() => void st.stepIn()}><ArrowDownToLine className="size-4" /></button></Tooltip>
      <Tooltip label="Step out (⇧F11)"><button type="button" className={btn} disabled={!stopped} onClick={() => void st.stepOut()}><ArrowUpFromLine className="size-4" /></button></Tooltip>
      <Tooltip label="Restart (⌘⇧F5)"><button type="button" className={btn} onClick={() => void st.restart()}><RotateCcw className="size-4" /></button></Tooltip>
      <Tooltip label="Stop (⇧F5)"><button type="button" className={cn(btn, 'text-danger')} onClick={() => void st.stop()}><Square className="size-3.5" fill="currentColor" /></button></Tooltip>
      <span className="ml-1 pl-2 border-l border-border text-[11px] text-muted max-w-[220px] truncate">{stopped ? `Paused${st.stopReason ? ` · ${st.stopReason}` : ''}` : state === 'starting' ? 'Starting…' : 'Running'}</span>
    </div>
  )
}

// ───────────────────────────── debug console (panel tab) ─────────────────────────────

export function DebugConsole() {
  const lines = useDebug((s) => s.console)
  const live = useDebug((s) => s.state !== 'idle' && s.state !== 'terminated')
  const evaluate = useDebug((s) => s.evaluate)
  const clear = useDebug((s) => s.clearConsole)
  const [input, setInput] = useState('')
  const [hist, setHist] = useState<string[]>([]); const [hi, setHi] = useState(-1)
  const endRef = useRef<HTMLDivElement>(null)
  useEffect(() => { endRef.current?.scrollIntoView({ block: 'end' }) }, [lines.length])
  const color = (c: string) => c === 'stderr' ? 'text-danger' : c === 'input' ? 'text-info' : c === 'result' ? 'text-fg' : c === 'console' || c === 'important' ? 'text-muted' : 'text-fg'
  return (
    <div className="h-full flex flex-col min-h-0 font-mono text-[12px]" data-keyctx="debugConsole">
      <div className="flex-1 min-h-0 overflow-y-auto px-3 py-1 selectable">
        {lines.length === 0 && <div className="text-faint">Program output and evaluation results appear here. Type an expression below while paused.</div>}
        {lines.map((l) => (
          <div key={l.id} className={cn('whitespace-pre-wrap break-words leading-[1.45]', color(l.category))}>
            {l.category === 'input' ? <span className="text-faint select-none">› </span> : null}{l.text.replace(/\n$/, '')}
            {l.source && <button type="button" onClick={() => void runCommand('editor.openFile', { path: l.source, line: l.line, focus: true })} className="ml-2 text-[10.5px] text-faint hover:text-fg">{l.source.split('/').pop()}{l.line ? `:${l.line}` : ''}</button>}
          </div>
        ))}
        <div ref={endRef} />
      </div>
      <form className="shrink-0 flex items-center gap-1.5 border-t border-border px-2 h-8" onSubmit={(e) => { e.preventDefault(); const v = input.trim(); if (!v) return; setHist((h) => [v, ...h.slice(0, 49)]); setHi(-1); setInput(''); void evaluate(v) }}>
        <span className="text-faint select-none">›</span>
        <input value={input} onChange={(e) => setInput(e.target.value)} onKeyDown={(e) => { if (e.key === 'ArrowUp') { e.preventDefault(); const n = Math.min(hi + 1, hist.length - 1); setHi(n); setInput(hist[n] ?? input) } else if (e.key === 'ArrowDown') { e.preventDefault(); const n = Math.max(hi - 1, -1); setHi(n); setInput(n === -1 ? '' : hist[n] ?? '') } }} placeholder={live ? 'Evaluate expression (Enter)' : 'Start a debug session to evaluate expressions'} className="flex-1 bg-transparent outline-none text-[12px]" />
        <Tooltip label="Clear console"><button type="button" onClick={clear} className="text-faint hover:text-fg p-1 rounded hover:bg-hover"><Trash2 className="size-3.5" /></button></Tooltip>
      </form>
    </div>
  )
}

export { Bug as DebugIcon }

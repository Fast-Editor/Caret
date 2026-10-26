import { Terminal, type IMarker } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { SearchAddon } from '@xterm/addon-search'
import { WebLinksAddon } from '@xterm/addon-web-links'
import '@xterm/xterm/css/xterm.css'
import { useEffect, useRef } from 'react'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import { neighborIndex } from '@/lib/terminalBlocks'
import { useSettings } from '@/store/settings'
import { useTerminals } from '@/store/terminal'
import { useWorkspace } from '@/store/workspace'
import { notify } from '@/store/notifications'
import { xtermTheme } from './xtermTheme'
import { terminalRegistry, type BlockMarkers } from './registry'
import { BlockGutter } from './BlockGutter'
import { ContextMenu, cn, type MenuEntry } from '@/components/common'

export function TerminalInstance({ sessionId, active }: { sessionId: string; active: boolean }) {
  const hostRef = useRef<HTMLDivElement>(null)
  const settings = useSettings((s) => s.settings)
  const session = useTerminals((s) => s.sessions[sessionId])
  const blocksOn = settings?.terminal?.blocks !== false
  const aiOn = settings?.terminal?.aiEnabled !== false
  /** Buffer row under the pointer when the context menu was opened (set in capture phase). */
  const menuRow = useRef<number | null>(null)

  useEffect(() => {
    const host = hostRef.current
    if (!host || !session) return
    const st = useSettings.getState().settings
    const trackBlocks = st?.terminal?.blocks !== false
    const maxBlocks = st?.terminal?.maxBlocks ?? 200
    const term = new Terminal({
      allowProposedApi: true, cursorBlink: st?.ui.terminalCursorBlink !== false, cursorStyle: st?.ui.terminalCursorStyle ?? 'block', fontFamily: st?.ui.terminalFontFamily ?? 'Menlo, monospace', fontSize: st?.ui.terminalFontSize ?? 12, lineHeight: 1.2,
      scrollback: st?.ui.terminalScrollback ?? 5000, macOptionIsMeta: true, theme: xtermTheme(st?.ui.theme), minimumContrastRatio: 1, smoothScrollDuration: st?.ui.smoothScrolling === false ? 0 : 80, scrollOnUserInput: true,
    })
    const fit = new FitAddon(); const search = new SearchAddon()
    term.loadAddon(fit); term.loadAddon(search)
    term.loadAddon(new WebLinksAddon((_e, uri) => void api.app.openExternal(uri)))
    term.open(host)
    let webgl: { dispose: () => void } | null = null
    void import('@xterm/addon-webgl').then(({ WebglAddon }) => { try { const a = new WebglAddon(); a.onContextLoss(() => { a.dispose(); webgl = null }); term.loadAddon(a); webgl = a } catch { webgl = null } }).catch(() => {})
    fit.fit()

    let ptyId: string | null = null
    let disposed = false
    const queue: string[] = []
    let raf = 0
    const flush = () => { raf = 0; if (!queue.length) return; const data = queue.join(''); queue.length = 0; term.write(data) }
    const enqueue = (d: string) => { queue.push(d); if (!raf) raf = requestAnimationFrame(flush) }
    const upd = useTerminals.getState().update

    // ── semantic blocks: xterm parses OSC 133 itself, so markers land on the exact rows ──
    const blockMarkers = new Map<number, BlockMarkers>()
    const gutterListeners = new Set<() => void>()
    const notifyGutter = () => gutterListeners.forEach((f) => f())
    let uiOpen: number | null = null
    let uiNext = 0
    let pendingPrompt: IMarker | undefined
    const endMarker = () => { const b = term.buffer.active; return term.registerMarker(b.cursorX === 0 && b.cursorY > 0 ? -1 : 0) ?? term.registerMarker(0) }
    const closeOpen = () => { if (uiOpen == null) return; const m = blockMarkers.get(uiOpen); if (m && !m.end) m.end = endMarker() ?? undefined; uiOpen = null }
    let oscSub: { dispose(): void } | undefined
    if (trackBlocks) {
      oscSub = term.parser.registerOscHandler(133, (data) => {
        const kind = data[0]
        if (kind === 'A') { closeOpen(); pendingPrompt?.dispose(); pendingPrompt = term.registerMarker(0) ?? undefined }
        else if (kind === 'B' || kind === 'C') {
          if (uiOpen == null) {
            const start = pendingPrompt ?? term.registerMarker(0)
            pendingPrompt = undefined
            if (start) {
              uiOpen = uiNext++
              blockMarkers.set(uiOpen, { start, output: term.registerMarker(0) ?? undefined })
              if (blockMarkers.size > maxBlocks) { const oldest = Math.min(...blockMarkers.keys()); const m = blockMarkers.get(oldest); m?.start.dispose(); m?.output?.dispose(); m?.end?.dispose(); blockMarkers.delete(oldest) }
            }
          }
        } else if (kind === 'D') closeOpen()
        notifyGutter()
        return true
      })
    }
    const rangeOf = (index: number): [number, number] | null => {
      const m = blockMarkers.get(index)
      if (!m || m.start.line < 0) return null
      const end = m.end && m.end.line >= 0 ? m.end.line : term.buffer.active.length - 1
      return [m.start.line, Math.max(m.start.line, end)]
    }
    const selectBlock = (index: number) => {
      const r = rangeOf(index); if (!r) return
      term.selectLines(r[0], r[1]); term.scrollToLine(r[0])
      const b = useTerminals.getState().sessions[sessionId]?.blocks.find((x) => x.index === index)
      useTerminals.getState().selectBlock(sessionId, b?.id ?? null)
    }
    const blockAtRow = (row: number): number | null => { for (const i of [...blockMarkers.keys()].reverse()) { const r = rangeOf(i); if (r && row >= r[0] && row <= r[1]) return i } return null }
    const navigate = (dir: 1 | -1) => {
      const sess = useTerminals.getState().sessions[sessionId]
      const cur = sess?.blocks.find((b) => b.id === sess.selectedBlockId)?.index
      const next = neighborIndex([...blockMarkers.keys()].filter((i) => rangeOf(i)), cur, dir)
      if (next != null) selectBlock(next)
    }
    const rowAt = (clientY: number): number | null => {
      const screen = term.element?.querySelector('.xterm-screen') as HTMLElement | null
      if (!screen) return null
      const r = screen.getBoundingClientRect(); const cell = r.height / Math.max(1, term.rows)
      if (!(cell > 0)) return null
      return Math.floor((clientY - r.top) / cell) + term.buffer.active.viewportY
    }

    const ws = useWorkspace.getState().workspace?.root
    const cwd = session.cwd ?? ws
    const shell = session.shell ?? st?.general.defaultShell ?? undefined
    api.pty.spawn({ cwd, shell, cols: term.cols, rows: term.rows, name: 'xterm-256color' }, (ev) => {
      if (disposed) return
      if (ev.type === 'data') enqueue(ev.data)
      else if (ev.type === 'exit') { upd(sessionId, { exitCode: ev.code }); enqueue(`\r\n\x1b[2m[process exited with code ${ev.code ?? 'null'} — press any key to close]\x1b[0m\r\n`) }
      else if (ev.type === 'title') upd(sessionId, { osTitle: ev.title })
      else if (ev.type === 'cwd') upd(sessionId, { cwd: ev.cwd })
      else if (ev.type === 'block-start' || ev.type === 'block-command' || ev.type === 'block-end' || ev.type === 'status') { useTerminals.getState().applyPtyEvent(sessionId, ev, maxBlocks); if (ev.type !== 'status') notifyGutter() }
    }).then((info) => {
      if (disposed) { void api.pty.kill(info.id); return }
      ptyId = info.id
      upd(sessionId, { ptyId: info.id, shell: info.shell, cwd: info.cwd })
      if (session.initialCommand) void api.pty.write(info.id, session.initialCommand + '\n')
    }).catch((e) => { enqueue(`\r\n\x1b[31mFailed to start terminal: ${String((e as Error).message ?? e)}\x1b[0m\r\n`); notify('error', 'Terminal failed to start', String((e as Error).message ?? e)) })

    const dataSub = term.onData((d) => {
      const sess = useTerminals.getState().sessions[sessionId]
      if (sess?.exitCode !== undefined) { useTerminals.getState().remove(sessionId); return }
      if (ptyId) void api.pty.write(ptyId, d)
    })
    const resizeSub = term.onResize(({ cols, rows }) => { if (ptyId) void api.pty.resize(ptyId, cols, rows) })
    const ro = new ResizeObserver(() => { if (host.clientWidth > 0 && host.clientHeight > 0) { try { fit.fit() } catch { /* ignore */ } } })
    ro.observe(host)

    term.attachCustomKeyEventHandler((e) => {
      if (e.type !== 'keydown') return true
      const mod = e.metaKey || e.ctrlKey
      if (mod && e.key.toLowerCase() === 'k' && !e.shiftKey) { if (useSettings.getState().settings?.terminal?.aiEnabled === false) return true; useTerminals.setState({ cmdkFor: sessionId }); return false }
      if (mod && e.key.toLowerCase() === 'f' && !e.shiftKey) { useTerminals.setState({ findFor: sessionId }); return false }
      if (e.metaKey && !e.shiftKey && !e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown') && trackBlocks) { navigate(e.key === 'ArrowUp' ? -1 : 1); return false }
      if (e.metaKey && e.key.toLowerCase() === 'c' && term.hasSelection()) { void navigator.clipboard.writeText(term.getSelection()); return false }
      if (e.metaKey && e.key.toLowerCase() === 'v') { void navigator.clipboard.readText().then((t) => { if (ptyId && t) void api.pty.write(ptyId, t) }); return false }
      if (e.metaKey && e.key.toLowerCase() === 'a' && !e.shiftKey) { term.selectAll(); return false }
      if (mod && e.shiftKey && (e.key.toLowerCase() === 'p' || e.key.toLowerCase() === 'f' || e.key.toLowerCase() === 'e')) return false // app-level chords
      if (mod && (e.key.toLowerCase() === 'p' || e.key.toLowerCase() === 'b' || e.key.toLowerCase() === 'j' || e.key.toLowerCase() === 'l' || e.key.toLowerCase() === 'i' || e.key.toLowerCase() === 'w' || e.key === ',' || e.key === '\\' || e.key.toLowerCase() === 's')) return false
      if (e.ctrlKey && e.key === '`') return false
      return true
    })

    terminalRegistry.set(sessionId, {
      term, fit, search, sessionId, ptyId: () => ptyId, focus: () => term.focus(), write: (d) => term.write(d), clear: () => term.clear(),
      sendInput: (d) => { if (ptyId) void api.pty.write(ptyId, d) },
      blocks: { markers: blockMarkers, rangeOf, selectBlock, blockAtRow, navigate, rowAt, subscribe: (fn) => { gutterListeners.add(fn); return () => { gutterListeners.delete(fn) } } },
    })
    if (active) setTimeout(() => term.focus(), 0)

    return () => {
      disposed = true
      ro.disconnect(); dataSub.dispose(); resizeSub.dispose(); oscSub?.dispose()
      if (raf) cancelAnimationFrame(raf)
      terminalRegistry.delete(sessionId)
      const id = ptyId
      if (id) void api.pty.kill(id).catch(() => {})
      webgl?.dispose()
      term.dispose()
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId])

  // live option updates
  useEffect(() => {
    const h = terminalRegistry.get(sessionId)
    if (!h || !settings) return
    h.term.options.fontFamily = settings.ui.terminalFontFamily
    h.term.options.fontSize = settings.ui.terminalFontSize
    h.term.options.scrollback = settings.ui.terminalScrollback
    h.term.options.theme = xtermTheme(settings.ui.theme)
    h.term.options.cursorStyle = settings.ui.terminalCursorStyle ?? 'block'
    h.term.options.cursorBlink = settings.ui.terminalCursorBlink !== false
    try { h.fit.fit() } catch { /* ignore */ }
  }, [sessionId, settings])
  useEffect(() => { if (active) { const h = terminalRegistry.get(sessionId); if (h) { try { h.fit.fit() } catch { /* ignore */ } h.focus() } } }, [active, sessionId])

  const menu = (): MenuEntry[] => {
    const h = terminalRegistry.get(sessionId)
    const sess = useTerminals.getState().sessions[sessionId]
    const row = menuRow.current
    const idx = h && row != null ? h.blocks.blockAtRow(row) : null
    const block = idx != null ? sess?.blocks.find((b) => b.index === idx) : sess?.blocks[sess.blocks.length - 1]
    const blockEntries: MenuEntry[] = block && blocksOn ? [
      { label: idx != null ? 'Select Command Output' : 'Select Last Command Output', onSelect: () => h?.blocks.selectBlock(block.index) },
      { label: 'Copy Command', disabled: !block.command, onSelect: () => void navigator.clipboard.writeText(block.command ?? '') },
      { label: 'Copy Command Output', onSelect: () => void runCommand('terminal.copyBlockOutput', { sessionId, blockId: block.id }) },
      { label: 'Re-run Command', disabled: !block.command || block.running, onSelect: () => h?.sendInput((block.command ?? '') + '\n') },
      ...(aiOn ? [
        { label: 'Add Command Output to Chat', onSelect: () => void runCommand('terminal.blockToContext', { sessionId, blockId: block.id }) },
        { label: typeof block.exitCode === 'number' && block.exitCode !== 0 ? 'Explain Error (AI)' : 'Explain Output (AI)', onSelect: () => void runCommand('terminal.explainBlock', { sessionId, blockId: block.id }) },
      ] satisfies MenuEntry[] : []),
      { type: 'separator' as const },
    ] : []
    return [
      ...blockEntries,
      { label: 'Copy', shortcut: 'mod+c', disabled: !h?.term.hasSelection(), onSelect: () => { if (h) void navigator.clipboard.writeText(h.term.getSelection()) } },
      { label: 'Paste', shortcut: 'mod+v', onSelect: () => void navigator.clipboard.readText().then((t) => h?.sendInput(t)) },
      { label: 'Select All', shortcut: 'mod+a', onSelect: () => h?.term.selectAll() },
      { type: 'separator' },
      ...(aiOn ? [
        { label: 'Add Terminal Output to Chat', onSelect: () => void runCommand('chat.addTerminalOutput', { terminalId: session?.ptyId, sessionId }) },
        { label: 'Add Selection to Chat', disabled: !h?.term.hasSelection(), onSelect: () => void runCommand('chat.sendPrompt', { text: '', context: [{ id: `term-sel-${Date.now()}`, kind: 'note', label: 'Terminal selection', data: h?.term.getSelection(), terminalId: session?.ptyId ?? undefined }], submit: false }) },
        { label: 'Generate Command (⌘K)', onSelect: () => useTerminals.setState({ cmdkFor: sessionId }) },
        { type: 'separator' as const },
      ] satisfies MenuEntry[] : []),
      { label: 'Find', shortcut: 'mod+f', onSelect: () => useTerminals.setState({ findFor: sessionId }) },
      { label: 'Clear', onSelect: () => h?.clear() },
      { label: 'Kill Terminal', danger: true, onSelect: () => useTerminals.getState().remove(sessionId) },
    ]
  }
  const status = session?.status
  return (
    <ContextMenu items={menu}>
      <div className="relative h-full w-full" onContextMenuCapture={(e) => { menuRow.current = terminalRegistry.get(sessionId)?.blocks.rowAt(e.clientY) ?? null }}>
        <div ref={hostRef} className={cn('h-full w-full pt-1 pr-2 [&_.xterm]:h-full', blocksOn ? 'pl-3' : 'pl-2')} data-keyctx="terminal" onDragOver={(e) => { if (e.dataTransfer.types.includes('application/x-caret-path')) e.preventDefault() }}
          onDrop={(e) => { const p = e.dataTransfer.getData('application/x-caret-path'); if (p) { e.preventDefault(); terminalRegistry.get(sessionId)?.sendInput(/\s/.test(p) ? `'${p}'` : p) } }} />
        {blocksOn && <BlockGutter sessionId={sessionId} onContextRow={(row) => { menuRow.current = row }} />}
        {status && status.status !== 'idle' && (
          <div className={cn('absolute top-1.5 right-3 z-10 pointer-events-none flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] border backdrop-blur-sm',
            status.status === 'waiting' ? 'border-warning/40 bg-warning/15 text-warning' : status.status === 'failed' ? 'border-danger/40 bg-danger/15 text-danger' : status.status === 'done' ? 'border-success/40 bg-success/15 text-success' : 'border-accent/40 bg-accent/15 text-fg')}
            title={status.detail} data-testid="terminal-status-pill">
            <span className={cn('size-1.5 rounded-full', status.status === 'waiting' ? 'bg-warning animate-pulse' : status.status === 'failed' ? 'bg-danger' : status.status === 'done' ? 'bg-success' : 'bg-accent animate-pulse')} />
            <span className="truncate max-w-[260px]">{status.title ? `${status.title}: ` : ''}{status.status === 'waiting' ? 'waiting for input' : status.detail ?? status.status}</span>
          </div>
        )}
      </div>
    </ContextMenu>
  )
}

import { ChevronDown, Plus, SquareSplitHorizontal, Trash2, MessageSquarePlus, TerminalSquare, BellRing } from 'lucide-react'
import { useEffect } from 'react'
import { IconButton, Menu, cn, type MenuEntry } from '@/components/common'
import { prompt } from '@/components/dialogs'
import { api } from '@/lib/api'
import { emit } from '@/lib/bus'
import { registerCommands, runCommand } from '@/lib/commands'
import { blockContextItem, effectiveStatus, formatBlockText, summarizeStatus, type TerminalStatusEntry, type TerminalStatusSummary } from '@/lib/terminalBlocks'
import { useSettings } from '@/store/settings'
import { useTerminals, type TerminalSession } from '@/store/terminal'
import { useUi } from '@/store/ui'
import { useWorkspace } from '@/store/workspace'
import type { ProgramStatus, TerminalBlock } from '@/types'
import { TerminalInstance } from './TerminalInstance'
import { CmdKBar, TerminalFind } from './CmdKBar'
import { terminalRegistry } from './registry'
export { terminalRegistry } from './registry'

const SHELLS = ['/bin/zsh', '/bin/bash', '/usr/local/bin/fish', '/opt/homebrew/bin/fish', '/bin/sh']

const STATUS_DOT: Record<ProgramStatus, string> = {
  idle: 'text-muted', working: 'text-accent', waiting: 'text-warning', done: 'text-success', failed: 'text-danger',
}
const STATUS_LABEL: Record<ProgramStatus, string> = { idle: '', working: 'running', waiting: 'waiting for input', done: 'done', failed: 'failed' }

function statusOf(s: TerminalSession | undefined): ProgramStatus { return s ? effectiveStatus(s.blocks, s.status, s.exitCode !== undefined) : 'idle' }

/** Snapshot of every terminal's status (for the chip, the bus and `terminal.statusSummary`). */
export function terminalStatusSummary(): TerminalStatusSummary {
  const s = useTerminals.getState()
  const entries: TerminalStatusEntry[] = s.groups.flatMap((g) => g.terminalIds).map((id) => {
    const sess = s.sessions[id]
    const last = sess?.blocks[sess.blocks.length - 1]
    return { sessionId: id, ptyId: sess?.ptyId ?? null, title: sess?.customTitle ?? sess?.title ?? 'terminal', status: statusOf(sess), program: sess?.status?.title, detail: sess?.status?.detail, command: last?.command, exitCode: last?.exitCode }
  })
  return summarizeStatus(entries)
}

export function TerminalPanel() {
  const t = useTerminals()
  const ws = useWorkspace((s) => s.workspace?.root)
  const defaultShell = useSettings((s) => s.settings?.general.defaultShell)
  const aiOn = useSettings((s) => s.settings?.terminal?.aiEnabled !== false)
  const blocksOn = useSettings((s) => s.settings?.terminal?.blocks !== false)
  useEffect(() => { if (t.groups.length === 0) t.create({ cwd: ws }) // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  const group = t.groups.find((g) => g.id === t.activeGroupId) ?? t.groups[0]
  const summary = terminalStatusSummary()
  const shellMenu = (): MenuEntry[] => [
    ...[defaultShell, ...SHELLS].filter((x, i, a): x is string => !!x && a.indexOf(x) === i).map((sh) => ({ label: sh.split('/').pop()! + (sh === defaultShell ? ' (default)' : ''), onSelect: () => t.create({ cwd: ws, shell: sh }) })),
    { type: 'separator' as const },
    { label: 'Custom shell…', onSelect: async () => { const sh = await prompt({ title: 'Shell executable', placeholder: '/path/to/shell' }); if (sh) t.create({ cwd: ws, shell: sh }) } },
  ]
  const tabMenu = (id: string): MenuEntry[] => {
    const last = t.sessions[id]?.blocks[t.sessions[id]!.blocks.length - 1]
    return [
      { label: 'Rename…', onSelect: async () => { const name = await prompt({ title: 'Terminal name', defaultValue: t.sessions[id]?.customTitle ?? t.sessions[id]?.title }); if (name) t.update(id, { customTitle: name, title: name }) } },
      { label: 'Split', onSelect: () => { t.setActive(id); t.create({ cwd: t.sessions[id]?.cwd ?? ws, split: true }) } },
      ...(aiOn ? [
        { label: 'Add Output to Chat', onSelect: () => void runCommand('chat.addTerminalOutput', { terminalId: t.sessions[id]?.ptyId, sessionId: id }) },
        ...(blocksOn && last ? [
          { label: 'Add Last Command Output to Chat', onSelect: () => void runCommand('terminal.blockToContext', { sessionId: id, blockId: last.id }) },
          { label: typeof last.exitCode === 'number' && last.exitCode !== 0 ? 'Explain Last Error (AI)' : 'Explain Last Command (AI)', onSelect: () => void runCommand('terminal.explainBlock', { sessionId: id, blockId: last.id }) },
        ] satisfies MenuEntry[] : []),
      ] satisfies MenuEntry[] : []),
      { type: 'separator' as const },
      { label: 'Kill', danger: true, onSelect: () => t.remove(id) },
    ]
  }
  return (
    <div className="flex h-full min-h-0">
      <div className="flex-1 min-w-0 flex flex-col relative">
        {t.cmdkFor && group?.terminalIds.includes(t.cmdkFor) && <CmdKBar sessionId={t.cmdkFor} />}
        {t.findFor && group?.terminalIds.includes(t.findFor) && <TerminalFind sessionId={t.findFor} />}
        <div className="flex-1 min-h-0 flex">
          {group?.terminalIds.map((id, i) => (
            <div key={id} className={cn('flex-1 min-w-0 h-full relative', i > 0 && 'border-l border-border', t.activeTerminalId === id && group.terminalIds.length > 1 && 'ring-inset ring-1 ring-accent/30')} onMouseDown={() => t.setActive(id)}>
              <TerminalInstance sessionId={id} active={t.activeTerminalId === id} />
            </div>
          ))}
        </div>
      </div>
      <div className="w-[170px] shrink-0 border-l border-border flex flex-col text-xs">
        <div className="flex items-center h-7 px-1 border-b border-border">
          <IconButton size="xs" icon={<Plus />} label="New Terminal" shortcut="ctrl+shift+`" onClick={() => t.create({ cwd: ws })} />
          <Menu items={shellMenu}><button aria-label="Launch profile" className="size-5 rounded flex items-center justify-center text-muted hover:text-fg hover:bg-hover"><ChevronDown className="size-3.5" /></button></Menu>
          <IconButton size="xs" icon={<SquareSplitHorizontal />} label="Split Terminal" onClick={() => t.create({ cwd: ws, split: true })} />
          <IconButton size="xs" icon={<Trash2 />} label="Kill Terminal" disabled={!t.activeTerminalId} onClick={() => t.activeTerminalId && t.remove(t.activeTerminalId)} />
        </div>
        {summary.text && (
          <button type="button" data-testid="terminal-status-summary" onClick={() => void runCommand(summary.waiting ? 'terminal.focusWaiting' : 'terminal.focus')} title={summary.text}
            className={cn('flex items-center gap-1.5 h-6 px-2 border-b border-border text-[11px] truncate text-left hover:bg-hover', summary.waiting ? 'text-warning' : summary.failed && !summary.working ? 'text-danger' : 'text-muted')}>
            {summary.waiting ? <BellRing className="size-3 shrink-0" /> : <span className={cn('size-1.5 rounded-full shrink-0', summary.working ? 'bg-accent animate-pulse' : 'bg-danger')} />}
            <span className="truncate">{summary.text}</span>
          </button>
        )}
        <div className="flex-1 overflow-auto py-1">
          {t.groups.map((g) => (
            <div key={g.id} className={cn('mx-1 rounded', g.id === t.activeGroupId && 'bg-selected/50')}>
              {g.terminalIds.map((id) => {
                const s = t.sessions[id]
                const status = statusOf(s)
                const hint = s?.status?.detail ?? (s?.status?.title ? `${s.status.title} ${STATUS_LABEL[status]}` : STATUS_LABEL[status])
                return (
                  <Menu key={id} items={() => tabMenu(id)}>
                    <div role="button" tabIndex={0} onClick={() => t.setActive(id)} onContextMenu={(e) => e.preventDefault()} data-status={status}
                      className={cn('group flex items-center gap-1.5 h-[24px] px-1.5 rounded cursor-pointer hover:bg-hover transition-base animate-fade-up', t.activeTerminalId === id && 'bg-selected text-fg')} title={[s?.cwd, hint].filter(Boolean).join(' — ')}>
                      <span className="relative shrink-0">
                        <TerminalSquare className={cn('size-3.5', s?.exitCode !== undefined ? 'text-danger' : 'text-muted')} />
                        {status !== 'idle' && <span className={cn('absolute -right-0.5 -bottom-0.5 size-1.5 rounded-full ring-1 ring-panel', STATUS_DOT[status].replace('text-', 'bg-'), (status === 'working' || status === 'waiting') && 'animate-pulse')} />}
                      </span>
                      <span className="truncate flex-1">{s?.customTitle ?? s?.title ?? 'terminal'}</span>
                      {status === 'waiting' && <span className="text-[10px] text-warning shrink-0">input</span>}
                      {aiOn && <button aria-label="Add to chat" onClick={(e) => { e.stopPropagation(); void runCommand('chat.addTerminalOutput', { terminalId: s?.ptyId, sessionId: id }) }} className="opacity-0 group-hover:opacity-100 text-muted hover:text-fg"><MessageSquarePlus className="size-3.5" /></button>}
                      <button aria-label="Kill" onClick={(e) => { e.stopPropagation(); t.remove(id) }} className="opacity-0 group-hover:opacity-100 text-muted hover:text-danger"><Trash2 className="size-3.5" /></button>
                    </div>
                  </Menu>
                )
              })}
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}

interface BlockRef { sessionId?: string; blockId?: string; index?: number }
/** Resolve `{sessionId?, blockId?|index?}` to a live session + block (defaults: active terminal, last block). */
function resolveBlock(a: BlockRef): { sess: TerminalSession; block: TerminalBlock } | null {
  const s = useTerminals.getState()
  const sessionId = a.sessionId ?? s.activeTerminalId
  const sess = sessionId ? s.sessions[sessionId] : undefined
  if (!sess) return null
  const block = a.blockId ? sess.blocks.find((b) => b.id === a.blockId) : a.index != null ? sess.blocks.find((b) => b.index === a.index) : sess.blocks[sess.blocks.length - 1]
  return block ? { sess, block } : null
}
async function blockOutput(sess: TerminalSession, block: TerminalBlock, maxChars = 20000): Promise<string> {
  if (!sess.ptyId) return block.outputPreview ?? ''
  return api.pty.blockOutput(sess.ptyId, block.id, maxChars).catch(() => block.outputPreview ?? '')
}
const aiEnabled = () => useSettings.getState().settings?.terminal?.aiEnabled !== false
const aiOff = () => { void runCommand('app.notify', { level: 'warn', title: 'Terminal AI features are off', message: 'Enable them in Settings → Appearance → Terminal.' }) }

export function registerTerminalCommands(): () => void {
  const ui = () => useUi.getState()
  const show = () => ui().showPanel('terminal')
  const active = () => { const s = useTerminals.getState(); return s.activeTerminalId ? terminalRegistry.get(s.activeTerminalId) : undefined }
  // Publish status changes for other modules (chat agents panel, status bar) without importing them.
  let lastSummary = ''
  const unsubStatus = useTerminals.subscribe(() => {
    const sum = terminalStatusSummary()
    const key = JSON.stringify(sum.terminals.map((e) => [e.sessionId, e.status, e.detail ?? '', e.program ?? '']))
    if (key === lastSummary) return
    lastSummary = key
    emit('terminal.status', sum)
    emit('agents.terminalStatus', sum)
  })
  const unsubCommands = registerCommands([
    { id: 'terminal.new', title: 'New Terminal', category: 'Terminal', keybinding: 'ctrl+shift+`', run: (args) => { const a = (args ?? {}) as { cwd?: string; command?: string }; show(); useTerminals.getState().create({ cwd: a.cwd ?? useWorkspace.getState().workspace?.root, command: a.command }) } },
    { id: 'terminal.toggle', title: 'Toggle Terminal', category: 'Terminal', keybinding: 'ctrl+`', run: () => { const u = ui(); if (u.panelVisible && u.panelView === 'terminal') { const inTerm = !!document.activeElement?.closest('[data-keyctx=terminal]'); if (inTerm) u.set({ panelVisible: false }); else active()?.focus() } else { show(); setTimeout(() => active()?.focus(), 50) } } },
    { id: 'terminal.focus', title: 'Focus Terminal', category: 'Terminal', run: () => { show(); setTimeout(() => active()?.focus(), 30) } },
    { id: 'terminal.kill', title: 'Kill Active Terminal', category: 'Terminal', run: () => { const s = useTerminals.getState(); if (s.activeTerminalId) s.remove(s.activeTerminalId) } },
    { id: 'terminal.split', title: 'Split Terminal', category: 'Terminal', run: () => { show(); useTerminals.getState().create({ cwd: useWorkspace.getState().workspace?.root, split: true }) } },
    { id: 'terminal.clear', title: 'Clear Terminal', category: 'Terminal', run: () => active()?.clear() },
    { id: 'terminal.find', title: 'Find in Terminal', category: 'Terminal', hidden: true, run: () => { const s = useTerminals.getState(); if (s.activeTerminalId) useTerminals.setState({ findFor: s.activeTerminalId }) } },
    { id: 'terminal.runCommand', title: 'Run Command in Terminal', category: 'Terminal', hidden: true, run: async (args) => {
      const a = args as { command: string; newTerminal?: boolean; focus?: boolean }
      show()
      let s = useTerminals.getState()
      if (a.newTerminal || !s.activeTerminalId) { s.create({ cwd: useWorkspace.getState().workspace?.root }); s = useTerminals.getState() }
      const id = s.activeTerminalId!
      const send = (tries: number) => { const h = terminalRegistry.get(id); if (h?.ptyId()) { h.sendInput(a.command + '\n'); if (a.focus !== false) h.focus() } else if (tries > 0) setTimeout(() => send(tries - 1), 150) }
      send(40)
    } },
    { id: 'ai.terminalCmdK', title: 'Terminal: Generate Command (⌘K)', category: 'AI', run: () => { if (!aiEnabled()) return aiOff(); show(); const s = useTerminals.getState(); if (s.activeTerminalId) useTerminals.setState({ cmdkFor: s.activeTerminalId }) } },
    // ── semantic blocks ──
    { id: 'terminal.nextBlock', title: 'Terminal: Next Command Block', category: 'Terminal', run: () => active()?.blocks.navigate(1) },
    { id: 'terminal.prevBlock', title: 'Terminal: Previous Command Block', category: 'Terminal', run: () => active()?.blocks.navigate(-1) },
    { id: 'terminal.selectBlock', title: 'Terminal: Select Command Output', category: 'Terminal', hidden: true, run: (args) => { const r = resolveBlock((args ?? {}) as BlockRef); if (r) terminalRegistry.get(r.sess.id)?.blocks.selectBlock(r.block.index) } },
    { id: 'terminal.blocks', title: 'Terminal: List Command Blocks', category: 'Terminal', hidden: true, run: (args) => { const a = (args ?? {}) as { sessionId?: string }; const s = useTerminals.getState(); const id = a.sessionId ?? s.activeTerminalId; return id ? s.sessions[id]?.blocks ?? [] : [] } },
    { id: 'terminal.copyBlockOutput', title: 'Terminal: Copy Last Command Output', category: 'Terminal', run: async (args) => {
      const r = resolveBlock((args ?? {}) as BlockRef); if (!r) return
      const out = await blockOutput(r.sess, r.block)
      await navigator.clipboard.writeText(out.replace(/\r\n?/g, '\n').replace(/\n+$/, ''))
      void runCommand('app.notify', { level: 'success', title: 'Command output copied', timeoutMs: 1500 })
    } },
    { id: 'terminal.rerunBlock', title: 'Terminal: Re-run Last Command', category: 'Terminal', run: (args) => { const r = resolveBlock((args ?? {}) as BlockRef); if (!r?.block.command) return; show(); const h = terminalRegistry.get(r.sess.id); h?.sendInput(r.block.command + '\n'); h?.focus() } },
    { id: 'terminal.blockToContext', title: 'Terminal: Add Command Output to Chat', category: 'Terminal', run: async (args) => {
      if (!aiEnabled()) return aiOff()
      const a = (args ?? {}) as BlockRef & { text?: string; submit?: boolean; mode?: string; newSession?: boolean }
      const r = resolveBlock(a)
      if (!r) { void runCommand('app.notify', { level: 'warn', title: 'No command block found', message: 'Run a command in a terminal with shell integration first.' }); return }
      const out = await blockOutput(r.sess, r.block)
      const item = blockContextItem(r.block, out, r.sess.ptyId, r.sess.customTitle ?? r.sess.title)
      await runCommand('chat.sendPrompt', { text: a.text ?? '', context: [item], submit: a.submit ?? false, mode: a.mode, newSession: a.newSession })
      void runCommand('chat.open')
      return item
    } },
    { id: 'terminal.explainBlock', title: 'Terminal: Explain Last Command Output (AI)', category: 'AI', run: async (args) => {
      if (!aiEnabled()) return aiOff()
      const r = resolveBlock((args ?? {}) as BlockRef)
      if (!r) { void runCommand('app.notify', { level: 'warn', title: 'No command block found' }); return }
      const failed = typeof r.block.exitCode === 'number' && r.block.exitCode !== 0
      const text = failed ? `This command failed (exit ${r.block.exitCode}). Explain the error and tell me exactly how to fix it.` : 'Explain this terminal output briefly and point out anything that needs attention.'
      return runCommand('terminal.blockToContext', { sessionId: r.sess.id, blockId: r.block.id, text, submit: true, mode: 'ask' })
    } },
    { id: 'terminal.blockText', title: 'Terminal: Command Block as Text', category: 'Terminal', hidden: true, run: async (args) => { const r = resolveBlock((args ?? {}) as BlockRef); if (!r) return undefined; return formatBlockText(r.block, await blockOutput(r.sess, r.block)) } },
    // ── program status (OSC 7501) ──
    { id: 'terminal.statusSummary', title: 'Terminal: Status Summary', category: 'Terminal', hidden: true, run: () => terminalStatusSummary() },
    { id: 'terminal.focusWaiting', title: 'Terminal: Focus Terminal Waiting for Input', category: 'Terminal', run: () => {
      const sum = terminalStatusSummary()
      const target = sum.terminals.find((e) => e.status === 'waiting') ?? sum.terminals.find((e) => e.status === 'working')
      if (!target) { void runCommand('app.notify', { level: 'info', title: 'No terminal is waiting for input', timeoutMs: 2000 }); return false }
      show(); useTerminals.getState().setActive(target.sessionId)
      setTimeout(() => terminalRegistry.get(target.sessionId)?.focus(), 30)
      return true
    } },
  ])
  return () => { unsubCommands(); unsubStatus() }
}

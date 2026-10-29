import { Popover } from 'radix-ui'
import { AlertCircle, AlertTriangle, Bot, Cpu, Database, GitBranch, Loader2, RefreshCw, ShieldCheck, Zap, ChevronUp, ChevronDown } from 'lucide-react'
import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import { runCommand, hasCommand } from '@/lib/commands'
import { subscribe } from '@/lib/bus'
import { Events, on } from '@/lib/tauri'
import { useGit } from '@/store/git'
import { useSettings } from '@/store/settings'
import { useUi } from '@/store/ui'
import { useWorkspace } from '@/store/workspace'
import { NotificationBell } from '@/components/notifications'
import { cn } from '@/components/common'
import type { IndexStatus, LspServerStatus, ProcessStats } from '@/types'

function Item({ children, onClick, title, className }: { children: React.ReactNode; onClick?: () => void; title?: string; className?: string }) {
  const Comp = onClick ? 'button' : 'div'
  return <Comp onClick={onClick} title={title} className={cn('h-full flex items-center gap-1.5 px-2 text-[11px] text-muted whitespace-nowrap', onClick && 'hover:bg-hover hover:text-fg', className)}>{children}</Comp>
}

export function StatusBar() {
  const git = useGit()
  const ws = useWorkspace((s) => s.workspace)
  const settings = useSettings((s) => s.settings)
  const zoom = useUi((s) => s.zoom)
  const [chord, setChord] = useState<string | null>(null)
  const [cursor, setCursor] = useState<{ line: number; column: number; selection?: number } | null>(null)
  const [edStatus, setEdStatus] = useState<{ language?: string; eol?: string; encoding?: string; indent?: string } | null>(null)
  const [tab, setTab] = useState<{ enabled: boolean; busy?: boolean; error?: boolean } | null>(null)
  const [diag, setDiag] = useState<{ errors: number; warnings: number }>({ errors: 0, warnings: 0 })
  const [agents, setAgents] = useState(0)
  const [index, setIndex] = useState<IndexStatus | null>(null)
  const [lsp, setLsp] = useState<LspServerStatus[]>([])
  const [stats, setStats] = useState<ProcessStats | null>(null)

  useEffect(() => { if (ws) void git.refresh() }, [ws?.root]) // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    const offs = [
      subscribe<{ message: string } | null>('keybindings.chord', (p) => setChord(p?.message ?? null)),
      subscribe<{ line: number; column: number; selection?: number }>('editor.cursor', setCursor),
      subscribe<{ language?: string; eol?: string; encoding?: string; indent?: string }>('editor.status', setEdStatus),
      subscribe<{ enabled: boolean; busy?: boolean; error?: boolean }>('tab.status', setTab),
      subscribe<{ running: number }>('agents.status', (p) => setAgents(p?.running ?? 0)),
      subscribe<{ errors: number; warnings: number }>('diagnostics.summary', setDiag),
      on(Events.gitChanged, () => void useGit.getState().refresh()),
      on<IndexStatus>(Events.indexStatus, setIndex),
      on<LspServerStatus[] | LspServerStatus>(Events.lspStatus, (p) => setLsp((prev) => Array.isArray(p) ? p : [...prev.filter((s) => s.serverId !== p.serverId), p])),
      on(Events.lspDiagnostics, () => { if (hasCommand('editor.getDiagnosticsSummary')) void runCommand<{ errors: number; warnings: number }>('editor.getDiagnosticsSummary').then((r) => r && setDiag(r)) }),
    ]
    api.index.status().then(setIndex).catch(() => {})
    api.lsp.status().then(setLsp).catch(() => {})
    return () => offs.forEach((o) => o())
  }, [])
  const showMem = settings?.ui.showStatusBarMemory ?? true
  useEffect(() => {
    if (!showMem) return
    const tick = () => api.app.processStats().then(setStats).catch(() => {})
    tick(); const id = setInterval(tick, 5000)
    return () => clearInterval(id)
  }, [showMem])

  const st = git.status
  const branchLabel = st?.isRepo ? (st.branch ?? (st.headSha ? st.headSha.slice(0, 7) : 'HEAD')) : null
  const dirtyCounts = st ? { staged: st.staged.length, unstaged: st.unstaged.length, untracked: st.untracked.length, conflicted: st.conflicted.length } : null
  const dirty = !!dirtyCounts && dirtyCounts.staged + dirtyCounts.unstaged + dirtyCounts.untracked + dirtyCounts.conflicted > 0
  const dirtyTitle = dirtyCounts && dirty ? ` · uncommitted: ${[dirtyCounts.staged && `${dirtyCounts.staged} staged`, dirtyCounts.unstaged && `${dirtyCounts.unstaged} modified`, dirtyCounts.untracked && `${dirtyCounts.untracked} untracked`, dirtyCounts.conflicted && `${dirtyCounts.conflicted} conflicted`].filter(Boolean).join(', ')}` : ''
  return (
    <div className="h-[22px] shrink-0 flex items-stretch bg-titlebar select-none px-1">
      <div className="flex items-stretch gap-1">
        {branchLabel && (
          <>
            <Item onClick={() => void runCommand('git.checkout')} title={(st?.upstream ? `${branchLabel} → ${st.upstream}` : branchLabel) + dirtyTitle}><GitBranch className="size-3.5" />{branchLabel}{dirty ? '*' : ''}{st?.state && st.state !== 'normal' && st.state !== 'clean' ? ` (${st.state})` : ''}</Item>
            <Item onClick={() => void runCommand(st && st.behind > 0 ? 'git.pull' : 'git.push')} title="Synchronize changes"><RefreshCw className={cn('size-3', git.refreshing && 'spin')} />{st ? <>{st.behind > 0 && <><ChevronDown className="size-3" />{st.behind}</>}{st.ahead > 0 && <><ChevronUp className="size-3" />{st.ahead}</>}</> : null}</Item>
          </>
        )}
        <Item onClick={() => void runCommand('view.problems')} title="Problems"><AlertCircle className={cn('size-3.5', diag.errors && 'text-danger')} />{diag.errors}<AlertTriangle className={cn('size-3.5 ml-1', diag.warnings && 'text-warning')} />{diag.warnings}</Item>
        {agents > 0 && <Item onClick={() => void runCommand('view.agents')} title="Running agents"><Bot className="size-3.5 text-accent pulse" />{agents} running</Item>}
        {index && index.state !== 'disabled' && (
          <Item onClick={() => void runCommand('settings.open', { page: 'indexing' })} title={`Codebase index: ${index.state} · ${index.indexedFiles}/${index.totalFiles} files`}>
            {index.state === 'indexing' || index.state === 'embedding' || index.state === 'scanning' ? <Loader2 className="size-3 spin" /> : <Database className="size-3" />}
            {index.state === 'ready' ? 'Indexed' : index.state === 'indexing' || index.state === 'embedding' ? `Indexing ${Math.round(index.progress * 100)}%` : index.state}
          </Item>
        )}
        {lsp.length > 0 && <LspPopover servers={lsp} />}
        {chord && <Item className="text-fg">{chord}</Item>}
      </div>
      <div className="flex-1" />
      <div className="flex items-stretch gap-1">
        {cursor && <Item onClick={() => void runCommand('editor.goToLine')} title="Go to Line">Ln {cursor.line}, Col {cursor.column}{cursor.selection ? ` (${cursor.selection} selected)` : ''}</Item>}
        {edStatus?.indent && <Item title="Indentation">{edStatus.indent}</Item>}
        {edStatus?.encoding && <Item title="Encoding">{edStatus.encoding}</Item>}
        {edStatus?.eol && <Item title="End of line">{edStatus.eol}</Item>}
        {edStatus?.language && <Item title="Language mode">{edStatus.language}</Item>}
        {tab && <Item onClick={() => void runCommand('ai.tab.toggle')} title={tab.error ? 'Tab completions: the last request failed — check the Tab model in Settings' : 'Tab completions'}><Zap className={cn('size-3', tab.error ? 'text-danger' : tab.enabled ? 'text-accent' : 'text-faint')} />{tab.busy ? 'Tab…' : tab.error ? 'Tab !' : tab.enabled ? 'Tab' : 'Tab off'}</Item>}
        {settings?.privacy.privacyMode && <Item title="Privacy mode: no data leaves beyond your configured model provider"><ShieldCheck className="size-3 text-success" />Privacy</Item>}
        {zoom !== 0 && <Item onClick={() => void runCommand('view.zoomReset')} title="Reset zoom">{Math.round(Math.pow(1.1, zoom) * 100)}%</Item>}
        {showMem && stats && <Item onClick={() => void runCommand('help.about')} title={`Core process RSS · CPU ${stats.cpuPercent.toFixed(0)}%`}><Cpu className="size-3" />{(stats.rssBytes / 1048576).toFixed(0)} MB</Item>}
        <NotificationBell />
      </div>
    </div>
  )
}

function LspPopover({ servers }: { servers: LspServerStatus[] }) {
  const running = servers.filter((s) => s.state === 'running').length
  const configs = useSettings((s) => s.settings?.lsp.servers ?? [])
  return (
    <Popover.Root>
      <Popover.Trigger asChild><button className="h-full flex items-center gap-1 px-1.5 text-[11px] text-muted hover:bg-hover hover:text-fg" title="Language servers"><span className={cn('size-1.5 rounded-full', running ? 'bg-success' : 'bg-faint')} />LSP {running}/{servers.length}</button></Popover.Trigger>
      <Popover.Portal>
        <Popover.Content side="top" align="start" sideOffset={6} className="z-[900] w-[340px] rounded-md border border-border bg-panel shadow-[var(--shadow)] p-2 text-xs">
          <div className="font-medium mb-1">Language servers</div>
          {servers.map((s) => {
            const cfg = configs.find((c) => c.id === s.configId)
            return (
              <div key={s.serverId} className="flex items-center gap-2 h-6">
                <span className={cn('size-1.5 rounded-full', s.state === 'running' ? 'bg-success' : s.state === 'starting' ? 'bg-warning' : s.state === 'error' ? 'bg-danger' : 'bg-faint')} />
                <span className="flex-1 truncate">{s.name}</span><span className="text-muted">{s.state}</span>
                {s.state === 'missing' && cfg?.installHint && <code className="mono text-[10px] text-faint truncate max-w-[140px]" title={cfg.installHint}>{cfg.installHint}</code>}
                {s.state === 'error' && s.error && <span className="text-danger truncate max-w-[140px]" title={s.error}>{s.error}</span>}
              </div>
            )
          })}
          <button onClick={() => void runCommand('settings.open', { page: 'lsp' })} className="mt-1 text-accent hover:underline">Configure…</button>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}

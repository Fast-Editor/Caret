import { memo, useState } from 'react'
import { BookOpen, Bot, Brain, Check, ChevronRight, CircleX, FileDiff, Globe, ListChecks, LoaderCircle, Pencil, Search, Server, ShieldCheck, ShieldAlert, ShieldOff, SquareTerminal, Ban, Globe as Browser, FileText } from 'lucide-react'
import type { ApprovalOption, CommandClassification, CommandRisk, ToolCallRecord } from '@/types'
import { toolGroup, toolTitle, fmtDuration } from '../util'
import { DiffView } from '../DiffView'
import { useChat } from '@/store/chat'
import { runCommand, hasCommand } from '@/lib/commands'
import { Markdown } from '@/lib/markdown'

const ICONS: Record<string, typeof Bot> = { read: FileText, search: Search, edit: Pencil, shell: SquareTerminal, web: Globe, plan: ListChecks, memory: Brain, mcp: Server, browser: Browser, agent: Bot, other: BookOpen }

function StatusIcon({ status }: { status: ToolCallRecord['status'] }) {
  switch (status) {
    case 'running': case 'pending': return <LoaderCircle size={12} className="spin text-muted" />
    case 'awaiting-approval': return <ShieldCheck size={12} className="text-warning" />
    case 'done': return <Check size={12} className="text-success" />
    case 'error': return <CircleX size={12} className="text-danger" />
    case 'rejected': case 'cancelled': return <Ban size={12} className="text-faint" />
  }
}

const OPTION_LABEL: Record<ApprovalOption, string> = { allow: 'Allow', 'allow-session': 'Allow for session', 'allow-always': 'Always allow', deny: 'Deny', 'deny-always': 'Always deny' }

const RISK_CLASS: Record<CommandRisk, string> = { safe: 'text-success bg-success/10 border-success/30', caution: 'text-warning bg-warning/10 border-warning/30', destructive: 'text-danger bg-danger/10 border-danger/30', blocked: 'text-danger bg-danger/15 border-danger/40' }

/** Small uppercase risk pill (safe / caution / destructive / blocked). */
export function RiskPill({ risk, className = '' }: { risk: CommandRisk; className?: string }) {
  return <span className={`inline-flex items-center h-[16px] px-1 rounded border text-[9.5px] font-semibold uppercase tracking-wide ${RISK_CLASS[risk]} ${className}`}>{risk}</span>
}

function SandboxChip({ sandboxed }: { sandboxed?: boolean }) {
  if (sandboxed === undefined) return null
  return sandboxed
    ? <span className="inline-flex items-center gap-0.5 h-[16px] px-1 rounded bg-info/10 text-info text-[9.5px] uppercase tracking-wide" title="Runs inside Caret's OS sandbox: writes limited to the workspace/temp/caches, credential folders unreadable"><ShieldCheck size={9} />sandboxed</span>
    : <span className="inline-flex items-center gap-0.5 h-[16px] px-1 rounded bg-input text-faint text-[9.5px] uppercase tracking-wide" title="No OS sandbox for this command (Settings → Agent → Sandbox)"><ShieldOff size={9} />unsandboxed</span>
}

/** Deterministic risk breakdown of a shell command: overall pill, labels, per-segment risks and reasons. */
function RiskPanel({ c, sandboxed, compact }: { c: CommandClassification; sandboxed?: boolean; compact?: boolean }) {
  const multi = c.segments.length > 1
  return (
    <div className="mt-1.5 text-[11px]">
      <div className="flex flex-wrap items-center gap-1">
        <RiskPill risk={c.overallRisk} />
        <SandboxChip sandboxed={sandboxed} />
        {c.allowlisted && <span className="px-1 h-[16px] inline-flex items-center rounded bg-success/10 text-success text-[9.5px] uppercase">allowlisted</span>}
        {c.denylisted && <span className="px-1 h-[16px] inline-flex items-center rounded bg-danger/10 text-danger text-[9.5px] uppercase">denylisted</span>}
        {c.labels.map((l) => <span key={l} className="px-1 h-[16px] inline-flex items-center rounded bg-input text-muted mono text-[10px]">{l}</span>)}
      </div>
      {!compact && multi && (
        <div className="mt-1 flex flex-col gap-0.5">
          {c.segments.map((s, i) => <div key={i} className="flex items-center gap-1.5 min-w-0"><RiskPill risk={s.risk} className="w-[76px] justify-center shrink-0" /><span className="mono truncate-1 flex-1">{s.text}</span>{s.labels.length > 0 && <span className="text-faint truncate-1 max-w-[45%]">{s.labels.join(', ')}</span>}</div>)}
        </div>
      )}
      {!compact && c.reasons.length > 0 && <ul className="mt-1 text-muted list-disc pl-4">{c.reasons.slice(0, 6).map((r) => <li key={r}>{r}</li>)}</ul>}
    </div>
  )
}

function ApprovalBox({ call }: { call: ToolCallRecord }) {
  const approve = useChat((s) => s.approve)
  const a = call.approval
  if (!a) return null
  const c = a.classification
  const risk: CommandRisk | undefined = c?.overallRisk
  const danger = risk === 'destructive' || risk === 'blocked' || a.riskTier === 2
  const blocked = !!c?.blocked
  const options = blocked ? a.options.filter((o) => !o.startsWith('allow')) : a.options
  const Icon = blocked ? ShieldOff : danger ? ShieldAlert : ShieldCheck
  const tone = blocked || danger ? 'danger' : 'warning'
  return (
    <div className={`mt-2 rounded-md border p-2 text-[12px] ${tone === 'danger' ? 'border-danger/40 bg-danger/5' : 'border-warning/40 bg-warning/5'}`}>
      <div className={`flex items-center gap-1.5 font-medium ${tone === 'danger' ? 'text-danger' : 'text-warning'}`}><Icon size={13} /> {a.title}{a.agentId && <span className="ml-1 px-1 rounded bg-input text-[10px] text-muted font-normal">{a.agentId}</span>}</div>
      {a.detail && a.detail !== a.command && <div className="text-muted mt-0.5 whitespace-pre-wrap break-words">{a.detail}</div>}
      {a.command && <pre className="mono mt-1.5 p-1.5 rounded bg-editor border border-border overflow-x-auto selectable">{a.cwd ? <span className="text-faint">{a.cwd.split('/').pop()} $ </span> : null}{a.command}</pre>}
      {c ? <RiskPanel c={c} sandboxed={a.sandboxed} /> : a.sandboxed !== undefined ? <div className="mt-1.5"><SandboxChip sandboxed={a.sandboxed} /></div> : null}
      {blocked && <div className="mt-1.5 text-danger text-[11px]">This command is blocked by the safety policy and cannot be approved. Edit the denylist in Settings → Agent if this is a mistake.</div>}
      {danger && !blocked && a.allowAlwaysPattern && <div className="mt-1 text-faint text-[10.5px]">“Allow for session / Always allow” apply to this exact command only.</div>}
      <div className="flex flex-wrap gap-1.5 mt-2">
        {options.map((o) => (
          <button key={o} type="button" onClick={() => void approve(a.approvalId, o)} title={o === 'allow-always' && a.allowAlwaysPattern ? `Always allow "${a.allowAlwaysPattern}"` : OPTION_LABEL[o]}
            className={`px-2 h-6 rounded border text-[11.5px] ${o.startsWith('allow') ? (o === 'allow' ? (danger ? 'bg-danger text-white border-danger hover:opacity-90' : 'bg-accent text-accent-fg border-accent hover:bg-accent-hover') : 'border-border hover:bg-hover') : 'border-border text-danger hover:bg-hover'}`}>
            {OPTION_LABEL[o]}{o === 'allow' ? ' ⏎' : ''}
          </button>
        ))}
      </div>
    </div>
  )
}

export const ToolCallCard = memo(function ToolCallCard({ call, sessionId }: { call: ToolCallRecord; sessionId: string }) {
  const [open, setOpen] = useState(call.status === 'awaiting-approval' || call.status === 'error')
  const group = toolGroup(call)
  const Icon = ICONS[group] ?? BookOpen
  const title = toolTitle(call)
  const expanded = open || call.status === 'awaiting-approval'
  const input = call.input && typeof call.input === 'object' ? (call.input as Record<string, unknown>) : null
  const filePath = input && typeof input.path === 'string' ? (input.path as string) : input && typeof input.target_file === 'string' ? (input.target_file as string) : undefined
  const dur = call.startedAt && call.finishedAt ? fmtDuration(call.finishedAt - call.startedAt) : ''
  const isShell = group === 'shell'
  const openFile = () => { if (filePath && hasCommand('editor.openFile')) void runCommand('editor.openFile', { path: filePath }) }
  const openDiff = (changeId?: string) => { if (changeId) { const c = useChat.getState().loaded[sessionId]?.pendingChanges.find((x) => x.changeId === changeId); void useChat.getState().loadChange(changeId).then((pc) => { if (pc && hasCommand('editor.openDiff')) void runCommand('editor.openDiff', { path: pc.path, original: pc.before ?? '', modified: pc.after ?? '', title: `${c?.kind ?? 'change'}: ${pc.path.split('/').pop()}`, changeId, sessionId }) }) } }

  return (
    <div className={`my-1 rounded-md border text-[12px] ${call.status === 'awaiting-approval' ? 'border-warning/50' : call.status === 'error' ? 'border-danger/40' : 'border-border'} bg-panel/60`}>
      <button type="button" onClick={() => setOpen((o) => !o)} className="w-full flex items-center gap-2 px-2 h-7 text-left hover:bg-hover rounded-md" title={call.name}>
        <ChevronRight size={12} className={`text-faint transition-transform ${expanded ? 'rotate-90' : ''}`} />
        <Icon size={13} className="text-muted shrink-0" />
        <span className="truncate-1 flex-1">
          {title}
          {call.isBackground && <span className="ml-1.5 text-[10px] text-faint uppercase">bg</span>}
          {call.mcpServerId && <span className="ml-1.5 px-1 rounded bg-input text-[10px] text-muted">MCP</span>}
        </span>
        {call.classification && call.classification.overallRisk !== 'safe' && call.status !== 'awaiting-approval' && <RiskPill risk={call.classification.overallRisk} />}
        {call.sandboxed && call.status !== 'awaiting-approval' && <ShieldCheck size={11} className="text-info" aria-label="sandboxed" />}
        {call.exitCode !== undefined && call.exitCode !== null && <span className={`mono text-[10.5px] px-1 rounded ${call.exitCode === 0 ? 'text-success bg-success/10' : 'text-danger bg-danger/10'}`}>exit {call.exitCode}</span>}
        {call.fileChanges && call.fileChanges.length > 0 && <span className="text-[10.5px] text-muted">{call.fileChanges.reduce((a, c) => a + c.additions, 0) > 0 && <span className="text-diff-add-fg">+{call.fileChanges.reduce((a, c) => a + c.additions, 0)} </span>}{call.fileChanges.reduce((a, c) => a + c.deletions, 0) > 0 && <span className="text-diff-del-fg">−{call.fileChanges.reduce((a, c) => a + c.deletions, 0)}</span>}</span>}
        {dur && <span className="text-[10.5px] text-faint">{dur}</span>}
        <StatusIcon status={call.status} />
      </button>
      {expanded && (
        <div className="px-2 pb-2 pt-0.5 border-t border-border/60">
          {call.status === 'awaiting-approval' && <ApprovalBox call={call} />}
          {isShell && (call.command || input?.command) ? (
            <>
              <pre className="mono mt-1.5 p-1.5 rounded bg-editor border border-border overflow-x-auto selectable text-[11.5px]"><span className="text-faint">$ </span>{String(call.command ?? input?.command)}</pre>
              {call.status !== 'awaiting-approval' && call.classification && (call.classification.overallRisk !== 'safe' || call.sandboxed !== undefined) && <RiskPanel c={call.classification} sandboxed={call.sandboxed} compact />}
            </>
          ) : input && Object.keys(input).length > 0 && group !== 'edit' ? (
            <details className="mt-1 text-[11px]"><summary className="text-faint cursor-pointer">Input</summary><pre className="mono mt-1 p-1.5 rounded bg-editor border border-border overflow-x-auto selectable max-h-48">{JSON.stringify(input, null, 2)}</pre></details>
          ) : call.inputText && call.status === 'running' && !input ? (
            <pre className="mono mt-1 p-1.5 rounded bg-editor border border-border overflow-x-auto text-[11px] max-h-32 text-faint">{call.inputText.slice(-1500)}</pre>
          ) : null}
          {call.diff && <div className="mt-1.5"><DiffView diff={call.diff} maxLines={200} /></div>}
          {call.fileChanges && call.fileChanges.length > 0 && (
            <div className="mt-1.5 flex flex-col gap-0.5">
              {call.fileChanges.map((fc) => (
                <button key={fc.changeId} type="button" onClick={() => openDiff(fc.changeId)} className="flex items-center gap-2 px-1.5 h-6 rounded hover:bg-hover text-left text-[11.5px]">
                  <FileDiff size={12} className="text-muted" /><span className="truncate-1 flex-1 mono">{fc.path.split('/').slice(-2).join('/')}</span>
                  <span className="text-diff-add-fg">+{fc.additions}</span><span className="text-diff-del-fg">−{fc.deletions}</span>
                  <span className={`text-[10px] uppercase ${fc.status === 'accepted' ? 'text-success' : fc.status === 'rejected' ? 'text-faint' : 'text-warning'}`}>{fc.status}</span>
                </button>
              ))}
            </div>
          )}
          {filePath && group === 'read' && <button type="button" onClick={openFile} className="mt-1 text-[11px] text-accent hover:underline">Open {filePath.split('/').pop()}</button>}
          {call.output && <Output text={call.output} truncated={call.outputTruncated} shell={isShell} markdown={group === 'agent' || group === 'web'} sessionId={sessionId} />}
          {call.error && <div className="mt-1.5 text-danger text-[11.5px] whitespace-pre-wrap break-words">{call.error}</div>}
          {call.subagentSessionId && <button type="button" onClick={() => void useChat.getState().selectSession(call.subagentSessionId!)} className="mt-1.5 text-[11px] text-accent hover:underline">Open subagent transcript →</button>}
          {call.summary && <div className="mt-1.5 text-muted text-[11.5px]">{call.summary}</div>}
        </div>
      )}
    </div>
  )
})

function Output({ text, truncated, shell, markdown, sessionId }: { text: string; truncated?: boolean; shell: boolean; markdown: boolean; sessionId: string }) {
  const [more, setMore] = useState(false)
  const limit = 1800
  const long = text.length > limit
  const shown = more || !long ? text : text.slice(0, limit)
  if (markdown && !shell) return <div className="mt-1.5 text-[12px] rounded border border-border bg-editor p-2 max-h-80 overflow-auto"><Markdown text={shown} sessionId={sessionId} compact />{long && <button type="button" onClick={() => setMore((m) => !m)} className="text-accent text-[11px] mt-1">{more ? 'Show less' : 'Show more'}</button>}</div>
  return (
    <div className="mt-1.5">
      <pre className={`mono p-1.5 rounded bg-editor border border-border overflow-auto selectable text-[11px] leading-[1.45] max-h-72 whitespace-pre-wrap break-words ${shell ? 'text-fg' : 'text-muted'}`}>{shown}{!more && long ? '…' : ''}</pre>
      {(long || truncated) && <div className="flex gap-2 mt-0.5 text-[11px]">{long && <button type="button" onClick={() => setMore((m) => !m)} className="text-accent">{more ? 'Show less' : `Show more (${text.length - limit} chars)`}</button>}{truncated && <span className="text-faint">output truncated by tool limit</span>}</div>}
    </div>
  )
}

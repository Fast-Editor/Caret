import { memo, useEffect, useMemo, useState } from 'react'
import { Check, ChevronDown, ChevronRight, FileDiff, MessageSquareText, X } from 'lucide-react'
import type { FileChangeSummary, ReviewGroup } from '@/types'
import { useChat } from '@/store/chat'
import { useSettings } from '@/store/settings'
import { useReview, useLatestReport, useSessionComments } from '@/store/review'
import { runCommand, hasCommand } from '@/lib/commands'
import { computeGroups, findingsForPath, fileKind, KIND_LABEL, overallFailing } from '@/lib/review'
import { ProofBadge } from '@/components/review/RiskTierPill'
import { MOD } from './util'

const SEV_RANK: Record<string, number> = { critical: 0, high: 1, medium: 2, low: 3, info: 4 }

function proofFor(report: ReturnType<typeof useLatestReport>, path: string): { status: 'pass' | 'warn' | 'fail' | 'running' | 'none'; count: number; title: string } {
  if (!report) return { status: 'none', count: 0, title: 'Not verified yet' }
  const fs = findingsForPath(report, path)
  if (fs.length === 0) return report.status === 'running' || report.status === 'pending' ? { status: 'running', count: 0, title: 'Verifying…' } : { status: 'pass', count: 0, title: 'No findings in this file' }
  const worst = fs.reduce((w, f) => Math.min(w, SEV_RANK[f.severity] ?? 4), 4)
  return { status: worst <= 1 ? 'fail' : 'warn', count: fs.length, title: fs.slice(0, 6).map((f) => `[${f.severity}] ${f.message}${f.line ? ` (L${f.line})` : ''}`).join('\n') + (fs.length > 6 ? `\n… ${fs.length - 6} more` : '') }
}

export const ReviewBar = memo(function ReviewBar({ sessionId, changes }: { sessionId: string; changes: FileChangeSummary[] }) {
  const accept = useChat((s) => s.acceptChanges)
  const reject = useChat((s) => s.rejectChanges)
  const loadChange = useChat((s) => s.loadChange)
  const messages = useChat((s) => s.loaded[sessionId]?.messages)
  const storeGroups = useReview((s) => s.groups[sessionId])
  const loadGroups = useReview((s) => s.loadGroups)
  const loadComments = useReview((s) => s.loadComments)
  const sendComments = useReview((s) => s.sendComments)
  const report = useLatestReport(sessionId)
  const comments = useSessionComments(sessionId)
  const blockOnFail = useSettings((s) => s.settings?.verify?.blockMergeOnFail ?? false)
  const [open, setOpen] = useState(true)
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({})
  const [sending, setSending] = useState(false)
  const pending = useMemo(() => changes.filter((c) => c.status === 'pending'), [changes])
  const pendingKey = pending.map((c) => c.changeId).join('|')
  useEffect(() => { if (pending.length) void loadGroups(sessionId, { pendingChanges: pending, messages: messages ?? [] }) /* eslint-disable-next-line react-hooks/exhaustive-deps */ }, [sessionId, pendingKey, messages?.length])
  useEffect(() => { void loadComments(sessionId) }, [sessionId, loadComments])
  const groups = useMemo<ReviewGroup[]>(() => {
    const byId = new Map(pending.map((c) => [c.changeId, c]))
    const src = storeGroups && storeGroups.length ? storeGroups : computeGroups({ pendingChanges: pending, messages: messages ?? [] })
    const seen = new Set<string>()
    const out = src.map((g) => ({ ...g, changeIds: g.changeIds.filter((id) => byId.has(id) && !seen.has(id) && (seen.add(id), true)) })).filter((g) => g.changeIds.length)
    const rest = pending.filter((c) => !seen.has(c.changeId)).map((c) => c.changeId)
    if (rest.length) out.push({ id: 'rest', title: 'Other changes', kind: 'other', changeIds: rest, collapsedByDefault: false, order: 1e9 })
    return out.sort((a, b) => a.order - b.order)
  }, [pending, storeGroups, messages])
  if (pending.length === 0) return null
  const byId = new Map(pending.map((c) => [c.changeId, c]))
  const adds = pending.reduce((a, c) => a + c.additions, 0), dels = pending.reduce((a, c) => a + c.deletions, 0)
  const failing = overallFailing(report)
  const acceptBlocked = blockOnFail && failing
  const openComments = comments.filter((c) => !c.resolved && c.author === 'user')
  const isCollapsed = (g: ReviewGroup) => collapsed[g.id] ?? g.collapsedByDefault
  const openDiff = async (c: FileChangeSummary) => {
    const pc = await loadChange(c.changeId)
    if (pc && hasCommand('editor.openDiff')) void runCommand('editor.openDiff', { path: pc.path, original: pc.before ?? '', modified: pc.after ?? '', title: `${c.kind}: ${pc.path.split('/').pop()}`, changeId: c.changeId, sessionId })
    else if (hasCommand('editor.openFile')) void runCommand('editor.openFile', { path: c.path })
  }
  const send = async () => { setSending(true); try { await sendComments(sessionId) } finally { setSending(false) } }
  const multi = groups.length > 1 || (groups.length === 1 && groups[0].kind !== 'other')
  const fileRow = (c: FileChangeSummary, indent: boolean) => {
    const kind = fileKind(c.path)
    const proof = proofFor(report, c.path)
    const fileComments = openComments.filter((k) => k.path === c.path || c.path.endsWith('/' + k.path) || k.path.endsWith('/' + c.path)).length
    return (
      <div key={c.changeId} className={`group/ch flex items-center gap-2 pr-2 h-7 hover:bg-hover ${indent ? 'pl-6' : 'pl-2'}`} data-change-id={c.changeId}>
        <button type="button" onClick={() => void openDiff(c)} className="flex-1 min-w-0 flex items-center gap-2 text-left" title={c.path}>
          <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${c.kind === 'create' ? 'bg-success' : c.kind === 'delete' ? 'bg-danger' : 'bg-warning'}`} />
          <span className="truncate-1 mono text-[11.5px]">{c.path.split('/').pop()}</span>
          <span className="truncate-1 text-faint text-[10.5px]">{c.path.split('/').slice(-3, -1).join('/')}</span>
          {kind !== 'source' && kind !== 'other' && <span className="px-1 rounded bg-input text-[9.5px] text-muted uppercase tracking-wide shrink-0">{kind}</span>}
        </button>
        {fileComments > 0 && <span className="flex items-center gap-0.5 text-[10px] text-accent" title={`${fileComments} open comment${fileComments === 1 ? '' : 's'}`}><MessageSquareText size={10} />{fileComments}</span>}
        <ProofBadge status={proof.status} count={proof.count} title={proof.title} />
        <span className="text-diff-add-fg text-[11px]">+{c.additions}</span><span className="text-diff-del-fg text-[11px]">−{c.deletions}</span>
        <button type="button" onClick={() => void reject(sessionId, [c.changeId])} title="Reject" className="opacity-0 group-hover/ch:opacity-100 text-muted hover:text-danger"><X size={12} /></button>
        <button type="button" onClick={() => void accept(sessionId, [c.changeId])} title="Accept" className="opacity-0 group-hover/ch:opacity-100 text-muted hover:text-success"><Check size={12} /></button>
      </div>
    )
  }
  return (
    <div className="mx-2 mb-1 rounded-md border border-border bg-panel text-[12px]" data-review-bar>
      <div className="flex items-center gap-2 px-2 h-8">
        <button type="button" onClick={() => setOpen((o) => !o)} className="flex items-center gap-1 text-muted hover:text-fg">{open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}<FileDiff size={13} /></button>
        <span className="font-medium">{pending.length} file{pending.length === 1 ? '' : 's'} changed</span>
        <span className="text-diff-add-fg">+{adds}</span><span className="text-diff-del-fg">−{dels}</span>
        {multi && <span className="text-[10.5px] text-faint">· {groups.length} concern{groups.length === 1 ? '' : 's'}</span>}
        <span className="flex-1" />
        {openComments.length > 0 && <button type="button" onClick={() => void send()} disabled={sending} title="Send your inline review comments to the agent as one request" className="flex items-center gap-1 px-2 h-6 rounded border border-accent/40 text-accent hover:bg-hover disabled:opacity-50"><MessageSquareText size={12} /> Send {openComments.length} review comment{openComments.length === 1 ? '' : 's'}</button>}
        <button type="button" onClick={() => void reject(sessionId, 'all')} title={`Reject all (${MOD}⌫)`} className="flex items-center gap-1 px-2 h-6 rounded border border-border hover:bg-hover"><X size={12} /> Reject all</button>
        <button type="button" onClick={() => void accept(sessionId, 'all')} disabled={acceptBlocked} title={acceptBlocked ? 'Verification failed and “block accept on failed verification” is on. Fix the findings or disable the setting.' : `Accept all (${MOD}⏎)`} className="flex items-center gap-1 px-2 h-6 rounded bg-accent text-accent-fg hover:bg-accent-hover disabled:opacity-40 disabled:cursor-not-allowed"><Check size={12} /> Accept all</button>
      </div>
      {open && (
        <div className="border-t border-border max-h-56 overflow-y-auto">
          {!multi && groups.flatMap((g) => g.changeIds.map((id) => byId.get(id)!).filter(Boolean).map((c) => fileRow(c, false)))}
          {multi && groups.map((g) => {
            const files = g.changeIds.map((id) => byId.get(id)!).filter(Boolean)
            const ga = files.reduce((a, c) => a + c.additions, 0), gd = files.reduce((a, c) => a + c.deletions, 0)
            const col = isCollapsed(g)
            const label = g.kind === 'file-kind' ? (KIND_LABEL[fileKind(files[0]?.path ?? '')] ?? g.title) : g.title
            return (
              <div key={g.id} data-review-group={g.id}>
                <div className="group/g flex items-center gap-1.5 px-2 h-7 bg-sidebar/40 hover:bg-hover border-b border-border/40">
                  <button type="button" onClick={() => setCollapsed((s) => ({ ...s, [g.id]: !col }))} className="flex-1 min-w-0 flex items-center gap-1.5 text-left">
                    <span className="text-faint">{col ? <ChevronRight size={11} /> : <ChevronDown size={11} />}</span>
                    <span className={`truncate-1 ${g.kind === 'file-kind' ? 'text-muted' : 'font-medium'}`} title={label}>{label}</span>
                    <span className="text-[10.5px] text-faint shrink-0">{files.length} file{files.length === 1 ? '' : 's'}</span>
                    <span className="text-diff-add-fg text-[10.5px]">+{ga}</span><span className="text-diff-del-fg text-[10.5px]">−{gd}</span>
                  </button>
                  <button type="button" onClick={() => void reject(sessionId, g.changeIds)} title="Reject group" className="opacity-0 group-hover/g:opacity-100 flex items-center gap-0.5 px-1.5 h-5 rounded border border-border text-[10.5px] text-muted hover:text-danger"><X size={10} /> Reject</button>
                  <button type="button" onClick={() => void accept(sessionId, g.changeIds)} disabled={acceptBlocked} title={acceptBlocked ? 'Blocked: verification failed' : 'Accept group'} className="opacity-0 group-hover/g:opacity-100 flex items-center gap-0.5 px-1.5 h-5 rounded border border-border text-[10.5px] text-muted hover:text-success disabled:opacity-40"><Check size={10} /> Accept</button>
                </div>
                {!col && files.map((c) => fileRow(c, true))}
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
})

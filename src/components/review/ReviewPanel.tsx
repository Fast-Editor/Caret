import { useEffect, useMemo, useState } from 'react'
import { Bug, ShieldCheck, Trash2 } from 'lucide-react'
import { useReview } from '@/store/review'
import { useChat } from '@/store/chat'
import { fmtRelative } from '@/components/chat/util'
import { ProofPanel } from './ProofPanel'
import { RiskTierPill } from './RiskTierPill'
import { StatusIcon } from './CheckRow'
import { BugbotReview } from './BugbotReview'

type Tab = 'verification' | 'bugbot'

function VerificationList() {
  const order = useReview((s) => s.reportOrder)
  const byId = useReview((s) => s.reportsById)
  const selectedId = useReview((s) => s.selectedReportId)
  const select = useReview((s) => s.selectReport)
  const loadReports = useReview((s) => s.loadReports)
  const activeSession = useChat((s) => s.activeSessionId)
  const runVerification = useReview((s) => s.runVerification)
  useEffect(() => { void loadReports() }, [loadReports])
  const reports = useMemo(() => order.map((id) => byId[id]).filter(Boolean), [order, byId])
  const selected = (selectedId && byId[selectedId]) || reports[0] || null
  return (
    <div className="h-full flex min-w-0">
      <div className="w-[260px] shrink-0 border-r border-border overflow-y-auto">
        <div className="flex items-center gap-2 px-2 h-8 border-b border-border text-[11px] text-muted">
          <span className="flex-1">Reports</span>
          {activeSession && <button type="button" onClick={() => void runVerification(activeSession)} className="px-1.5 h-5 rounded border border-border hover:bg-hover text-[10.5px]">Verify current chat</button>}
        </div>
        {reports.length === 0 && <div className="p-3 text-[11.5px] text-muted">No verification reports yet. Reports appear after agent turns that change files (auto-run) or when you press Verify.</div>}
        {reports.map((r) => (
          <button key={r.id} type="button" onClick={() => select(r.id)} className={`w-full text-left px-2 py-1.5 border-b border-border/60 hover:bg-hover ${selected?.id === r.id ? 'bg-selected' : ''}`}>
            <div className="flex items-center gap-1.5"><StatusIcon status={r.status} /><RiskTierPill tier={r.riskTier} size="xs" /><span className="flex-1" /><span className="text-[10px] text-faint">{fmtRelative(r.startedAt)}</span></div>
            <div className="mt-0.5 text-[11px] text-muted truncate-1">{r.scope.kind}{r.scope.sessionId ? ' · chat' : ''} · {r.filesTouched.length} files · {r.checks.filter((c) => c.status !== 'skipped').length} checks</div>
          </button>
        ))}
      </div>
      <div className="flex-1 min-w-0 overflow-y-auto p-2">
        {selected ? <ProofPanel sessionId={selected.sessionId ?? null} report={selected} compact={false} embedded /> : <div className="h-full flex items-center justify-center text-[12px] text-muted">Select a report</div>}
      </div>
    </div>
  )
}

export function ReviewPanel() {
  const [tab, setTab] = useState<Tab>('verification')
  const hasReports = useReview((s) => s.reportOrder.length > 0)
  const current = useReview((s) => s.current)
  const dismissReview = useReview((s) => s.dismissReview)
  useEffect(() => { if (!hasReports && current) setTab('bugbot') }, [hasReports, current])
  return (
    <div className="h-full flex flex-col min-w-0 bg-panel text-fg">
      <div className="flex items-center gap-1 px-2 h-8 border-b border-border text-[12px]">
        <button type="button" onClick={() => setTab('verification')} className={`flex items-center gap-1 px-2 h-6 rounded ${tab === 'verification' ? 'bg-active text-fg' : 'text-muted hover:text-fg'}`}><ShieldCheck size={12} /> Verification</button>
        <button type="button" onClick={() => setTab('bugbot')} className={`flex items-center gap-1 px-2 h-6 rounded ${tab === 'bugbot' ? 'bg-active text-fg' : 'text-muted hover:text-fg'}`}><Bug size={12} /> Bugbot review</button>
        <span className="flex-1" />
        {tab === 'bugbot' && current && <button type="button" onClick={() => void dismissReview(current.id)} title="Delete review" className="w-6 h-6 flex items-center justify-center rounded hover:bg-hover text-muted"><Trash2 size={12} /></button>}
      </div>
      <div className="flex-1 min-h-0">{tab === 'verification' ? <VerificationList /> : <BugbotReview />}</div>
    </div>
  )
}

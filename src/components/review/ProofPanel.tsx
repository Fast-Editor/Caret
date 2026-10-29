import { useEffect, useMemo, useState } from 'react'
import { LoaderCircle, Play, ShieldCheck, ExternalLink, StopCircle } from 'lucide-react'
import { Chevron, Collapse } from '@/components/chat/Collapse'
import type { VerificationReport } from '@/types'
import { useReview, useLatestReport } from '@/store/review'
import { useSettings } from '@/store/settings'
import { runCommand, hasCommand } from '@/lib/commands'
import { sortChecks, summarizeReport } from '@/lib/review'
import { RiskTierPill } from './RiskTierPill'
import { CheckRow } from './CheckRow'

function fmtElapsed(r: VerificationReport): string {
  const end = r.finishedAt ?? Date.now()
  const ms = Math.max(0, end - r.startedAt)
  return ms < 1000 ? `${ms}ms` : ms < 60_000 ? `${(ms / 1000).toFixed(1)}s` : `${Math.floor(ms / 60_000)}m ${Math.round((ms % 60_000) / 1000)}s`
}

function FactsClaims({ report }: { report: VerificationReport }) {
  if (!report.facts.length && !report.claims.length) return null
  return (
    <div className="grid grid-cols-2 gap-2 px-2 py-1.5 border-t border-border/60 text-[11px]">
      <div>
        <div className="text-[10px] uppercase tracking-wide text-success font-semibold mb-0.5">Verified facts</div>
        {report.facts.length ? <ul className="space-y-0.5">{report.facts.map((f, i) => <li key={i} className="flex gap-1"><span className="text-success">✓</span><span className="break-words">{f}</span></li>)}</ul> : <div className="text-faint">none yet</div>}
      </div>
      <div>
        <div className="text-[10px] uppercase tracking-wide text-muted font-semibold mb-0.5">Agent claims</div>
        {report.claims.length ? <ul className="space-y-0.5">{report.claims.map((c, i) => <li key={i} className="flex gap-1 text-muted"><span className="text-faint">“</span><span className="break-words italic">{c}</span></li>)}</ul> : <div className="text-faint">none detected</div>}
      </div>
    </div>
  )
}

/** Verified Diff proof panel: per-check status, metrics, findings, facts vs claims, risk tier. */
export function ProofPanel({ sessionId, report: explicit, compact = true, embedded }: { sessionId?: string | null; report?: VerificationReport | null; compact?: boolean; embedded?: boolean }) {
  const latest = useLatestReport(sessionId)
  const report = explicit ?? latest
  const verifying = useReview((s) => (sessionId ? !!s.verifying[sessionId] : false))
  const error = useReview((s) => (sessionId ? s.verifyError[sessionId] : undefined))
  const runVerification = useReview((s) => s.runVerification)
  const cancel = useReview((s) => s.cancelVerification)
  const loadLatest = useReview((s) => s.loadLatest)
  const autoRun = useSettings((s) => s.settings?.verify?.autoRunAfterTurn)
  const [open, setOpen] = useState(!compact)
  const [, tick] = useState(0)
  useEffect(() => { if (sessionId && !latest && !explicit) void loadLatest(sessionId) }, [sessionId, latest, explicit, loadLatest])
  useEffect(() => { if (!report || report.finishedAt) return; const id = setInterval(() => tick((n) => n + 1), 1000); return () => clearInterval(id) }, [report])
  const checks = useMemo(() => (report ? sortChecks(report.checks) : []), [report])
  const running = verifying || report?.status === 'running' || report?.status === 'pending'
  if (!sessionId && !report) return null
  if (compact && !report && !verifying && !error) {
    return (
      <div className="mx-2 mb-1 flex items-center gap-2 px-2 h-7 rounded-md border border-dashed border-border text-[11px] text-muted animate-fade">
        <ShieldCheck size={12} className="text-faint" />
        <span className="flex-1 truncate-1">Not verified yet{autoRun === false ? ' · auto-run is off' : ''}</span>
        {sessionId && <button type="button" onClick={() => void runVerification(sessionId)} className="flex items-center gap-1 px-1.5 h-5 rounded border border-border hover:bg-hover text-[10.5px] press"><Play size={10} /> Verify</button>}
        <button type="button" onClick={() => void runCommand('settings.open', { page: 'agent', query: 'verify' })} className="text-[10.5px] text-faint hover:text-fg">settings</button>
      </div>
    )
  }
  const summary = report ? summarizeReport(report) : verifying ? 'Starting verification…' : error ?? ''
  return (
    <div className={`${embedded ? '' : 'mx-2 mb-1'} rounded-md border border-border bg-panel text-[12px] overflow-hidden animate-fade-up`} data-proof-panel data-status={report?.status ?? (verifying ? 'running' : 'none')}>
      <div className="flex items-center gap-2 px-2 h-8">
        <button type="button" onClick={() => setOpen((o) => !o)} className="flex items-center gap-1 text-muted hover:text-fg shrink-0" aria-expanded={open}><Chevron open={open} size={12} />{running ? <LoaderCircle size={13} className="text-accent spin" /> : <ShieldCheck key={report?.status} size={13} className={`animate-pop ${report?.status === 'fail' ? 'text-danger' : report?.status === 'warn' ? 'text-warning' : 'text-success'}`} />}</button>
        <RiskTierPill tier={report?.riskTier} reasons={report?.riskReasons} />
        <span className={`truncate-1 flex-1 min-w-0 ${error && !report ? 'text-danger' : 'text-muted'}`} title={summary}>{summary}</span>
        {report && <span className="text-[10.5px] text-faint tabular-nums shrink-0">{fmtElapsed(report)}</span>}
        {report && compact && hasCommand('view.review') && <button type="button" onClick={() => { useReview.getState().selectReport(report.id); void runCommand('view.review') }} title="Open full report" className="w-6 h-6 flex items-center justify-center rounded hover:bg-hover text-muted"><ExternalLink size={12} /></button>}
        {sessionId && (running && report ? (
          <button type="button" onClick={() => void cancel(report.id)} title="Cancel" className="flex items-center gap-1 px-1.5 h-6 rounded border border-border hover:bg-hover text-[11px] press animate-fade"><StopCircle size={11} /> Stop</button>
        ) : (
          <button type="button" onClick={() => void runVerification(sessionId)} disabled={running} title="Run verification now" className="flex items-center gap-1 px-1.5 h-6 rounded border border-border hover:bg-hover text-[11px] disabled:opacity-50 press animate-fade"><Play size={11} /> Verify{report ? ' again' : ''}</button>
        ))}
      </div>
      <Collapse open={open && !!report}>
        {report && <div className="border-t border-border max-h-[40vh] overflow-y-auto">
          {checks.map((c) => <CheckRow key={c.id} check={c} reportId={report.id} defaultOpen={!compact && c.findings.length > 0} />)}
          {report.riskReasons.length > 0 && <div className="px-2 py-1 border-t border-border/60 text-[11px] text-muted"><span className="font-medium text-fg">Why tier {report.riskTier}:</span> {report.riskReasons.join(' · ')}</div>}
          <FactsClaims report={report} />
          {report.filesTouched.length > 0 && <div className="px-2 py-1 border-t border-border/60 text-[10.5px] text-faint truncate-1" title={report.filesTouched.join('\n')}>{report.filesTouched.length} file{report.filesTouched.length === 1 ? '' : 's'} · +{report.additions} −{report.deletions}</div>}
        </div>}
      </Collapse>
      {open && !report && error && <div className="px-2 py-1.5 border-t border-border text-[11px] text-danger animate-fade">{error}</div>}
    </div>
  )
}

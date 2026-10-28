import { useState } from 'react'
import { Check, ChevronDown, ChevronRight, CircleAlert, CircleX, LoaderCircle, Minus, TriangleAlert, Terminal } from 'lucide-react'
import type { VerifyCheck } from '@/types'
import { metricChips, CHECK_TITLES } from '@/lib/review'
import { FindingRow } from './FindingRow'

export function StatusIcon({ status, size = 12 }: { status: VerifyCheck['status']; size?: number }) {
  switch (status) {
    case 'pass': return <Check size={size} className="text-success" />
    case 'fail': return <CircleX size={size} className="text-danger" />
    case 'error': return <CircleAlert size={size} className="text-danger" />
    case 'warn': return <TriangleAlert size={size} className="text-warning" />
    case 'running': return <LoaderCircle size={size} className="text-accent spin" />
    case 'skipped': return <Minus size={size} className="text-faint" />
    default: return <span className="inline-block rounded-full border border-border-strong" style={{ width: size - 2, height: size - 2 }} />
  }
}

export function CheckRow({ check, reportId, defaultOpen }: { check: VerifyCheck; reportId: string; defaultOpen?: boolean }) {
  const hasBody = check.findings.length > 0 || !!check.output || !!check.skippedReason || !!check.command
  const [open, setOpen] = useState(defaultOpen ?? (check.status === 'fail' || check.status === 'error'))
  const chips = metricChips(check)
  const title = check.title || CHECK_TITLES[check.kind] || check.kind
  return (
    <div className="border-t border-border/60 first:border-t-0">
      <button type="button" onClick={() => hasBody && setOpen((o) => !o)} className={`w-full flex items-center gap-1.5 px-2 h-7 text-left ${hasBody ? 'hover:bg-hover' : ''}`} title={check.summary}>
        <span className="w-3 flex justify-center shrink-0 text-faint">{hasBody ? (open ? <ChevronDown size={11} /> : <ChevronRight size={11} />) : null}</span>
        <StatusIcon status={check.status} />
        <span className="text-[12px] font-medium shrink-0">{title}</span>
        <span className="text-[11px] text-muted truncate-1 flex-1 min-w-0">{check.status === 'skipped' ? (check.skippedReason || 'skipped') : check.summary}</span>
        <span className="flex items-center gap-1 shrink-0">
          {chips.slice(0, 3).map((c) => <span key={c} className="px-1.5 h-4 rounded bg-input text-[10px] text-muted whitespace-nowrap">{c}</span>)}
          {check.findings.length > 0 && <span className={`px-1.5 h-4 rounded text-[10px] ${check.status === 'fail' ? 'bg-danger/15 text-danger' : 'bg-warning/15 text-warning'}`}>{check.findings.length}</span>}
          {check.durationMs !== undefined && <span className="text-[10px] text-faint tabular-nums">{check.durationMs < 1000 ? `${check.durationMs}ms` : `${(check.durationMs / 1000).toFixed(1)}s`}</span>}
        </span>
      </button>
      {open && (
        <div className="pb-1">
          {check.findings.length > 0 && <div className="mx-2 mb-1 rounded border border-border bg-editor/40">{check.findings.map((f) => <FindingRow key={f.id} f={f} reportId={reportId} />)}</div>}
          {check.command && <div className="mx-2 mb-1 flex items-center gap-1.5 text-[10.5px] text-faint mono truncate-1"><Terminal size={10} /> {check.command}</div>}
          {check.output && <details className="mx-2 mb-1"><summary className="text-[10.5px] text-faint cursor-pointer select-none">Raw output</summary><pre className="mt-1 p-1.5 rounded bg-editor border border-border mono text-[10.5px] whitespace-pre-wrap max-h-48 overflow-auto selectable">{check.output.length > 20_000 ? check.output.slice(0, 20_000) + '\n… (truncated)' : check.output}</pre></details>}
        </div>
      )}
    </div>
  )
}

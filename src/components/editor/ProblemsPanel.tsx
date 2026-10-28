import { useMemo, useState } from 'react'
import { AlertCircle, AlertTriangle, Info, ChevronDown, ChevronRight, Sparkles, Filter } from 'lucide-react'
import { useProblems } from '@/store/problems'
import { relPath } from '@/store/workspace'
import { runCommand } from '@/lib/commands'
import type { LspDiagnostic } from '@/types'

function Sev({ s }: { s?: number }) { return s === 2 ? <AlertTriangle size={13} className="text-warning shrink-0" /> : s === 3 || s === 4 ? <Info size={13} className="text-info shrink-0" /> : <AlertCircle size={13} className="text-danger shrink-0" /> }

export function ProblemsPanel() {
  const byPath = useProblems((s) => s.byPath)
  const counts = useProblems((s) => ({ e: s.errors, w: s.warnings, i: s.infos }))
  const [filter, setFilter] = useState('')
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({})
  const [sevFilter, setSevFilter] = useState<'all' | 'errors' | 'warnings'>('all')
  const groups = useMemo(() => {
    const f = filter.toLowerCase()
    const out: { path: string; items: (LspDiagnostic & { owner: string })[] }[] = []
    for (const [path, owners] of Object.entries(byPath)) {
      const items = Object.entries(owners).flatMap(([owner, list]) => list.map((d) => ({ ...d, owner })))
        .filter((d) => sevFilter === 'all' || (sevFilter === 'errors' ? (d.severity ?? 1) === 1 : d.severity === 2))
        .filter((d) => !f || d.message.toLowerCase().includes(f) || path.toLowerCase().includes(f) || String(d.code ?? '').toLowerCase().includes(f))
        .sort((a, b) => (a.severity ?? 1) - (b.severity ?? 1) || a.range.start.line - b.range.start.line)
      if (items.length) out.push({ path, items })
    }
    return out.sort((a, b) => a.path.localeCompare(b.path))
  }, [byPath, filter, sevFilter])
  return (
    <div className="h-full flex flex-col text-xs">
      <div className="h-8 shrink-0 flex items-center gap-2 px-2 border-b border-border">
        <span className="flex items-center gap-1 text-danger"><AlertCircle size={12} />{counts.e}</span>
        <span className="flex items-center gap-1 text-warning"><AlertTriangle size={12} />{counts.w}</span>
        <span className="flex items-center gap-1 text-info"><Info size={12} />{counts.i}</span>
        <span className="flex-1" />
        <Filter size={12} className="text-muted" />
        <select value={sevFilter} onChange={(e) => setSevFilter(e.target.value as typeof sevFilter)} className="bg-input border border-border rounded px-1 h-6 outline-none"><option value="all">All</option><option value="errors">Errors</option><option value="warnings">Warnings</option></select>
        <input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Filter (e.g. text, **/*.ts)" className="w-56 h-6 px-2 rounded bg-input border border-border outline-none focus:border-accent" />
      </div>
      <div className="flex-1 overflow-auto">
        {!groups.length && <div className="p-4 text-muted">No problems have been detected in the workspace.</div>}
        {groups.map((g) => {
          const isCollapsed = collapsed[g.path]
          return (
            <div key={g.path}>
              <button className="w-full flex items-center gap-1 px-2 h-6 hover:bg-hover text-left" onClick={() => setCollapsed((c) => ({ ...c, [g.path]: !c[g.path] }))}>
                {isCollapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
                <span className="font-medium">{g.path.split('/').pop()}</span>
                <span className="text-faint truncate">{relPath(g.path)}</span>
                <span className="ml-1 px-1 rounded bg-active text-muted">{g.items.length}</span>
              </button>
              {!isCollapsed && g.items.map((d, i) => (
                <div key={i} className="group flex items-start gap-2 pl-7 pr-2 py-0.5 hover:bg-hover cursor-pointer" onClick={() => void runCommand('editor.openFile', { path: g.path, line: d.range.start.line + 1, column: d.range.start.character + 1, preview: true })}>
                  <Sev s={d.severity} />
                  <span className="flex-1 min-w-0"><span className="selectable">{d.message}</span> <span className="text-faint">{d.source ? `${d.source}${d.code !== undefined ? `(${String(d.code)})` : ''}` : ''} [Ln {d.range.start.line + 1}, Col {d.range.start.character + 1}]</span></span>
                  <button className="opacity-0 group-hover:opacity-100 px-1.5 rounded bg-active hover:bg-hover flex items-center gap-1 shrink-0" title="Fix with AI" onClick={(e) => { e.stopPropagation(); void runCommand('ai.fixInChat', { path: g.path, line: d.range.start.line + 1, message: d.message }) }}><Sparkles size={11} />Fix</button>
                </div>
              ))}
            </div>
          )
        })}
      </div>
    </div>
  )
}

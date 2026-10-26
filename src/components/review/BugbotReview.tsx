import { useEffect, useMemo, useState } from 'react'
import { Bug, ChevronDown, ChevronRight, LoaderCircle, Play, Trash2, WandSparkles, X } from 'lucide-react'
import type { ReviewFinding, ReviewRequest } from '@/types'
import { useReview } from '@/store/review'
import { api } from '@/lib/api'
import { runCommand, hasCommand } from '@/lib/commands'
import { Markdown, CodeBlock } from '@/lib/markdown'
import { severityRank, uid, fmtRelative, baseName } from '@/components/chat/util'
import { getActiveFile } from '@/lib/chatBridge'

const SEV: Record<string, string> = { critical: 'bg-danger text-inverse', high: 'bg-danger/20 text-danger', medium: 'bg-warning/20 text-warning', low: 'bg-info/20 text-info', info: 'bg-input text-muted' }

function BugbotFindingRow({ f, reviewId }: { f: ReviewFinding; reviewId: string }) {
  const [open, setOpen] = useState(f.severity === 'critical' || f.severity === 'high')
  const dismiss = useReview((s) => s.dismissFinding)
  const openFile = () => { if (hasCommand('editor.openFile')) void runCommand('editor.openFile', { path: f.path, line: f.line, endLine: f.endLine }) }
  const fix = () => void runCommand('chat.sendPrompt', { text: `Fix this review finding in ${baseName(f.path)}:${f.line}: **${f.title}**\n\n${f.description}${f.suggestion ? `\n\nSuggested fix:\n\`\`\`\n${f.suggestion}\n\`\`\`` : ''}`, context: [{ id: uid('f'), kind: 'file', label: baseName(f.path), path: f.path, range: { startLine: Math.max(1, f.line - 30), endLine: (f.endLine ?? f.line) + 30 } }], mode: 'agent', newSession: false, submit: true })
  return (
    <div className="border-b border-border/60">
      <div className="group/f flex items-start gap-2 px-3 py-1.5 hover:bg-hover">
        <button type="button" onClick={() => setOpen((o) => !o)} className="mt-0.5 text-faint">{open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}</button>
        <span className={`mt-0.5 px-1 rounded text-[9.5px] uppercase font-semibold shrink-0 ${SEV[f.severity] ?? SEV.info}`}>{f.severity}</span>
        <div className="flex-1 min-w-0">
          <button type="button" onClick={openFile} className="text-left text-[12px] font-medium hover:underline truncate-1 w-full" title={`${f.path}:${f.line}`}>{f.title}</button>
          <div className="text-[10.5px] text-faint mono truncate-1">{f.path.split('/').slice(-2).join('/')}:{f.line}{f.category ? ` · ${f.category}` : ''}</div>
        </div>
        <div className="flex items-center gap-1 opacity-0 group-hover/f:opacity-100">
          <button type="button" onClick={fix} title="Fix in chat" className="flex items-center gap-1 px-1.5 h-6 rounded border border-border hover:bg-hover text-[11px]"><WandSparkles size={11} /> Fix</button>
          <button type="button" onClick={() => void dismiss(reviewId, f.id)} title="Dismiss" className="w-6 h-6 flex items-center justify-center rounded hover:bg-hover text-muted"><X size={12} /></button>
        </div>
      </div>
      {open && <div className="pl-9 pr-3 pb-2 text-[12px]"><Markdown text={f.description} compact />{f.suggestion && <CodeBlock code={f.suggestion} className={`language-${baseName(f.path).split('.').pop() ?? ''}:${f.path}`} compact />}</div>}
    </div>
  )
}

export function BugbotReview() {
  const st = useReview()
  const [files, setFiles] = useState<string[]>([])
  useEffect(() => { if (!st.loaded) void st.load() }, [st.loaded, st])
  const findings = useMemo(() => {
    const src = st.running ? st.streamingFindings : st.current?.findings ?? []
    return [...src].filter((f) => !st.dismissed.has(f.id)).sort((a, b) => severityRank(a.severity) - severityRank(b.severity) || a.path.localeCompare(b.path))
  }, [st.running, st.streamingFindings, st.current, st.dismissed])
  const byFile = useMemo(() => { const m = new Map<string, ReviewFinding[]>(); for (const f of findings) (m.get(f.path) ?? m.set(f.path, []).get(f.path)!).push(f); return [...m.entries()] }, [findings])
  const run = async () => { let fl: string[] | undefined; if (st.scope === 'files') { const f = await getActiveFile(); fl = f ? [f.path] : files; setFiles(fl) } await st.run({ files: fl }) }
  const counts = findings.reduce<Record<string, number>>((a, f) => { a[f.severity] = (a[f.severity] ?? 0) + 1; return a }, {})
  return (
    <div className="h-full flex flex-col min-w-0 bg-panel text-fg">
      <div className="flex flex-wrap items-center gap-1.5 px-2 h-9 border-b border-border text-[12px]">
        <Bug size={13} className="text-muted" /><span className="font-medium">Review</span>
        <select value={st.scope} onChange={(e) => st.set({ scope: e.target.value as ReviewRequest['scope'] })} className="h-6 px-1 rounded border border-border bg-input text-[11.5px]">
          <option value="working-tree">Working tree</option><option value="staged">Staged</option><option value="branch">Branch vs base</option><option value="commit">Commit</option><option value="files">Current file</option>
        </select>
        {st.scope === 'branch' && <BaseInput />}
        {st.scope === 'commit' && <input value={st.sha} onChange={(e) => st.set({ sha: e.target.value })} placeholder="sha" className="h-6 w-24 px-1 rounded border border-border bg-input text-[11.5px] mono" />}
        <input value={st.focus} onChange={(e) => st.set({ focus: e.target.value })} placeholder="Focus (optional): security, perf, …" className="h-6 flex-1 min-w-[120px] px-1.5 rounded border border-border bg-input text-[11.5px]" />
        <button type="button" onClick={() => void run()} disabled={st.running} className="flex items-center gap-1 px-2 h-6 rounded bg-accent text-accent-fg hover:bg-accent-hover disabled:opacity-50 text-[11.5px]">{st.running ? <LoaderCircle size={12} className="spin" /> : <Play size={12} />} Review</button>
        {st.reviews.length > 0 && <select value={st.current?.id ?? ''} onChange={(e) => st.select(e.target.value)} className="h-6 px-1 rounded border border-border bg-input text-[11px] max-w-[160px]">{st.reviews.map((r) => <option key={r.id} value={r.id}>{fmtRelative(r.createdAt)} · {r.scope} · {r.findings.length}</option>)}</select>}
        {st.current && <button type="button" onClick={() => void st.dismissReview(st.current!.id)} title="Delete review" className="w-6 h-6 flex items-center justify-center rounded hover:bg-hover text-muted"><Trash2 size={12} /></button>}
      </div>
      <div className="flex-1 overflow-y-auto">
        {st.error && <div className="m-2 p-2 rounded border border-danger/40 text-danger text-[12px]">{st.error}</div>}
        {st.running && <div className="px-3 py-2 text-[11.5px] text-muted pulse">{st.status || 'Reviewing…'}</div>}
        {(st.current || findings.length > 0) && (
          <div className="px-3 py-2 border-b border-border text-[12px]">
            <div className="flex flex-wrap items-center gap-1.5 mb-1">{(['critical', 'high', 'medium', 'low', 'info'] as const).filter((s) => counts[s]).map((s) => <span key={s} className={`px-1 rounded text-[10px] uppercase font-semibold ${SEV[s]}`}>{counts[s]} {s}</span>)}{st.current && <span className="text-faint text-[11px]">· {st.current.stats.files} files, +{st.current.stats.additions} −{st.current.stats.deletions} · {st.current.modelId}</span>}</div>
            {st.current?.summary && <Markdown text={st.current.summary} compact className="text-muted" />}
          </div>
        )}
        {byFile.map(([path, fs]) => (
          <div key={path}>
            <div className="sticky top-0 z-10 flex items-center gap-2 px-3 h-7 bg-panel border-b border-border text-[11px] text-muted"><span className="mono truncate-1">{path.split('/').slice(-3).join('/')}</span><span className="text-faint">{fs.length}</span></div>
            {fs.map((f) => <BugbotFindingRow key={f.id} f={f} reviewId={st.current?.id ?? ''} />)}
          </div>
        ))}
        {!st.running && findings.length === 0 && !st.error && (
          <div className="flex flex-col items-center justify-center py-10 text-center text-[12px] text-muted"><Bug size={22} className="mb-2 text-faint" />{st.current ? 'No findings — looks clean.' : 'Run a review of your working tree, staged changes, a branch diff or a commit.'}<div className="text-[11px] text-faint mt-1">Findings link to code and can be fixed in chat.</div></div>
        )}
      </div>
    </div>
  )
}

function BaseInput() {
  const base = useReview((s) => s.base); const set = useReview((s) => s.set)
  useEffect(() => { if (!base) api.git.defaultBranch().then((b) => { if (b) set({ base: b }) }).catch(() => {}) }, [base, set])
  return <input value={base} onChange={(e) => set({ base: e.target.value })} placeholder="main" className="h-6 w-24 px-1 rounded border border-border bg-input text-[11.5px] mono" />
}

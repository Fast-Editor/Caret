import { useState } from 'react'
import { ExternalLink, WandSparkles, LoaderCircle } from 'lucide-react'
import { Chevron, Collapse } from '@/components/chat/Collapse'
import type { VerifyFinding } from '@/types'
import { runCommand, hasCommand } from '@/lib/commands'
import { useReview } from '@/store/review'
import { Markdown, CodeBlock } from '@/lib/markdown'
import { api } from '@/lib/api'

export const SEV_CLS: Record<string, string> = { critical: 'bg-danger text-inverse', high: 'bg-danger/20 text-danger', medium: 'bg-warning/20 text-warning', low: 'bg-info/20 text-info', info: 'bg-input text-muted' }

export function FindingRow({ f, reportId, compact }: { f: VerifyFinding; reportId: string; compact?: boolean }) {
  const [open, setOpen] = useState(!compact && (f.severity === 'critical' || f.severity === 'high'))
  const [applying, setApplying] = useState(false)
  const applyFix = useReview((s) => s.applyFix)
  const openFile = () => { if (f.path && hasCommand('editor.openFile')) void runCommand('editor.openFile', { path: f.path, line: f.line, endLine: f.endLine }) }
  const doFix = async () => {
    setApplying(true)
    try {
      const pc = await applyFix(reportId, f.id)
      if (pc && hasCommand('editor.openDiff')) void runCommand('editor.openDiff', { path: pc.path, original: pc.before ?? '', modified: pc.after ?? '', title: `fix: ${pc.path.split('/').pop()}`, changeId: pc.changeId, sessionId: pc.sessionId })
      else if (!pc) void runCommand('app.notify', { level: 'warn', title: 'No automatic fix available', message: f.suggestion ? 'See the suggestion and apply it manually or ask the agent.' : undefined })
    } finally { setApplying(false) }
  }
  const askAgent = () => void runCommand('chat.sendPrompt', { text: `Fix this verification finding${f.path ? ` in ${f.path.split('/').pop()}${f.line ? `:${f.line}` : ''}` : ''}: ${f.message}${f.rule ? ` (${f.rule})` : ''}${f.suggestion ? `\n\nSuggested fix:\n\`\`\`\n${f.suggestion}\n\`\`\`` : ''}`, context: f.path ? [{ id: `vf_${f.id}`, kind: 'file', label: f.path.split('/').pop() ?? f.path, path: f.path, range: f.line ? { startLine: Math.max(1, f.line - 25), endLine: (f.endLine ?? f.line) + 25 } : undefined }] : [], mode: 'agent', submit: true })
  const loc = f.path ? `${f.path.split('/').slice(-2).join('/')}${f.line ? `:${f.line}` : ''}` : ''
  return (
    <div className="border-t border-border/60 first:border-t-0 animate-fade" data-finding-severity={f.severity}>
      <div className="group/vf flex items-start gap-1.5 px-2 py-1 hover:bg-hover">
        <button type="button" onClick={() => setOpen((o) => !o)} className="mt-0.5 text-faint shrink-0" aria-expanded={open}><Chevron open={open} size={11} /></button>
        <span className={`mt-0.5 px-1 rounded text-[9px] uppercase font-semibold shrink-0 ${SEV_CLS[f.severity] ?? SEV_CLS.info}`}>{f.severity}</span>
        <div className="flex-1 min-w-0">
          <div className="text-[11.5px] leading-snug break-words">{f.message}</div>
          {(loc || f.rule) && <button type="button" onClick={openFile} className="text-[10.5px] text-faint mono hover:underline truncate-1 max-w-full text-left" title={f.path}>{loc}{f.rule ? ` · ${f.rule}` : ''}</button>}
        </div>
        <div className="flex items-center gap-1 opacity-0 group-hover/vf:opacity-100 shrink-0">
          {f.path && <button type="button" onClick={openFile} title="Open" className="w-5 h-5 flex items-center justify-center rounded hover:bg-active text-muted"><ExternalLink size={11} /></button>}
          {f.fixable && <button type="button" onClick={() => void doFix()} title="Apply fix" disabled={applying} className="flex items-center gap-1 px-1.5 h-5 rounded border border-border hover:bg-active text-[10.5px] press">{applying ? <LoaderCircle size={10} className="spin" /> : <WandSparkles size={10} />} Apply fix</button>}
          {!f.fixable && <button type="button" onClick={askAgent} title="Fix in chat" className="flex items-center gap-1 px-1.5 h-5 rounded border border-border hover:bg-active text-[10.5px] press"><WandSparkles size={10} /> Fix</button>}
        </div>
      </div>
      <Collapse open={open}>
        {(f.suggestion || f.evidence || f.url) ? <div className="pl-7 pr-2 pb-1.5 text-[11.5px]">
          {f.suggestion && <CodeBlock code={f.suggestion} className={`language-${f.path?.split('.').pop() ?? 'text'}${f.path ? `:${f.path}` : ''}`} compact />}
          {f.evidence && <pre className="mono text-[10.5px] text-muted whitespace-pre-wrap max-h-32 overflow-auto selectable">{f.evidence}</pre>}
          {f.url && <a className="text-accent hover:underline text-[11px]" href={f.url} onClick={(e) => { e.preventDefault(); void api.app.openExternal(f.url!) }}>Reference</a>}
        </div> : <div className="pl-7 pr-2 pb-1.5 text-[11px] text-faint"><Markdown text={f.message} compact /></div>}
      </Collapse>
    </div>
  )
}

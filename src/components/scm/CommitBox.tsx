import { useRef, useState, type KeyboardEvent } from 'react'
import { Check, ChevronDown, Sparkles } from 'lucide-react'
import { useGit } from '@/store/git'
import { api } from '@/lib/api'
import { canCommit, countChanges } from './helpers'
import { Button, Menu, Tooltip, notify, errorMessage, cn } from '@/components/settings/primitives'

export function CommitBox() {
  const { status, commitSummary, commitBody, amend, signoff, committing, generating, setCommitMessage, setAmend, setSignoff, commit } = useGit()
  const [showBody, setShowBody] = useState(false)
  const [commitAll, setCommitAll] = useState(false)
  const summaryRef = useRef<HTMLTextAreaElement>(null)
  const counts = countChanges(status)
  const ok = canCommit(status, commitSummary, amend, commitAll)

  const doCommit = async (push = false) => {
    try {
      const sha = await commit({ push, all: commitAll })
      if (sha) notify('success', push ? 'Committed and pushed' : 'Committed', sha.slice(0, 7))
    } catch (e) { notify('error', 'Commit failed', errorMessage(e)) }
  }
  const generate = async () => {
    useGit.setState({ generating: true })
    try {
      const r = await api.ai.commitMessage({ style: 'conventional', includeBody: true })
      setCommitMessage(r.summary, r.body)
      if (r.body) setShowBody(true)
      summaryRef.current?.focus()
    } catch (e) { notify('error', 'Could not generate commit message', errorMessage(e)) } finally { useGit.setState({ generating: false }) }
  }
  const onKey = (e: KeyboardEvent) => { if ((e.metaKey || e.ctrlKey) && e.key === 'Enter' && ok) { e.preventDefault(); void doCommit(false) } }
  const summaryLen = commitSummary.length

  return (
    <div className="px-2 pt-2 pb-1.5 border-b border-border">
      <div className="relative">
        <textarea ref={summaryRef} value={commitSummary} onChange={(e) => setCommitMessage(e.target.value.replace(/\n/g, ' '), commitBody)} onKeyDown={onKey} rows={1} placeholder={amend ? 'Amend last commit (message optional)' : `Message (⌘↩ to commit${status?.branch ? ` on "${status.branch}"` : ''})`} aria-label="Commit summary"
          className="w-full resize-none rounded-[var(--radius)] border border-border bg-input pl-2 pr-8 py-1.5 text-xs outline-none focus:border-accent placeholder:text-faint leading-snug" />
        <Tooltip label={generating ? 'Generating…' : 'Generate commit message with AI'}>
          <button onClick={generate} disabled={generating || !(counts.staged || counts.total)} aria-label="Generate commit message" className={cn('absolute right-1.5 top-1.5 h-5 w-5 rounded inline-flex items-center justify-center text-muted hover:text-accent hover:bg-hover disabled:opacity-40', generating && 'text-accent pulse')}><Sparkles size={13} /></button>
        </Tooltip>
        {summaryLen > 50 && <span className={cn('absolute right-8 top-1.5 text-[10px]', summaryLen > 72 ? 'text-warning' : 'text-faint')}>{summaryLen}</span>}
      </div>
      {showBody ? (
        <textarea value={commitBody} onChange={(e) => setCommitMessage(commitSummary, e.target.value)} onKeyDown={onKey} rows={3} placeholder="Description (optional)" aria-label="Commit description"
          className="mt-1 w-full resize-y rounded-[var(--radius)] border border-border bg-input px-2 py-1.5 text-xs outline-none focus:border-accent placeholder:text-faint leading-snug" />
      ) : (
        <button onClick={() => setShowBody(true)} className="mt-0.5 text-[10px] text-faint hover:text-muted">+ description</button>
      )}
      <div className="mt-1.5 flex items-center gap-1">
        <div className="flex-1 inline-flex rounded-[var(--radius)] overflow-hidden border border-transparent">
          <Button variant="primary" className="flex-1 rounded-r-none justify-center" disabled={!ok} loading={committing} onClick={() => doCommit(false)}>
            <Check size={12} />{amend ? 'Amend' : commitAll ? `Commit all (${counts.total})` : counts.staged ? `Commit ${counts.staged} file${counts.staged === 1 ? '' : 's'}` : 'Commit'}
          </Button>
          <Menu align="end" trigger={<button aria-label="More commit actions" className="h-7 w-6 bg-accent text-accent-fg border-l border-accent-fg/20 hover:bg-accent-hover inline-flex items-center justify-center rounded-r-[var(--radius)]"><ChevronDown size={12} /></button>}
            items={[
              { id: 'push', label: 'Commit & Push', disabled: !ok, onSelect: () => void doCommit(true) },
              { id: 'all', label: `${commitAll ? '✓ ' : ''}Commit all changes (stage everything)`, onSelect: () => setCommitAll(!commitAll) },
              'sep',
              { id: 'amend', label: `${amend ? '✓ ' : ''}Amend last commit`, onSelect: () => setAmend(!amend) },
              { id: 'signoff', label: `${signoff ? '✓ ' : ''}Add Signed-off-by`, onSelect: () => setSignoff(!signoff) },
            ]} />
        </div>
      </div>
      {(amend || signoff || commitAll) && <div className="mt-1 flex gap-1 text-[10px] text-muted">{amend && <span className="px-1 rounded bg-hover">amend</span>}{signoff && <span className="px-1 rounded bg-hover">sign-off</span>}{commitAll && <span className="px-1 rounded bg-hover">all</span>}</div>}
      {counts.conflicted > 0 && <div className="mt-1 text-[10px] text-danger">Resolve merge conflicts before committing.</div>}
    </div>
  )
}

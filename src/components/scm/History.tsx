import { useEffect, useState } from 'react'
import { ChevronRight, Copy, GitCommitHorizontal } from 'lucide-react'
import { useGit } from '@/store/git'
import { useWorkspace } from '@/store/workspace'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import type { GitCommit, GitCommitFile } from '@/types'
import { initials, hueFor, STATUS_CLASS, STATUS_LETTER, splitPath } from './helpers'
import { Button, Dialog, IconButton, Spinner, relativeTime, notify, errorMessage, cn } from '@/components/settings/primitives'

export function History() {
  const { log, logHasMore, loadingLog, loadLog, status } = useGit()
  const [open, setOpen] = useState(true)
  const [detail, setDetail] = useState<GitCommit | null>(null)
  useEffect(() => { if (open && status?.isRepo && log.length === 0) void loadLog(true) }, [open, status?.isRepo, log.length, loadLog])
  if (!status?.isRepo) return null
  return (
    <div className="border-t border-border">
      <button onClick={() => setOpen(!open)} aria-expanded={open} className="w-full flex items-center gap-1 h-6 px-1 text-[11px] font-semibold text-muted hover:bg-hover/50"><ChevronRight size={12} className="caret-chevron" data-open={open} />History</button>
      {open && (
        <div className="pb-1">
          {log.length === 0 && !loadingLog && <div className="px-3 py-3 text-xs text-muted">No commits yet</div>}
          {log.map((c) => <CommitRow key={c.sha} c={c} onClick={() => setDetail(c)} />)}
          {loadingLog && <div className="px-3 py-2 flex items-center gap-2 text-xs text-muted"><Spinner />Loading…</div>}
          {logHasMore && log.length > 0 && !loadingLog && <div className="px-3 py-1"><Button size="sm" variant="ghost" onClick={() => void loadLog()}>Load more</Button></div>}
        </div>
      )}
      {detail && <CommitDetails c={detail} onClose={() => setDetail(null)} />}
    </div>
  )
}

function Avatar({ name }: { name: string }) {
  return <span style={{ background: `hsl(${hueFor(name)} 45% 35%)` }} className="h-5 w-5 shrink-0 rounded-full inline-flex items-center justify-center text-[9px] font-semibold text-white">{initials(name)}</span>
}

function CommitRow({ c, onClick }: { c: GitCommit; onClick: () => void }) {
  return (
    <button onClick={onClick} className="w-full flex items-center gap-2 h-[26px] px-2 text-xs hover:bg-hover/60 text-left" title={`${c.sha}\n${c.summary}\n${c.author} · ${new Date(c.time > 1e12 ? c.time : c.time * 1000).toLocaleString()}`}>
      <Avatar name={c.author} />
      <span className="truncate-1 flex-1 min-w-0">{c.summary}</span>
      {c.refs.slice(0, 2).map((r) => <span key={r} className="px-1 rounded border border-border bg-hover text-[9px] mono text-muted truncate-1 max-w-[90px]">{r.replace('refs/heads/', '').replace('refs/remotes/', '')}</span>)}
      <span className="text-[10px] text-faint shrink-0">{relativeTime(c.time)}</span>
    </button>
  )
}

export function CommitDetails({ c, onClose }: { c: GitCommit; onClose: () => void }) {
  const [files, setFiles] = useState<GitCommitFile[] | null>(null)
  const root = useWorkspace((s) => s.workspace?.root)
  useEffect(() => { setFiles(null); api.git.commitFiles(c.sha).then(setFiles).catch((e) => { notify('error', 'Could not load commit', errorMessage(e)); setFiles([]) }) }, [c.sha])
  const openDiff = async (f: GitCommitFile) => {
    const parent = c.parents[0]
    let original = ''; let modified = ''
    if (parent && f.status !== 'added') { try { original = await api.git.showFile(f.path, parent) } catch { original = '' } }
    if (f.status !== 'deleted') { try { modified = await api.git.showFile(f.path, c.sha) } catch { modified = '' } }
    await runCommand('editor.openDiff', { path: root ? `${root}/${f.path}` : f.path, original, modified, title: `${f.path} (${c.shortSha})` })
  }
  const adds = files?.reduce((n, f) => n + f.additions, 0) ?? 0
  const dels = files?.reduce((n, f) => n + f.deletions, 0) ?? 0
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose() }} title={<span className="flex items-center gap-2"><GitCommitHorizontal size={14} />{c.summary}</span>} width={640}
      description={<span className="flex items-center gap-2 flex-wrap"><Avatar name={c.author} />{c.author} · {relativeTime(c.time)} · <span className="mono">{c.shortSha}</span><IconButton title="Copy SHA" onClick={() => { void navigator.clipboard?.writeText(c.sha); notify('success', 'SHA copied') }}><Copy size={11} /></IconButton>{c.refs.map((r) => <span key={r} className="px-1 rounded border border-border bg-hover text-[10px] mono">{r.replace('refs/heads/', '').replace('refs/remotes/', '')}</span>)}</span>}>
      {c.body && <pre className="mono text-[11px] whitespace-pre-wrap text-muted mb-3 selectable">{c.body}</pre>}
      <div className="text-[11px] text-muted mb-1">{files ? `${files.length} file${files.length === 1 ? '' : 's'} changed` : 'Loading files…'}{files && <span className="ml-2"><span className="text-success">+{adds}</span> <span className="text-danger">−{dels}</span></span>}</div>
      <div className="rounded border border-border divide-y divide-border max-h-[46vh] overflow-auto">
        {files?.map((f) => { const { dir, base } = splitPath(f.path); return (
          <button key={f.path} onClick={() => void openDiff(f)} className="w-full flex items-center gap-2 h-7 px-2 text-xs hover:bg-hover text-left">
            <span className={cn('w-3 font-semibold', STATUS_CLASS[f.status])}>{STATUS_LETTER[f.status]}</span>
            <span className="truncate-1 flex-1 min-w-0">{base}<span className="text-faint ml-1.5">{dir}</span></span>
            <span className="text-[10px] mono"><span className="text-success">+{f.additions}</span> <span className="text-danger">−{f.deletions}</span></span>
          </button>) })}
        {files && files.length === 0 && <div className="px-2 py-3 text-xs text-muted">No file changes (merge or empty commit).</div>}
      </div>
    </Dialog>
  )
}

import { useEffect, useMemo, useState } from 'react'
import { BrushCleaning, ChevronRight, FolderGit2, Plus, SquareTerminal, Trash2, Lock } from 'lucide-react'
import { useGit } from '@/store/git'
import { useWorkspace } from '@/store/workspace'
import { useChat } from '@/store/chat'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import type { WorktreeCleanupCandidate } from '@/types'
import { Badge, Button, Dialog, Field, IconButton, Spinner, TextInput, Toggle, confirmDialog, notify, errorMessage } from '@/components/settings/primitives'
import { isValidBranchName } from './helpers'

const samePath = (a: string, b: string) => { const n = (p: string) => p.replace(/\/+$/, ''); const x = n(a); const y = n(b); return x === y || x.endsWith(`/${y}`) || y.endsWith(`/${x}`) }

export function Worktrees() {
  const { worktrees, loadWorktrees, worktreeRemove, status } = useGit()
  const ws = useWorkspace((s) => s.workspace)
  const sessions = useChat((s) => s.sessions)
  const [open, setOpen] = useState(false)
  const [add, setAdd] = useState(false)
  const [cleanup, setCleanup] = useState(false)
  useEffect(() => { if (open) void loadWorktrees() }, [open, loadWorktrees])
  const agentOf = useMemo(() => (path: string) => sessions.find((s) => s.worktreePath && samePath(s.worktreePath, path)), [sessions])
  if (!status?.isRepo) return null
  const remove = async (path: string) => {
    if (!(await confirmDialog({ title: 'Remove worktree?', message: path, danger: true, confirmLabel: 'Remove' }))) return
    try { await worktreeRemove(path) } catch (e) {
      if (await confirmDialog({ title: 'Worktree has changes', message: `${errorMessage(e)}\n\nForce remove?`, danger: true, confirmLabel: 'Force remove' })) { try { await worktreeRemove(path, true) } catch (e2) { notify('error', 'Remove failed', errorMessage(e2)) } }
    }
  }
  return (
    <div className="border-t border-border">
      <div className="group flex items-center h-6 px-1 text-[11px] font-semibold text-muted hover:bg-hover/50 transition-base">
        <button onClick={() => setOpen(!open)} aria-expanded={open} className="flex items-center gap-1 flex-1 h-full text-left"><ChevronRight size={12} className="caret-chevron" data-open={open} />Worktrees{worktrees.length > 1 && <span className="ml-1 px-1 rounded-full bg-hover text-[10px] font-normal">{worktrees.length}</span>}</button>
        <div className="hidden group-hover:flex"><IconButton title="Clean up agent worktrees" onClick={() => setCleanup(true)}><BrushCleaning size={12} /></IconButton><IconButton title="Add worktree" onClick={() => setAdd(true)}><Plus size={12} /></IconButton></div>
      </div>
      {open && (
        <div className="pb-1">
          {worktrees.map((w) => { const agent = w.isMain ? undefined : agentOf(w.path); return (
            <div key={w.path} className="group flex items-center gap-2 h-[26px] px-2 text-xs hover:bg-hover/60" title={agent ? `${agent.title}\n${w.branch ?? ''}\n${w.path}` : w.path}>
              <FolderGit2 size={13} className={agent ? 'text-accent shrink-0' : 'text-muted shrink-0'} />
              {agent ? (
                <span className="truncate-1 flex-1 min-w-0 flex items-center gap-1.5"><span className="truncate-1">{agent.title}</span>{w.branch && <span className="mono text-[10.5px] px-1 rounded bg-input text-muted truncate-1 max-w-[45%]">{w.branch}</span>}</span>
              ) : (
                <span className="truncate-1 flex-1 min-w-0">{w.branch ?? w.head.slice(0, 7)}<span className="text-faint ml-1.5 text-[11px]">{w.path.replace(ws?.root ?? '', '.') }</span></span>
              )}
              {agent && <span className="text-[9px] px-1 rounded bg-accent/15 text-accent">agent</span>}
              {w.isMain && <span className="text-[9px] px-1 rounded bg-hover text-muted">main</span>}
              {w.isLocked && <Lock size={11} className="text-faint" />}
              <div className="hidden group-hover:flex gap-0.5">
                {agent && <IconButton title="Open thread" onClick={() => void runCommand('chat.open', { sessionId: agent.id })}><ChevronRight size={11} /></IconButton>}
                <IconButton title="Open terminal here" onClick={() => void runCommand('terminal.new', { cwd: w.path })}><SquareTerminal size={11} /></IconButton>
                {!w.isMain && <IconButton title="Remove worktree" onClick={() => void remove(w.path)}><Trash2 size={11} /></IconButton>}
              </div>
            </div>
          ) })}
          {worktrees.length === 0 && <div className="px-3 py-2 text-xs text-muted">No worktrees</div>}
          <button type="button" onClick={() => setCleanup(true)} className="mx-2 mt-1 flex items-center gap-1.5 h-6 px-2 rounded border border-border text-[11px] text-muted hover:bg-hover hover:text-fg press"><BrushCleaning size={11} />Clean up agent worktrees</button>
        </div>
      )}
      {add && <AddWorktreeDialog onClose={() => setAdd(false)} />}
      {cleanup && <CleanupDialog onClose={() => setCleanup(false)} onDone={() => { setCleanup(false); void loadWorktrees(); void useGit.getState().loadBranches() }} />}
    </div>
  )
}

/** Preview of agent worktrees that match the cleanup policies (Settings > Agent > Worktrees), with checkboxes. */
function CleanupDialog({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const [items, setItems] = useState<WorktreeCleanupCandidate[] | null>(null)
  const [picked, setPicked] = useState<Set<string>>(new Set())
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    api.chat.worktreeCleanupPreview().then((list) => { setItems(list); setPicked(new Set(list.filter((c) => !c.dirty).map((c) => c.sessionId))) }).catch((e) => { notify('error', 'Could not preview cleanup', errorMessage(e)); setItems([]) })
  }, [])
  const toggle = (id: string) => setPicked((p) => { const n = new Set(p); if (n.has(id)) n.delete(id); else n.add(id); return n })
  const remove = async () => {
    setBusy(true)
    try {
      const rep = await api.chat.worktreeCleanup([...picked])
      const skipped = rep.skipped.map((s) => { const c = items?.find((x) => x.sessionId === s.sessionId); return `${c?.title ?? s.sessionId}: ${s.reason}` })
      notify(rep.removed.length ? 'success' : 'info', `Removed ${rep.removed.length} worktree${rep.removed.length === 1 ? '' : 's'}`, [...skipped.map((s) => `skipped ${s}`), ...rep.notes].join('\n') || undefined)
      onDone()
    } catch (e) { notify('error', 'Cleanup failed', errorMessage(e)) } finally { setBusy(false) }
  }
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose() }} title="Clean up agent worktrees" description="Worktrees of threads that match your cleanup policies (inactive, archived, merged or without commits). Dirty worktrees follow the “on local changes” setting." width={640}
      footer={<><Button onClick={() => void runCommand('settings.open', { page: 'agent', query: 'worktrees' })}>Policies…</Button><span className="flex-1" /><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={picked.size === 0} loading={busy} onClick={remove}>Remove {picked.size || ''}</Button></>}>
      {items === null && <div className="flex items-center gap-2 py-4 text-xs text-muted"><Spinner /> Checking worktrees…</div>}
      {items && items.length === 0 && <div className="py-4 text-xs text-muted">Nothing to clean up. Every agent worktree is still in use or kept by policy.</div>}
      {items && items.length > 0 && (
        <div className="max-h-[320px] overflow-auto rounded border border-border divide-y divide-border">
          {items.map((c) => (
            <label key={c.sessionId} className="flex items-start gap-2 px-2 py-1.5 text-xs hover:bg-hover cursor-pointer">
              <input type="checkbox" checked={picked.has(c.sessionId)} onChange={() => toggle(c.sessionId)} className="mt-0.5 accent-[var(--accent)]" />
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-1.5 min-w-0"><span className="truncate-1 font-medium">{c.title}</span>{c.branch && <span className="mono text-[10.5px] text-muted truncate-1">{c.branch}</span>}{c.dirty && <Badge tone="warning">local changes</Badge>}</div>
                <div className="text-[10.5px] text-faint truncate-1" title={c.path}>{c.path}</div>
                <div className="flex flex-wrap gap-1 mt-0.5">{c.reasons.map((r) => <span key={r} className="px-1 h-[16px] inline-flex items-center rounded bg-input text-muted text-[10px]">{r}</span>)}</div>
              </div>
            </label>
          ))}
        </div>
      )}
    </Dialog>
  )
}

function AddWorktreeDialog({ onClose }: { onClose: () => void }) {
  const ws = useWorkspace((s) => s.workspace)
  const worktreeAdd = useGit((s) => s.worktreeAdd)
  const branches = useGit((s) => s.branches)
  const [branch, setBranch] = useState('')
  const [path, setPath] = useState('')
  const [create, setCreate] = useState(true)
  const [busy, setBusy] = useState(false)
  const suggested = ws && branch ? `${ws.root}/.caret/worktrees/${branch.replace(/[^a-zA-Z0-9._-]+/g, '-')}` : ''
  const exists = branches.some((b) => !b.isRemote && b.name === branch)
  const valid = branch.trim() && (create ? isValidBranchName(branch) && !exists : exists)
  const submit = async () => {
    setBusy(true)
    try { await worktreeAdd(path.trim() || suggested, branch.trim(), create); notify('success', 'Worktree created', path.trim() || suggested); onClose() } catch (e) { notify('error', 'Could not add worktree', errorMessage(e)) } finally { setBusy(false) }
  }
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose() }} title="Add worktree" description="Worktrees let agents work on branches in parallel without touching your working copy." footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!valid} loading={busy} onClick={submit}>Create</Button></>}>
      <Field label="Branch" hint={create ? (exists ? 'Branch already exists — turn off "create" to use it.' : 'New branch from current HEAD') : 'Existing local branch'}><TextInput mono value={branch} onChange={setBranch} placeholder="feature/agent-task" autoFocus /></Field>
      <Field label="Create new branch"><div className="h-7 flex items-center"><Toggle checked={create} onChange={setCreate} label="Create branch" /></div></Field>
      <Field label="Path" hint="Defaults to .caret/worktrees/<branch>"><TextInput mono value={path} onChange={setPath} placeholder={suggested} /></Field>
    </Dialog>
  )
}

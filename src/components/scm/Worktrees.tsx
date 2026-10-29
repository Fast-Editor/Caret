import { useEffect, useState } from 'react'
import { ChevronRight, FolderGit2, Plus, SquareTerminal, Trash2, Lock } from 'lucide-react'
import { useGit } from '@/store/git'
import { useWorkspace } from '@/store/workspace'
import { runCommand } from '@/lib/commands'
import { Button, Dialog, Field, IconButton, TextInput, Toggle, confirmDialog, notify, errorMessage } from '@/components/settings/primitives'
import { isValidBranchName } from './helpers'

export function Worktrees() {
  const { worktrees, loadWorktrees, worktreeRemove, status } = useGit()
  const ws = useWorkspace((s) => s.workspace)
  const [open, setOpen] = useState(false)
  const [add, setAdd] = useState(false)
  useEffect(() => { if (open) void loadWorktrees() }, [open, loadWorktrees])
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
        <div className="hidden group-hover:flex"><IconButton title="Add worktree" onClick={() => setAdd(true)}><Plus size={12} /></IconButton></div>
      </div>
      {open && (
        <div className="pb-1">
          {worktrees.map((w) => (
            <div key={w.path} className="group flex items-center gap-2 h-[26px] px-2 text-xs hover:bg-hover/60" title={w.path}>
              <FolderGit2 size={13} className="text-muted shrink-0" />
              <span className="truncate-1 flex-1 min-w-0">{w.branch ?? w.head.slice(0, 7)}<span className="text-faint ml-1.5 text-[11px]">{w.path.replace(ws?.root ?? '', '.') }</span></span>
              {w.isMain && <span className="text-[9px] px-1 rounded bg-hover text-muted">main</span>}
              {w.isLocked && <Lock size={11} className="text-faint" />}
              <div className="hidden group-hover:flex gap-0.5">
                <IconButton title="Open terminal here" onClick={() => void runCommand('terminal.new', { cwd: w.path })}><SquareTerminal size={11} /></IconButton>
                {!w.isMain && <IconButton title="Remove worktree" onClick={() => void remove(w.path)}><Trash2 size={11} /></IconButton>}
              </div>
            </div>
          ))}
          {worktrees.length === 0 && <div className="px-3 py-2 text-xs text-muted">No worktrees</div>}
        </div>
      )}
      {add && <AddWorktreeDialog onClose={() => setAdd(false)} />}
    </div>
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

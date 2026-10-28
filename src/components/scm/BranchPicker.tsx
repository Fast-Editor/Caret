import { useEffect, useMemo, useState } from 'react'
import { Command } from 'cmdk'
import { Check, GitBranch, Plus, Trash2, Cloud } from 'lucide-react'
import { useGit } from '@/store/git'
import { sortBranches, isValidBranchName } from './helpers'
import { Dialog, Button, confirmDialog, notify, errorMessage, relativeTime, cn } from '@/components/settings/primitives'

export function BranchPicker({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const branches = useGit((s) => s.branches)
  const loadBranches = useGit((s) => s.loadBranches)
  const checkout = useGit((s) => s.checkout)
  const createBranch = useGit((s) => s.createBranch)
  const deleteBranch = useGit((s) => s.deleteBranch)
  const [q, setQ] = useState('')
  const [busy, setBusy] = useState(false)
  useEffect(() => { if (open) { setQ(''); void loadBranches() } }, [open, loadBranches])
  const sorted = useMemo(() => sortBranches(branches), [branches])
  const local = sorted.filter((b) => !b.isRemote)
  const remote = sorted.filter((b) => b.isRemote)
  const exact = branches.some((b) => b.name === q.trim())
  const canCreate = q.trim().length > 0 && !exact && isValidBranchName(q.trim())

  const go = async (name: string, isRemote: boolean) => {
    setBusy(true)
    try {
      if (isRemote) {
        const localName = name.replace(/^[^/]+\//, '')
        await checkout(localName, !branches.some((b) => !b.isRemote && b.name === localName))
      } else await checkout(name)
      onOpenChange(false)
    } catch (e) { notify('error', `Checkout failed`, errorMessage(e)) } finally { setBusy(false) }
  }
  const create = async () => {
    setBusy(true)
    try { await createBranch(q.trim(), true); notify('success', `Created branch ${q.trim()}`); onOpenChange(false) } catch (e) { notify('error', 'Create branch failed', errorMessage(e)) } finally { setBusy(false) }
  }
  const del = async (name: string) => {
    if (!(await confirmDialog({ title: `Delete branch “${name}”?`, danger: true, confirmLabel: 'Delete' }))) return
    try { await deleteBranch(name) } catch (e) {
      if (await confirmDialog({ title: 'Branch not fully merged', message: `${errorMessage(e)}\n\nForce delete?`, danger: true, confirmLabel: 'Force delete' })) { try { await deleteBranch(name, true) } catch (e2) { notify('error', 'Delete failed', errorMessage(e2)) } }
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Switch branch" width={520}>
      <Command label="Branches" shouldFilter className="w-full">
        <Command.Input autoFocus value={q} onValueChange={setQ} placeholder="Find or create a branch…" className="h-8 w-full px-2 mb-2 rounded-[var(--radius)] border border-border bg-input text-xs outline-none focus:border-accent" />
        <Command.List className="max-h-[46vh] overflow-auto">
          <Command.Empty className="px-2 py-4 text-xs text-muted text-center">{canCreate ? 'No matching branch' : 'No branches'}</Command.Empty>
          {canCreate && (
            <Command.Group heading={<Heading>Create</Heading>}>
              <Command.Item value={`create:${q}`} onSelect={create} className={itemCls}><Plus size={13} className="text-accent" /><span>Create branch <span className="mono font-medium">{q.trim()}</span> from current HEAD</span></Command.Item>
            </Command.Group>
          )}
          {local.length > 0 && (
            <Command.Group heading={<Heading>Local</Heading>}>
              {local.map((b) => (
                <Command.Item key={b.name} value={b.name} disabled={busy} onSelect={() => { if (!b.isCurrent) void go(b.name, false) }} className={itemCls}>
                  {b.isCurrent ? <Check size={13} className="text-accent" /> : <GitBranch size={13} className="text-muted" />}
                  <span className="mono truncate-1 flex-1">{b.name}</span>
                  {b.upstream && <span className="text-[10px] text-faint truncate-1 max-w-[120px]">{b.upstream}</span>}
                  {b.lastCommitTime && <span className="text-[10px] text-faint">{relativeTime(b.lastCommitTime)}</span>}
                  {!b.isCurrent && <button onClick={(e) => { e.stopPropagation(); void del(b.name) }} aria-label={`Delete ${b.name}`} className="text-faint hover:text-danger"><Trash2 size={12} /></button>}
                </Command.Item>
              ))}
            </Command.Group>
          )}
          {remote.length > 0 && (
            <Command.Group heading={<Heading>Remote</Heading>}>
              {remote.map((b) => (
                <Command.Item key={b.name} value={b.name} disabled={busy} onSelect={() => void go(b.name, true)} className={itemCls}>
                  <Cloud size={13} className="text-muted" /><span className="mono truncate-1 flex-1">{b.name}</span>
                  {b.lastCommitTime && <span className="text-[10px] text-faint">{relativeTime(b.lastCommitTime)}</span>}
                </Command.Item>
              ))}
            </Command.Group>
          )}
        </Command.List>
      </Command>
      <div className="mt-2 flex justify-end"><Button onClick={() => onOpenChange(false)}>Close</Button></div>
    </Dialog>
  )
}
const itemCls = cn('flex items-center gap-2 px-2 h-7 rounded text-xs cursor-pointer select-none data-[selected=true]:bg-hover data-[disabled=true]:opacity-50')
function Heading({ children }: { children: React.ReactNode }) { return <div className="px-2 py-1 text-[10px] uppercase tracking-wide text-faint">{children}</div> }

import { useCallback, useMemo, useState } from 'react'
import { ChevronDown, ChevronRight, FileDiff, Minus, Plus, Undo2, ExternalLink } from 'lucide-react'
import { useGit } from '@/store/git'
import { useWorkspace } from '@/store/workspace'
import { runCommand } from '@/lib/commands'
import { api } from '@/lib/api'
import type { GitFileChange } from '@/types'
import { groupChanges, splitPath, STATUS_CLASS, STATUS_LETTER, STATUS_TITLE, type ChangeSection } from './helpers'
import { IconButton, Tooltip, confirmDialog, notify, errorMessage, cn } from '@/components/settings/primitives'

/** Open a diff tab for a change (HEAD vs working tree, or HEAD vs index for staged). */
export async function openChangeDiff(path: string, staged: boolean, base?: string): Promise<void> {
  const root = useWorkspace.getState().workspace?.root
  const abs = root && !path.startsWith('/') ? `${root}/${path}` : path
  const rel = root && path.startsWith(root) ? path.slice(root.length + 1) : path
  let original = ''
  try { original = await api.git.showFile(rel, base ?? 'HEAD') } catch { original = '' }
  let modified = ''
  if (staged) {
    try { modified = await api.git.showFile(rel, ':0') } catch { try { modified = (await api.fs.readFile(abs)).content } catch { modified = '' } }
  } else {
    try { modified = (await api.fs.readFile(abs)).content } catch { modified = '' }
  }
  await runCommand('editor.openDiff', { path: abs, original, modified, title: `${rel} (${staged ? 'Index' : 'Working Tree'})` })
}

export function ChangesList() {
  const status = useGit((s) => s.status)
  const sections = useMemo(() => groupChanges(status), [status])
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({})
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [anchor, setAnchor] = useState<string | null>(null)
  const allPaths = useMemo(() => sections.flatMap((s) => s.items.map((i) => `${s.id}:${i.path}`)), [sections])

  const onSelect = useCallback((key: string, e: React.MouseEvent) => {
    setSelected((prev) => {
      const next = new Set(e.metaKey || e.ctrlKey ? prev : [])
      if (e.shiftKey && anchor) {
        const a = allPaths.indexOf(anchor), b = allPaths.indexOf(key)
        if (a !== -1 && b !== -1) { for (let i = Math.min(a, b); i <= Math.max(a, b); i++) next.add(allPaths[i]); return next }
      }
      if (next.has(key) && (e.metaKey || e.ctrlKey)) next.delete(key); else next.add(key)
      return next
    })
    if (!e.shiftKey) setAnchor(key)
  }, [anchor, allPaths])

  if (sections.length === 0) return <div className="px-3 py-6 text-center text-xs text-muted">No changes</div>
  return (
    <div className="py-1" onKeyDown={(e) => { if (e.key === 'Escape') setSelected(new Set()) }}>
      {sections.map((s) => <SectionView key={s.id} section={s} collapsed={!!collapsed[s.id]} onToggle={() => setCollapsed({ ...collapsed, [s.id]: !collapsed[s.id] })} selected={selected} onSelect={onSelect} />)}
    </div>
  )
}

function SectionView({ section, collapsed, onToggle, selected, onSelect }: { section: ChangeSection; collapsed: boolean; onToggle: () => void; selected: Set<string>; onSelect: (key: string, e: React.MouseEvent) => void }) {
  const { stage, unstage, discard, stageAll, unstageAll } = useGit()
  const act = async (f: () => Promise<void>, label: string) => { try { await f() } catch (e) { notify('error', `${label} failed`, errorMessage(e)) } }
  const paths = section.items.map((i) => i.path)
  const discardAll = async () => {
    const ok = await confirmDialog({ title: section.id === 'untracked' ? `Delete ${paths.length} untracked file${paths.length === 1 ? '' : 's'}?` : `Discard all changes in ${paths.length} file${paths.length === 1 ? '' : 's'}?`, message: 'This cannot be undone.', danger: true, confirmLabel: 'Discard' })
    if (ok) await act(() => discard(paths), 'Discard')
  }
  return (
    <div>
      <div className="group flex items-center h-6 px-1 text-[11px] font-semibold uppercase tracking-wide text-muted hover:bg-hover/50">
        <button onClick={onToggle} className="flex items-center gap-1 flex-1 min-w-0 h-full text-left" aria-expanded={!collapsed}>{collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}<span className="truncate-1">{section.title}</span><span className="ml-1 px-1 rounded-full bg-hover text-[10px] font-normal">{section.items.length}</span></button>
        <div className="hidden group-hover:flex items-center gap-0.5">
          {section.id === 'staged' ? <IconButton title="Unstage all" onClick={() => act(unstageAll, 'Unstage all')}><Minus size={12} /></IconButton> : <>
            {(section.id === 'unstaged' || section.id === 'untracked') && <IconButton title="Discard all" onClick={discardAll}><Undo2 size={12} /></IconButton>}
            <IconButton title="Stage all" onClick={() => act(section.id === 'conflicted' ? () => stage(paths) : stageAll, 'Stage all')}><Plus size={12} /></IconButton>
          </>}
        </div>
      </div>
      {!collapsed && section.items.map((c) => <ChangeRow key={c.path} c={c} section={section.id} selected={selected.has(`${section.id}:${c.path}`)} onSelect={(e) => onSelect(`${section.id}:${c.path}`, e)} selectedPaths={[...selected].filter((k) => k.startsWith(section.id + ':')).map((k) => k.slice(section.id.length + 1))} />)}
    </div>
  )
}

function ChangeRow({ c, section, selected, onSelect, selectedPaths }: { c: GitFileChange; section: ChangeSection['id']; selected: boolean; onSelect: (e: React.MouseEvent) => void; selectedPaths: string[] }) {
  const { stage, unstage, discard } = useGit()
  const root = useWorkspace((s) => s.workspace?.root)
  const { dir, base } = splitPath(c.path)
  const targets = selected && selectedPaths.length > 1 ? selectedPaths : [c.path]
  const act = async (f: () => Promise<void>, label: string) => { try { await f() } catch (e) { notify('error', `${label} failed`, errorMessage(e)) } }
  const open = () => { if (c.status === 'deleted') return void openChangeDiff(c.path, section === 'staged'); if (section === 'untracked' || c.status === 'added') void runCommand('editor.openFile', { path: root ? `${root}/${c.path}` : c.path, preview: true }); else void openChangeDiff(c.path, section === 'staged') }
  const doDiscard = async () => {
    const n = targets.length
    const ok = await confirmDialog({ title: section === 'untracked' ? `Delete ${n === 1 ? base : `${n} files`}?` : `Discard changes in ${n === 1 ? base : `${n} files`}?`, message: 'This cannot be undone.', danger: true, confirmLabel: section === 'untracked' ? 'Delete' : 'Discard' })
    if (ok) await act(() => discard(targets), 'Discard')
  }
  return (
    <div role="button" tabIndex={0} onClick={(e) => { onSelect(e); if (!(e.metaKey || e.ctrlKey || e.shiftKey)) open() }} onKeyDown={(e) => { if (e.key === 'Enter') open() }}
      className={cn('group flex items-center h-[22px] pl-4 pr-1 text-xs cursor-pointer', selected ? 'bg-selected' : 'hover:bg-hover/60')} title={`${c.path}${c.oldPath ? ` ← ${c.oldPath}` : ''}`}>
      <span className={cn('truncate-1 flex-1 min-w-0', c.status === 'deleted' && 'line-through text-muted')}>{base}<span className="text-faint ml-1.5 text-[11px]">{dir}</span></span>
      <div className="hidden group-hover:flex items-center gap-0.5 mr-1" onClick={(e) => e.stopPropagation()}>
        <IconButton title="Open file" onClick={() => void runCommand('editor.openFile', { path: root ? `${root}/${c.path}` : c.path })}><ExternalLink size={11} /></IconButton>
        {section !== 'untracked' && c.status !== 'deleted' && <IconButton title="Open diff" onClick={() => void openChangeDiff(c.path, section === 'staged')}><FileDiff size={11} /></IconButton>}
        {section === 'staged' ? <IconButton title="Unstage" onClick={() => act(() => unstage(targets), 'Unstage')}><Minus size={11} /></IconButton> : <>
          {section !== 'conflicted' && <IconButton title={section === 'untracked' ? 'Delete file' : 'Discard changes'} onClick={doDiscard}><Undo2 size={11} /></IconButton>}
          <IconButton title="Stage" onClick={() => act(() => stage(targets), 'Stage')}><Plus size={11} /></IconButton>
        </>}
      </div>
      <Tooltip label={STATUS_TITLE[c.status]}><span className={cn('w-4 text-center font-semibold', STATUS_CLASS[c.status])}>{STATUS_LETTER[c.status]}</span></Tooltip>
    </div>
  )
}

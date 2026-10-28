import { useMemo, useState } from 'react'
import { Popover, DropdownMenu } from 'radix-ui'
import { Archive, Copy, Download, Ellipsis, FolderGit2, History, LoaderCircle, Pencil, Pin, Search, Trash2 } from 'lucide-react'
import type { ChatSessionSummary } from '@/types'
import { useChat } from '@/store/chat'
import { dayGroup, fmtRelative } from './util'
import { runCommand } from '@/lib/commands'

const itemCls = 'flex items-center gap-2 px-2 h-7 rounded text-[12px] outline-none data-[highlighted]:bg-selected cursor-pointer'

export function SessionMenu({ s, onRename }: { s: ChatSessionSummary; onRename: () => void }) {
  const { pinSession, archiveSession, deleteSession, duplicateSession, exportSession } = useChat.getState()
  const exportAs = async (format: 'markdown' | 'json') => {
    const content = await exportSession(s.id, format)
    try { const { save } = await import('@tauri-apps/plugin-dialog'); const { api } = await import('@/lib/api'); const path = await save({ defaultPath: `${s.title.replace(/[^\w.-]+/g, '_')}.${format === 'markdown' ? 'md' : 'json'}` }); if (path) { await api.fs.writeFile(path, content); void runCommand('app.notify', { level: 'success', title: 'Exported', message: path }) } }
    catch (e) { void runCommand('app.notify', { level: 'error', title: 'Export failed', message: String(e) }) }
  }
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild><button type="button" onClick={(e) => e.stopPropagation()} title="More" className="w-5 h-5 flex items-center justify-center rounded hover:bg-hover text-muted"><Ellipsis size={13} /></button></DropdownMenu.Trigger>
      <DropdownMenu.Portal><DropdownMenu.Content align="end" sideOffset={4} className="z-50 min-w-[180px] rounded-md border border-border bg-tooltip p-1 shadow-[var(--shadow)] text-fg" onClick={(e) => e.stopPropagation()}>
        <DropdownMenu.Item className={itemCls} onSelect={onRename}><Pencil size={12} /> Rename</DropdownMenu.Item>
        <DropdownMenu.Item className={itemCls} onSelect={() => void pinSession(s.id, !s.pinned)}><Pin size={12} /> {s.pinned ? 'Unpin' : 'Pin'}</DropdownMenu.Item>
        <DropdownMenu.Item className={itemCls} onSelect={() => void duplicateSession(s.id)}><Copy size={12} /> Duplicate</DropdownMenu.Item>
        <DropdownMenu.Sub><DropdownMenu.SubTrigger className={itemCls}><Download size={12} /> Export</DropdownMenu.SubTrigger><DropdownMenu.Portal><DropdownMenu.SubContent className="z-50 min-w-[140px] rounded-md border border-border bg-tooltip p-1 shadow-[var(--shadow)]"><DropdownMenu.Item className={itemCls} onSelect={() => void exportAs('markdown')}>Markdown</DropdownMenu.Item><DropdownMenu.Item className={itemCls} onSelect={() => void exportAs('json')}>JSON</DropdownMenu.Item></DropdownMenu.SubContent></DropdownMenu.Portal></DropdownMenu.Sub>
        {s.worktreePath && <DropdownMenu.Item className={itemCls} onSelect={() => void runCommand('terminal.new', { cwd: s.worktreePath })}><FolderGit2 size={12} /> Open worktree in terminal</DropdownMenu.Item>}
        <DropdownMenu.Item className={itemCls} onSelect={() => void archiveSession(s.id, !s.archived)}><Archive size={12} /> {s.archived ? 'Unarchive' : 'Archive'}</DropdownMenu.Item>
        <DropdownMenu.Separator className="my-1 h-px bg-border" />
        <DropdownMenu.Item className={`${itemCls} text-danger`} onSelect={() => { if (window.confirm(`Delete "${s.title}"?`)) void deleteSession(s.id) }}><Trash2 size={12} /> Delete</DropdownMenu.Item>
      </DropdownMenu.Content></DropdownMenu.Portal>
    </DropdownMenu.Root>
  )
}

export function SessionRow({ s, active, onSelect, compact }: { s: ChatSessionSummary; active: boolean; onSelect: () => void; compact?: boolean }) {
  const [renaming, setRenaming] = useState(false)
  const [title, setTitle] = useState(s.title)
  const rename = useChat((st) => st.renameSession)
  const busy = s.status === 'running' || s.status === 'summarizing'
  return (
    <div onClick={onSelect} className={`group/row flex items-start gap-2 px-2 py-1.5 rounded cursor-pointer ${active ? 'bg-selected' : 'hover:bg-hover'}`}>
      <span className={`mt-1.5 w-1.5 h-1.5 rounded-full shrink-0 ${busy ? 'bg-accent pulse' : s.status === 'awaiting-approval' || s.status === 'awaiting-input' ? 'bg-warning' : s.status === 'error' ? 'bg-danger' : 'bg-border-strong'}`} title={s.status} />
      <div className="flex-1 min-w-0">
        {renaming ? (
          <input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} onClick={(e) => e.stopPropagation()} onBlur={() => { setRenaming(false); if (title.trim() && title !== s.title) void rename(s.id, title.trim()) }} onKeyDown={(e) => { if (e.key === 'Enter') (e.target as HTMLInputElement).blur(); if (e.key === 'Escape') { setTitle(s.title); setRenaming(false) } }} className="w-full h-5 px-1 bg-input border border-border rounded text-[12px] outline-none" />
        ) : (
          <div className="flex items-center gap-1 text-[12px] truncate-1">{s.pinned && <Pin size={10} className="text-accent shrink-0" />}<span className="truncate-1">{s.title || 'New chat'}</span>{s.isSubagent && <span className="text-[10px] text-faint">sub</span>}</div>
        )}
        {!compact && <div className="text-[10.5px] text-faint truncate-1">{s.lastMessagePreview || 'No messages yet'}</div>}
        <div className="flex items-center gap-1.5 text-[10px] text-faint mt-0.5">
          <span>{fmtRelative(s.updatedAt)}</span><span>·</span><span className="capitalize">{s.mode}</span>
          {s.pendingChangeCount > 0 && <span className="px-1 rounded bg-warning/15 text-warning">{s.pendingChangeCount} pending</span>}
          {s.worktreePath && <span className="flex items-center gap-0.5"><FolderGit2 size={9} /> worktree</span>}
          {busy && <LoaderCircle size={9} className="spin" />}
        </div>
      </div>
      <div className="opacity-0 group-hover/row:opacity-100"><SessionMenu s={s} onRename={() => { setTitle(s.title); setRenaming(true) }} /></div>
    </div>
  )
}

export function HistoryPopover({ children }: { children: React.ReactNode }) {
  const open = useChat((s) => s.historyOpen)
  const setOpen = useChat((s) => s.setHistoryOpen)
  const sessions = useChat((s) => s.sessions)
  const active = useChat((s) => s.activeSessionId)
  const select = useChat((s) => s.selectSession)
  const [q, setQ] = useState('')
  const [showArchived, setShowArchived] = useState(false)
  const grouped = useMemo(() => {
    const list = sessions.filter((s) => !s.isSubagent && (showArchived ? true : !s.archived) && (!q || s.title.toLowerCase().includes(q.toLowerCase()) || s.lastMessagePreview.toLowerCase().includes(q.toLowerCase())))
    const pinned = list.filter((s) => s.pinned)
    const rest = list.filter((s) => !s.pinned)
    const groups: Record<string, ChatSessionSummary[]> = {}
    for (const s of rest) (groups[dayGroup(s.updatedAt)] ??= []).push(s)
    return { pinned, groups }
  }, [sessions, q, showArchived])
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild>{children}</Popover.Trigger>
      <Popover.Portal>
        <Popover.Content align="end" sideOffset={6} className="z-40 w-[340px] rounded-md border border-border bg-tooltip shadow-[var(--shadow)] text-fg">
          <div className="flex items-center gap-1.5 px-2 h-8 border-b border-border"><History size={12} className="text-faint" /><Search size={12} className="text-faint" /><input autoFocus value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search chats…" className="flex-1 bg-transparent outline-none text-[12px]" /><button type="button" onClick={() => setShowArchived((a) => !a)} className={`text-[10.5px] px-1 rounded ${showArchived ? 'bg-selected' : 'text-faint hover:text-fg'}`}>archived</button></div>
          <div className="max-h-[60vh] overflow-y-auto p-1">
            {grouped.pinned.length > 0 && <><div className="px-2 pt-1 pb-0.5 text-[10.5px] text-faint">Pinned</div>{grouped.pinned.map((s) => <SessionRow key={s.id} s={s} active={s.id === active} onSelect={() => { void select(s.id); setOpen(false) }} />)}</>}
            {(['Today', 'Yesterday', 'This week', 'Earlier'] as const).map((g) => grouped.groups[g]?.length ? <div key={g}><div className="px-2 pt-1 pb-0.5 text-[10.5px] text-faint">{g}</div>{grouped.groups[g].map((s) => <SessionRow key={s.id} s={s} active={s.id === active} onSelect={() => { void select(s.id); setOpen(false) }} />)}</div> : null)}
            {sessions.length === 0 && <div className="px-2 py-4 text-center text-faint text-[11.5px]">No chats yet</div>}
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}

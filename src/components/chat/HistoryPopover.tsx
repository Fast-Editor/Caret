import { useEffect, useMemo, useRef, useState } from 'react'
import { Popover, DropdownMenu } from 'radix-ui'
import { AlarmClock, Archive, BellOff, CheckCheck, ChevronRight, Copy, Download, Ellipsis, FolderGit2, FolderOpen, History, LoaderCircle, Pencil, Pin, Search, Sparkles, Trash2 } from 'lucide-react'
import type { ChatSessionSummary, MessageSearchHit } from '@/types'
import { useChat, isSessionBusy } from '@/store/chat'
import { useUi } from '@/store/ui'
import { useWorkspace } from '@/store/workspace'
import { fmtRelative, fmtWake, groupSessions, groupSessionsByProject, isSnoozed, projectFoldKey, snoozePresets, type ProjectGroup, type ThreadSection } from './util'
import { runCommand } from '@/lib/commands'

const itemCls = 'flex items-center gap-2 px-2 h-7 rounded text-[12px] outline-none data-[highlighted]:bg-selected cursor-pointer'
const subContentCls = 'z-50 min-w-[160px] rounded-md border border-border bg-tooltip p-1 shadow-[var(--shadow)]'
const toggleCls = (on: boolean) => `text-[10.5px] px-1 rounded whitespace-nowrap ${on ? 'bg-selected' : 'text-faint hover:text-fg'}`

export function SessionMenu({ s, onRename }: { s: ChatSessionSummary; onRename: () => void }) {
  const { pinSession, archiveSession, deleteSession, duplicateSession, exportSession, snoozeSession, settleSession, regenerateTitle } = useChat.getState()
  const snoozed = isSnoozed(s)
  const exportAs = async (format: 'markdown' | 'json') => {
    const content = await exportSession(s.id, format)
    try { const { save } = await import('@tauri-apps/plugin-dialog'); const { api } = await import('@/lib/api'); const path = await save({ defaultPath: `${s.title.replace(/[^\w.-]+/g, '_')}.${format === 'markdown' ? 'md' : 'json'}` }); if (path) { await api.fs.writeFile(path, content); void runCommand('app.notify', { level: 'success', title: 'Exported', message: path }) } }
    catch (e) { void runCommand('app.notify', { level: 'error', title: 'Export failed', message: String(e) }) }
  }
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild><button type="button" onClick={(e) => e.stopPropagation()} title="More" className="w-5 h-5 flex items-center justify-center rounded hover:bg-hover text-muted"><Ellipsis size={13} /></button></DropdownMenu.Trigger>
      <DropdownMenu.Portal><DropdownMenu.Content align="end" sideOffset={4} className="z-50 min-w-[190px] rounded-md border border-border bg-tooltip p-1 shadow-[var(--shadow)] text-fg" onClick={(e) => e.stopPropagation()}>
        <DropdownMenu.Item className={itemCls} onSelect={onRename}><Pencil size={12} /> Rename</DropdownMenu.Item>
        <DropdownMenu.Item className={itemCls} onSelect={() => void regenerateTitle(s.id).catch(() => {})}><Sparkles size={12} /> Regenerate title</DropdownMenu.Item>
        <DropdownMenu.Item className={itemCls} onSelect={() => void pinSession(s.id, !s.pinned)}><Pin size={12} /> {s.pinned ? 'Unpin' : 'Pin'}</DropdownMenu.Item>
        <DropdownMenu.Sub>
          <DropdownMenu.SubTrigger className={itemCls}><AlarmClock size={12} /> {snoozed ? `Snoozed (${fmtWake(s.snoozedUntil ?? 0)})` : 'Snooze'}</DropdownMenu.SubTrigger>
          <DropdownMenu.Portal><DropdownMenu.SubContent className={subContentCls}>
            {snoozePresets().map((p) => <DropdownMenu.Item key={p.id} className={itemCls} onSelect={() => void snoozeSession(s.id, p.until)}>{p.label}</DropdownMenu.Item>)}
            {snoozed && <><DropdownMenu.Separator className="my-1 h-px bg-border" /><DropdownMenu.Item className={itemCls} onSelect={() => void snoozeSession(s.id, null)}><BellOff size={12} /> Unsnooze</DropdownMenu.Item></>}
          </DropdownMenu.SubContent></DropdownMenu.Portal>
        </DropdownMenu.Sub>
        <DropdownMenu.Item className={itemCls} onSelect={() => void settleSession(s.id, !s.settled)}><CheckCheck size={12} /> {s.settled ? 'Unsettle' : 'Settle'}</DropdownMenu.Item>
        <DropdownMenu.Item className={itemCls} onSelect={() => void duplicateSession(s.id)}><Copy size={12} /> Duplicate</DropdownMenu.Item>
        <DropdownMenu.Sub><DropdownMenu.SubTrigger className={itemCls}><Download size={12} /> Export</DropdownMenu.SubTrigger><DropdownMenu.Portal><DropdownMenu.SubContent className={subContentCls}><DropdownMenu.Item className={itemCls} onSelect={() => void exportAs('markdown')}>Markdown</DropdownMenu.Item><DropdownMenu.Item className={itemCls} onSelect={() => void exportAs('json')}>JSON</DropdownMenu.Item></DropdownMenu.SubContent></DropdownMenu.Portal></DropdownMenu.Sub>
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
  const snoozed = isSnoozed(s)
  return (
    <div onClick={onSelect} className={`group/row flex items-start gap-2 px-2 py-1.5 rounded cursor-pointer ${active ? 'bg-selected' : 'hover:bg-hover'}`} data-session-id={s.id}>
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
          {snoozed && <span className="flex items-center gap-0.5 text-accent" title={`Snoozed until ${new Date(s.snoozedUntil ?? 0).toLocaleString()}`}><AlarmClock size={9} /> {fmtWake(s.snoozedUntil ?? 0)}</span>}
          {s.settled && <span className="flex items-center gap-0.5" title="Settled"><CheckCheck size={9} /> settled</span>}
          {busy && <LoaderCircle size={9} className="spin" />}
        </div>
      </div>
      <div className="opacity-0 group-hover/row:opacity-100"><SessionMenu s={s} onRename={() => { setTitle(s.title); setRenaming(true) }} /></div>
    </div>
  )
}

function SectionHeader({ title, count, collapsed, onToggle, meta }: { title: string; count: number; collapsed: boolean; onToggle: () => void; meta?: string }) {
  return (
    <button type="button" onClick={onToggle} aria-expanded={!collapsed} className="w-full flex items-center gap-1 px-1.5 pt-1 pb-0.5 text-[10.5px] text-faint hover:text-fg">
      <ChevronRight size={10} className="chat-chevron shrink-0" data-open={!collapsed} /><span>{title}</span><span className="opacity-70">{count}</span>
      {meta && <span className="ml-auto truncate-1">{meta}</span>}
    </button>
  )
}

/** Lifecycle sections (Working, Pinned, day buckets, Snoozed, Settled, Archived); fold state lives in the ui store. */
export function ThreadSections({ sections, active, onSelect, compact, emptyText }: { sections: ThreadSection[]; active: string | null; onSelect: (s: ChatSessionSummary) => void; compact?: boolean; emptyText?: string }) {
  const toggle = useUi((u) => u.toggleThreadFold)
  if (!sections.length) return emptyText ? <div className="px-3 py-6 text-center text-faint text-[11.5px]">{emptyText}</div> : null
  return (
    <>
      {sections.map((sec) => (
        <div key={sec.id}>
          <SectionHeader title={sec.title} count={sec.sessions.length} collapsed={sec.collapsed} onToggle={() => toggle(sec.id, sec.defaultCollapsed)} meta={sec.id === 'snoozed' && sec.collapsed ? `next wake ${fmtWake(sec.sessions[0]?.snoozedUntil ?? 0)}` : undefined} />
          {!sec.collapsed && sec.sessions.map((s) => <SessionRow key={s.id} s={s} active={s.id === active} onSelect={() => onSelect(s)} compact={compact} />)}
        </div>
      ))}
    </>
  )
}

/** "Group by project": one foldable group per workspace root, each with its lifecycle sections. */
export function ProjectSections({ groups, active, onSelect, compact }: { groups: ProjectGroup[]; active: string | null; onSelect: (s: ChatSessionSummary) => void; compact?: boolean }) {
  const toggle = useUi((u) => u.toggleThreadFold)
  return (
    <>
      {groups.map((g) => (
        <div key={g.root || '__global'} className="mb-1">
          <button type="button" onClick={() => toggle(projectFoldKey(g.root))} aria-expanded={!g.collapsed} title={g.root || 'Sessions without a folder'} className="w-full flex items-center gap-1 px-1.5 h-6 rounded text-[11px] font-medium hover:bg-hover">
            <ChevronRight size={11} className="chat-chevron shrink-0" data-open={!g.collapsed} /><FolderOpen size={11} className="text-faint shrink-0" /><span className="truncate-1">{g.title}</span>
            {g.current && <span className="text-[9.5px] text-faint font-normal">current</span>}
            <span className="ml-auto text-[10px] text-faint font-normal">{g.count}</span>
          </button>
          {!g.collapsed && <div className="pl-2"><ThreadSections sections={g.sections} active={active} onSelect={onSelect} compact={compact} /></div>}
        </div>
      ))}
    </>
  )
}

function MessageHits({ hits, loading, onOpen }: { hits: MessageSearchHit[]; loading: boolean; onOpen: (h: MessageSearchHit) => void }) {
  return (
    <div className="mt-1 border-t border-border pt-1">
      <div className="px-2 pt-1 pb-0.5 text-[10.5px] text-faint flex items-center gap-1">Messages{loading && <LoaderCircle size={9} className="spin" />}</div>
      {hits.map((h) => (
        <button key={`${h.sessionId}:${h.messageId}`} type="button" onClick={() => onOpen(h)} className="w-full text-left px-2 py-1.5 rounded hover:bg-hover">
          <div className="flex items-center gap-1 text-[11.5px]"><span className="truncate-1 flex-1">{h.sessionTitle || 'New chat'}</span><span className="text-[10px] text-faint shrink-0">{fmtRelative(h.updatedAt)}</span></div>
          <div className="text-[10.5px] text-faint truncate-1"><span className="uppercase text-[9px] mr-1">{h.role === 'user' ? 'you' : h.role}</span>{h.snippet}</div>
        </button>
      ))}
      {!loading && hits.length === 0 && <div className="px-2 py-2 text-[11px] text-faint">No messages match</div>}
    </div>
  )
}

export function HistoryPopover({ children }: { children: React.ReactNode }) {
  const open = useChat((s) => s.historyOpen)
  const setOpen = useChat((s) => s.setHistoryOpen)
  const sessions = useChat((s) => s.sessions)
  const allSessions = useChat((s) => s.allSessions)
  const active = useChat((s) => s.activeSessionId)
  const openThread = useChat((s) => s.openThread)
  const loadAll = useChat((s) => s.loadAllSessions)
  const historySearch = useChat((s) => s.historySearch)
  const showArchived = useUi((u) => u.threadShowArchived)
  const byProject = useUi((u) => u.threadGroupByProject)
  const folds = useUi((u) => u.threadFolds)
  const setUi = useUi((u) => u.set)
  const root = useWorkspace((w) => w.workspace?.root ?? null)
  const [q, setQ] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)
  const [hits, setHits] = useState<MessageSearchHit[]>([])
  const [searching, setSearching] = useState(false)

  // `chat.searchMessages`: open focused on the search box (optionally prefilled).
  useEffect(() => { if (historySearch.nonce) { setQ(historySearch.query); requestAnimationFrame(() => inputRef.current?.focus()) } }, [historySearch])
  useEffect(() => { if (open && byProject) void loadAll() }, [open, byProject, loadAll])
  // Message search across session files: at least 3 characters, debounced 200 ms.
  useEffect(() => {
    const query = q.trim()
    if (query.length < 3) { setHits([]); setSearching(false); return }
    setSearching(true)
    let cancelled = false
    const t = window.setTimeout(() => {
      useChat.getState().searchMessages(query, { includeArchived: showArchived, allWorkspaces: byProject, limit: 30 })
        .then((r) => { if (!cancelled) { setHits(r); setSearching(false) } })
        .catch(() => { if (!cancelled) { setHits([]); setSearching(false) } })
    }, 200)
    return () => { cancelled = true; window.clearTimeout(t) }
  }, [q, showArchived, byProject])

  const opts = useMemo(() => ({ showArchived, folds, isBusy: isSessionBusy, query: q }), [showArchived, folds, q])
  const sections = useMemo(() => (byProject ? null : groupSessions(sessions, opts)), [byProject, sessions, opts])
  const groups = useMemo(() => (byProject ? groupSessionsByProject(allSessions.length ? allSessions : sessions, root, opts) : null), [byProject, allSessions, sessions, root, opts])
  const pick = (s: ChatSessionSummary) => { void openThread(s.id, s.workspaceRoot); setOpen(false) }
  const openHit = (h: MessageSearchHit) => { void openThread(h.sessionId, h.workspaceRoot, h.messageId); setOpen(false) }
  const showMessages = q.trim().length >= 3
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild>{children}</Popover.Trigger>
      <Popover.Portal>
        <Popover.Content align="end" sideOffset={6} className="z-40 w-[360px] rounded-md border border-border bg-tooltip shadow-[var(--shadow)] text-fg">
          <div className="flex items-center gap-1.5 px-2 h-8 border-b border-border">
            <History size={12} className="text-faint" /><Search size={12} className="text-faint" />
            <input ref={inputRef} autoFocus value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search threads and messages…" className="flex-1 min-w-0 bg-transparent outline-none text-[12px]" />
            <button type="button" onClick={() => setUi({ threadGroupByProject: !byProject })} title="Group threads by project (all workspaces)" className={toggleCls(byProject)}>by project</button>
            <button type="button" onClick={() => setUi({ threadShowArchived: !showArchived })} title="Show archived threads" className={toggleCls(showArchived)}>archived</button>
          </div>
          <div className="max-h-[60vh] overflow-y-auto p-1">
            {groups ? <ProjectSections groups={groups} active={active} onSelect={pick} /> : <ThreadSections sections={sections ?? []} active={active} onSelect={pick} />}
            {sessions.length === 0 && !showMessages && <div className="px-2 py-4 text-center text-faint text-[11.5px]">No chats yet</div>}
            {showMessages && <MessageHits hits={hits} loading={searching} onOpen={openHit} />}
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}

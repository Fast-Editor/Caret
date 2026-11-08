import { useEffect, useMemo, useState } from 'react'
import { FolderGit2, FolderOpen, Plus } from 'lucide-react'
import type { ChatSessionSummary } from '@/types'
import { useChat, isSessionBusy } from '@/store/chat'
import { useUi } from '@/store/ui'
import { useWorkspace } from '@/store/workspace'
import { ProjectSections, ThreadSections } from './HistoryPopover'
import { groupSessions, groupSessionsByProject, type ThreadSection } from './util'
import { runCommand } from '@/lib/commands'

type Filter = 'all' | 'running' | 'archived'

/** The filter tabs narrow the lifecycle sections: running = Working only, archived = Archived only. */
function pickSections(sections: ThreadSection[], filter: Filter): ThreadSection[] {
  if (filter === 'running') return sections.filter((s) => s.id === 'working')
  if (filter === 'archived') return sections.filter((s) => s.id === 'archived')
  return sections
}

export function AgentsSidebar({ onOpen }: { onOpen?: (sessionId: string) => void } = {}) {
  const sessions = useChat((s) => s.sessions)
  const allSessions = useChat((s) => s.allSessions)
  const active = useChat((s) => s.activeSessionId)
  const openThread = useChat((s) => s.openThread)
  const loadAll = useChat((s) => s.loadAllSessions)
  const byProject = useUi((u) => u.threadGroupByProject)
  const folds = useUi((u) => u.threadFolds)
  const setUi = useUi((u) => u.set)
  const root = useWorkspace((w) => w.workspace?.root ?? null)
  const [filter, setFilter] = useState<Filter>('all')
  useEffect(() => { if (byProject) void loadAll() }, [byProject, loadAll])
  const opts = useMemo(() => ({ showArchived: filter === 'archived', folds, isBusy: isSessionBusy }), [filter, folds])
  const sections = useMemo(() => (byProject ? null : pickSections(groupSessions(sessions, opts), filter)), [byProject, sessions, opts, filter])
  const groups = useMemo(() => (byProject ? groupSessionsByProject(allSessions.length ? allSessions : sessions, root, opts).map((g) => ({ ...g, sections: pickSections(g.sections, filter) })).filter((g) => g.sections.length) : null), [byProject, allSessions, sessions, root, opts, filter])
  const open = (s: ChatSessionSummary) => {
    const local = useChat.getState().sessions.some((x) => x.id === s.id)
    void openThread(s.id, s.workspaceRoot)
    if (!local) return // another project: openThread offers to open that folder first
    if (onOpen) onOpen(s.id)
    else { const ui = useUi.getState(); if (!ui.chatVisible && ui.layout !== 'agents') ui.set({ chatVisible: true }); useChat.getState().focusComposer() }
  }
  const running = sessions.filter((s) => !s.isSubagent && (s.status === 'running' || s.status === 'awaiting-approval' || s.status === 'awaiting-input')).length
  const empty = filter === 'running' ? 'No agents running' : filter === 'archived' ? 'No archived chats' : 'No agents yet. Press ⌘I to start one.'
  return (
    <div className="h-full flex flex-col min-w-0">
      <div className="flex items-center gap-1 px-2 h-8 shrink-0">
        <span className="text-[11px] uppercase tracking-wide text-muted flex-1">Agents{running > 0 && <span className="ml-1.5 px-1 rounded bg-accent/20 text-accent normal-case">{running} active</span>}</span>
        <button type="button" onClick={() => setUi({ threadGroupByProject: !byProject })} title="Group by project (all workspaces)" className={`w-6 h-6 flex items-center justify-center rounded hover:bg-hover ${byProject ? 'bg-selected text-fg' : 'text-muted'}`}><FolderOpen size={13} /></button>
        <button type="button" onClick={() => void runCommand('chat.newBackground')} title="New background agent (isolated worktree) ⌘E" className="w-6 h-6 flex items-center justify-center rounded hover:bg-hover text-muted"><FolderGit2 size={13} /></button>
        <button type="button" onClick={() => void runCommand('chat.new')} title="New agent ⌘I" className="w-6 h-6 flex items-center justify-center rounded hover:bg-hover text-muted"><Plus size={14} /></button>
      </div>
      <div className="flex gap-1 px-2 pb-1.5 text-[11px]">
        {(['all', 'running', 'archived'] as Filter[]).map((f) => <button key={f} type="button" onClick={() => setFilter(f)} className={`px-1.5 h-5 rounded capitalize ${filter === f ? 'bg-selected text-fg' : 'text-muted hover:bg-hover'}`}>{f}</button>)}
      </div>
      <div className="flex-1 overflow-y-auto px-1">
        {groups
          ? (groups.length ? <ProjectSections groups={groups} active={active} onSelect={open} /> : <div className="px-3 py-6 text-center text-faint text-[11.5px]">{empty}</div>)
          : <ThreadSections sections={sections ?? []} active={active} onSelect={open} emptyText={empty} />}
      </div>
    </div>
  )
}

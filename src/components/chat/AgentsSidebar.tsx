import { useMemo, useState } from 'react'
import { FolderGit2, Plus } from 'lucide-react'
import { useChat } from '@/store/chat'
import { useUi } from '@/store/ui'
import { SessionRow } from './HistoryPopover'
import { runCommand } from '@/lib/commands'

type Filter = 'all' | 'running' | 'archived'

export function AgentsSidebar({ onOpen }: { onOpen?: (sessionId: string) => void } = {}) {
  const sessions = useChat((s) => s.sessions)
  const active = useChat((s) => s.activeSessionId)
  const select = useChat((s) => s.selectSession)
  const [filter, setFilter] = useState<Filter>('all')
  const list = useMemo(() => {
    const base = sessions.filter((s) => !s.isSubagent)
    const f = filter === 'running' ? base.filter((s) => s.status === 'running' || s.status === 'awaiting-approval' || s.status === 'awaiting-input' || s.status === 'summarizing') : filter === 'archived' ? base.filter((s) => s.archived) : base.filter((s) => !s.archived)
    const rank = (s: typeof base[number]) => (s.status === 'awaiting-approval' || s.status === 'awaiting-input' ? 0 : s.status === 'running' ? 1 : s.pinned ? 2 : 3)
    return [...f].sort((a, b) => rank(a) - rank(b) || b.updatedAt - a.updatedAt)
  }, [sessions, filter])
  const open = (id: string) => { void select(id); if (onOpen) onOpen(id); else { const ui = useUi.getState(); if (!ui.chatVisible && ui.layout !== 'agents') ui.set({ chatVisible: true }); useChat.getState().focusComposer() } }
  const running = sessions.filter((s) => !s.isSubagent && (s.status === 'running' || s.status === 'awaiting-approval' || s.status === 'awaiting-input')).length
  return (
    <div className="h-full flex flex-col min-w-0">
      <div className="flex items-center gap-1 px-2 h-8 shrink-0">
        <span className="text-[11px] uppercase tracking-wide text-muted flex-1">Agents{running > 0 && <span className="ml-1.5 px-1 rounded bg-accent/20 text-accent normal-case">{running} active</span>}</span>
        <button type="button" onClick={() => void runCommand('chat.newBackground')} title="New background agent (isolated worktree) ⌘E" className="w-6 h-6 flex items-center justify-center rounded hover:bg-hover text-muted"><FolderGit2 size={13} /></button>
        <button type="button" onClick={() => void runCommand('chat.new')} title="New agent ⌘I" className="w-6 h-6 flex items-center justify-center rounded hover:bg-hover text-muted"><Plus size={14} /></button>
      </div>
      <div className="flex gap-1 px-2 pb-1.5 text-[11px]">
        {(['all', 'running', 'archived'] as Filter[]).map((f) => <button key={f} type="button" onClick={() => setFilter(f)} className={`px-1.5 h-5 rounded capitalize ${filter === f ? 'bg-selected text-fg' : 'text-muted hover:bg-hover'}`}>{f}</button>)}
      </div>
      <div className="flex-1 overflow-y-auto px-1">
        {list.map((s) => <SessionRow key={s.id} s={s} active={s.id === active} onSelect={() => open(s.id)} />)}
        {list.length === 0 && <div className="px-3 py-6 text-center text-faint text-[11.5px]">{filter === 'running' ? 'No agents running' : filter === 'archived' ? 'No archived chats' : 'No agents yet. Press ⌘I to start one.'}</div>}
      </div>
    </div>
  )
}

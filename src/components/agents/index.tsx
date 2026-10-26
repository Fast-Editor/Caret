import { useEffect, useMemo, useState } from 'react'
import { Check, ChevronDown, ChevronRight, FileDiff, LayoutPanelLeft, ListChecks, Undo2, X } from 'lucide-react'
import { useChat } from '@/store/chat'
import { ChatPane } from '@/components/chat/ChatPane'
import { AgentsSidebar } from '@/components/chat/AgentsSidebar'
import { DiffView } from '@/components/chat/DiffView'
import { TodoList } from '@/components/chat/TodoList'
import { runCommand, hasCommand } from '@/lib/commands'
import { fmtRelative } from '@/components/chat/util'
import type { PendingChange } from '@/types'

function ChangeRow({ sessionId, changeId }: { sessionId: string; changeId: string }) {
  const change = useChat((s) => s.loaded[sessionId]?.pendingChanges.find((c) => c.changeId === changeId))
  const cached = useChat((s) => s.changeCache[changeId])
  const loadChange = useChat((s) => s.loadChange)
  const accept = useChat((s) => s.acceptChanges)
  const reject = useChat((s) => s.rejectChanges)
  const [open, setOpen] = useState(false)
  useEffect(() => { if (open && !cached) void loadChange(changeId) }, [open, cached, changeId, loadChange])
  if (!change) return null
  const pc: PendingChange | undefined = cached
  return (
    <div className="border-b border-border">
      <div className="group/c flex items-center gap-2 px-2 h-8 hover:bg-hover">
        <button type="button" onClick={() => setOpen((o) => !o)} className="flex items-center gap-1.5 flex-1 min-w-0 text-left">
          {open ? <ChevronDown size={12} className="text-faint" /> : <ChevronRight size={12} className="text-faint" />}
          <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${change.kind === 'create' ? 'bg-success' : change.kind === 'delete' ? 'bg-danger' : 'bg-warning'}`} />
          <span className="truncate-1 mono text-[11.5px]">{change.path.split('/').slice(-2).join('/')}</span>
          <span className="text-diff-add-fg text-[11px]">+{change.additions}</span><span className="text-diff-del-fg text-[11px]">−{change.deletions}</span>
          {change.status !== 'pending' && <span className={`text-[10px] uppercase ${change.status === 'accepted' ? 'text-success' : 'text-faint'}`}>{change.status}</span>}
        </button>
        {hasCommand('editor.openDiff') && <button type="button" title="Open in editor" onClick={() => void loadChange(changeId).then((p) => p && runCommand('editor.openDiff', { path: p.path, original: p.before ?? '', modified: p.after ?? '', title: p.path.split('/').pop(), changeId, sessionId }))} className="opacity-0 group-hover/c:opacity-100 text-muted hover:text-fg"><FileDiff size={12} /></button>}
        {change.status === 'pending' && <><button type="button" title="Reject" onClick={() => void reject(sessionId, [changeId])} className="opacity-0 group-hover/c:opacity-100 text-muted hover:text-danger"><X size={12} /></button><button type="button" title="Accept" onClick={() => void accept(sessionId, [changeId])} className="opacity-0 group-hover/c:opacity-100 text-muted hover:text-success"><Check size={12} /></button></>}
      </div>
      {open && <div className="px-2 pb-2">{pc ? <DiffView diff={pc.diff} maxLines={300} /> : <div className="text-faint text-[11px] px-1 py-2">Loading diff…</div>}</div>}
    </div>
  )
}

function ChangesColumn({ sessionId }: { sessionId: string }) {
  const session = useChat((s) => s.loaded[sessionId])
  const checkpoints = useChat((s) => s.checkpoints[sessionId])
  const loadCheckpoints = useChat((s) => s.loadCheckpoints)
  const restore = useChat((s) => s.restoreCheckpoint)
  const accept = useChat((s) => s.acceptChanges)
  const reject = useChat((s) => s.rejectChanges)
  const [showAll, setShowAll] = useState(false)
  useEffect(() => { void loadCheckpoints(sessionId).catch(() => {}) }, [sessionId, loadCheckpoints, session?.messages.length])
  const changes = useMemo(() => (session?.pendingChanges ?? []).filter((c) => showAll || c.status === 'pending'), [session?.pendingChanges, showAll])
  const pending = (session?.pendingChanges ?? []).filter((c) => c.status === 'pending')
  if (!session) return null
  return (
    <div className="h-full flex flex-col min-w-0 border-l border-border bg-sidebar">
      <div className="flex items-center gap-2 px-3 h-9 border-b border-border text-[12px]">
        <FileDiff size={13} className="text-muted" /><span className="font-medium">Changes</span><span className="text-faint">{pending.length} pending</span>
        <span className="flex-1" />
        <button type="button" onClick={() => setShowAll((a) => !a)} className="text-[10.5px] text-muted hover:text-fg">{showAll ? 'pending only' : 'show all'}</button>
        {pending.length > 0 && <><button type="button" onClick={() => void reject(sessionId, 'all')} className="px-1.5 h-6 rounded border border-border hover:bg-hover text-[11px]">Reject all</button><button type="button" onClick={() => void accept(sessionId, 'all')} className="px-1.5 h-6 rounded bg-accent text-accent-fg text-[11px]">Accept all</button></>}
      </div>
      <div className="flex-1 overflow-y-auto">
        {changes.map((c) => <ChangeRow key={c.changeId} sessionId={sessionId} changeId={c.changeId} />)}
        {changes.length === 0 && <div className="px-3 py-6 text-center text-faint text-[11.5px]">No {showAll ? '' : 'pending '}changes</div>}
        {session.todos.length > 0 && <div className="mt-2 flex items-center gap-1.5 px-3 h-7 text-[11px] uppercase tracking-wide text-muted"><ListChecks size={12} /> Tasks</div>}
        <TodoList sessionId={sessionId} todos={session.todos} />
        {checkpoints && checkpoints.length > 0 && (
          <div className="mt-2"><div className="flex items-center gap-1.5 px-3 h-7 text-[11px] uppercase tracking-wide text-muted"><Undo2 size={12} /> Checkpoints</div>
            {checkpoints.map((cp) => <button key={cp.id} type="button" onClick={() => { if (window.confirm(`Restore ${cp.files.length} file(s) to "${cp.label}"?`)) void restore(sessionId, cp.id) }} className="w-full flex items-center gap-2 px-3 h-7 text-left hover:bg-hover text-[11.5px]"><span className="truncate-1 flex-1">{cp.label}</span><span className="text-faint">{cp.files.length} files · {fmtRelative(cp.createdAt)}</span></button>)}
          </div>
        )}
      </div>
    </div>
  )
}

/** Chat-centric workbench: sessions · transcript · changes. */
export function AgentsLayout() {
  const activeId = useChat((s) => s.activeSessionId)
  return (
    <div className="h-full w-full flex min-w-0 bg-app">
      <div className="w-[260px] shrink-0 border-r border-border bg-sidebar flex flex-col">
        <div className="flex items-center gap-2 px-3 h-9 border-b border-border text-[12px]"><LayoutPanelLeft size={13} className="text-muted" /><span className="font-medium flex-1">Agents</span><button type="button" onClick={() => void runCommand('chat.toggleAgentsLayout')} title="Back to editor layout" className="text-[11px] text-muted hover:text-fg">Editor →</button></div>
        <div className="flex-1 min-h-0"><AgentsSidebar onOpen={() => {}} /></div>
      </div>
      <div className="flex-1 min-w-0 flex justify-center bg-app"><div className="w-full max-w-[900px] h-full border-x border-border"><ChatPane embedded /></div></div>
      <div className="w-[380px] shrink-0 min-w-0">{activeId ? <ChangesColumn sessionId={activeId} /> : null}</div>
    </div>
  )
}

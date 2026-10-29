import { useEffect, useState } from 'react'
import { Tooltip } from 'radix-ui'
import { History, LayoutPanelLeft, Plus, Settings, X } from 'lucide-react'
import { useChat, isSessionBusy } from '@/store/chat'
import { useUi } from '@/store/ui'
import { runCommand } from '@/lib/commands'
import { MessageList } from './MessageList'
import { Composer } from './Composer'
import { ReviewBar } from './ReviewBar'
import { ProofPanel } from '@/components/review/ProofPanel'
import { TodoList } from './TodoList'
import { QueuedMessages } from './QueuedMessages'
import { EmptyState } from './EmptyState'
import { HistoryPopover } from './HistoryPopover'
import { fmtCost, fmtTokens } from './util'
import './chat.css'

function IconBtn({ title, onClick, children }: { title: string; onClick: () => void; children: React.ReactNode }) {
  return <button type="button" onClick={onClick} title={title} className="flex items-center justify-center w-6 h-6 rounded hover:bg-hover text-muted hover:text-fg press">{children}</button>
}

export function ChatPane({ embedded }: { embedded?: boolean } = {}) {
  const activeId = useChat((s) => s.activeSessionId)
  const session = useChat((s) => (s.activeSessionId ? s.loaded[s.activeSessionId] : undefined))
  const summary = useChat((s) => s.sessions.find((x) => x.id === s.activeSessionId))
  const sessionsLoaded = useChat((s) => s.sessionsLoaded)
  const createSession = useChat((s) => s.createSession)
  const renameSession = useChat((s) => s.renameSession)
  const selectSession = useChat((s) => s.selectSession)
  const focusNonce = useChat((s) => s.composerFocusNonce)
  const setChat = useUi((s) => s.set)
  const [editingTitle, setEditingTitle] = useState(false)
  const [title, setTitle] = useState('')

  // Ensure there is a session to type into once sessions have loaded (reuse most recent non-archived idle chat with no messages, else create lazily on first send).
  useEffect(() => {
    if (!sessionsLoaded || activeId) return
    const st = useChat.getState()
    const recent = st.sessions.find((s) => !s.isSubagent && !s.archived)
    if (recent) void selectSession(recent.id)
    else void createSession({ activate: true })
  }, [sessionsLoaded, activeId, selectSession, createSession])

  useEffect(() => { if (activeId && !session) void useChat.getState().loadSession(activeId) }, [activeId, session])

  const busy = isSessionBusy(session?.status)
  const usage = session?.usage
  const titleText = session?.title ?? summary?.title ?? 'New chat'

  return (
    <Tooltip.Provider delayDuration={400}>
      <div className="h-full w-full flex flex-col bg-sidebar min-w-0" data-chat-pane>
        <div className="h-[34px] shrink-0 flex items-center gap-1 px-2 border-b border-border">
          <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${busy ? 'bg-accent animate-breathe' : session?.status === 'awaiting-approval' || session?.status === 'awaiting-input' ? 'bg-warning' : session?.status === 'error' ? 'bg-danger' : 'bg-border-strong'}`} title={session?.status ?? 'idle'} />
          {editingTitle ? (
            <input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} onBlur={() => { setEditingTitle(false); if (activeId && title.trim()) void renameSession(activeId, title.trim()) }} onKeyDown={(e) => { if (e.key === 'Enter') (e.target as HTMLInputElement).blur(); if (e.key === 'Escape') setEditingTitle(false) }} className="flex-1 h-6 px-1 bg-input border border-border rounded text-[12px] outline-none" />
          ) : (
            <button type="button" onDoubleClick={() => { setTitle(titleText); setEditingTitle(true) }} title={usage ? `Session usage: ${fmtTokens(usage.inputTokens)} in · ${fmtTokens(usage.outputTokens)} out${usage.costUsd !== undefined ? ` · ${fmtCost(usage.costUsd)}` : ''} (double-click to rename)` : 'Double-click to rename'} className="flex-1 min-w-0 text-left text-[12.5px] font-medium truncate-1">{titleText}{session?.worktreePath && <span className="ml-1.5 text-[10px] text-faint font-normal">worktree</span>}</button>
          )}
          <IconBtn title="New chat (⌘I)" onClick={() => void createSession().then(() => useChat.getState().focusComposer())}><Plus size={14} /></IconBtn>
          <HistoryPopover><button type="button" title="History (⌘⇧H)" className="flex items-center justify-center w-6 h-6 rounded hover:bg-hover text-muted hover:text-fg"><History size={14} /></button></HistoryPopover>
          {!embedded && <IconBtn title="Agents layout" onClick={() => void runCommand('chat.toggleAgentsLayout')}><LayoutPanelLeft size={14} /></IconBtn>}
          <IconBtn title="Agent settings" onClick={() => void runCommand('settings.open', { page: 'agent' })}><Settings size={14} /></IconBtn>
          {!embedded && <IconBtn title="Close pane (⌘L)" onClick={() => setChat({ chatVisible: false })}><X size={14} /></IconBtn>}
        </div>
        {activeId ? (
          <>
            {session && session.messages.length > 0 ? <MessageList sessionId={activeId} messages={session.messages} /> : <div key={activeId} className="flex-1 min-h-0 overflow-y-auto animate-fade"><EmptyState sessionId={activeId} /></div>}
            {session && <TodoList sessionId={activeId} todos={session.todos} />}
            {session && <ProofPanel sessionId={activeId} compact />}
            {session && <ReviewBar sessionId={activeId} changes={session.pendingChanges} />}
            {session && <QueuedMessages sessionId={activeId} queued={session.queuedMessages} />}
            <Composer sessionId={activeId} autoFocusNonce={focusNonce} />
          </>
        ) : (
          <div className="flex-1 flex items-center justify-center text-muted text-[12px]">{sessionsLoaded ? 'Starting…' : 'Loading chats…'}</div>
        )}
      </div>
    </Tooltip.Provider>
  )
}

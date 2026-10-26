import { useShallow } from 'zustand/shallow'
import { Bot, Bug, ListChecks, MessageCircle } from 'lucide-react'
import { useChat } from '@/store/chat'
import { MOD } from './util'

const CARDS = [
  { id: 'agent', icon: Bot, title: 'Agent', text: 'Build features, fix bugs, refactor — the agent edits files and runs commands.' },
  { id: 'ask', icon: MessageCircle, title: 'Ask', text: 'Understand code. Read-only answers with citations into your files.' },
  { id: 'plan', icon: ListChecks, title: 'Plan', text: 'Research first, then approve a step-by-step plan before building.' },
  { id: 'debug', icon: Bug, title: 'Debug', text: 'Instrument, run, analyze — hypothesis-driven debugging.' },
]

export function EmptyState({ sessionId }: { sessionId: string }) {
  const setMode = useChat((s) => s.setMode)
  const mode = useChat((s) => s.draft(sessionId).mode)
  const sessions = useChat(useShallow((s) => s.sessions.filter((x) => !x.isSubagent && x.messageCount > 0).slice(0, 4)))
  const select = useChat((s) => s.selectSession)
  return (
    <div className="flex flex-col items-center justify-center h-full px-5 text-center select-none">
      <div className="text-[15px] font-semibold mb-1">What are we building?</div>
      <div className="text-muted text-[12px] mb-4">Type <kbd className="px-1 rounded border border-border bg-input mono">@</kbd> to add files, docs, git or web context · <kbd className="px-1 rounded border border-border bg-input mono">/</kbd> for commands</div>
      <div className="grid grid-cols-2 gap-2 w-full max-w-[420px]">
        {CARDS.map((c) => (
          <button key={c.id} type="button" onClick={() => void setMode(sessionId, c.id)} className={`text-left p-2.5 rounded-md border ${mode === c.id ? 'border-accent bg-selected' : 'border-border hover:bg-hover'}`}>
            <div className="flex items-center gap-1.5 text-[12.5px] font-medium"><c.icon size={13} className="text-accent" /> {c.title}</div>
            <div className="text-[11px] text-muted mt-0.5 leading-[1.4]">{c.text}</div>
          </button>
        ))}
      </div>
      <div className="text-[10.5px] text-faint mt-4 space-x-2"><span>{MOD}L add selection</span><span>{MOD}I new chat</span><span>{MOD}K inline edit</span><span>{MOD}⏎ accept changes</span></div>
      {sessions.length > 0 && <div className="mt-5 w-full max-w-[420px] text-left"><div className="text-[10.5px] text-faint mb-1">Recent</div>{sessions.map((s) => <button key={s.id} type="button" onClick={() => void select(s.id)} className="w-full text-left px-2 h-7 rounded hover:bg-hover text-[12px] truncate-1">{s.title}</button>)}</div>}
    </div>
  )
}

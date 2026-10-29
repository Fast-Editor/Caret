import { memo, useState } from 'react'
import { CircleCheck, Circle, CircleDot, Ban } from 'lucide-react'
import { Chevron, Collapse } from './Collapse'
import type { TodoItem } from '@/types'
import { useChat } from '@/store/chat'

export const TodoList = memo(function TodoList({ sessionId, todos }: { sessionId: string; todos: TodoItem[] }) {
  const setTodos = useChat((s) => s.setTodos)
  const [open, setOpen] = useState(true)
  if (!todos.length) return null
  const done = todos.filter((t) => t.status === 'completed').length
  const toggle = (t: TodoItem) => void setTodos(sessionId, todos.map((x) => (x.id === t.id ? { ...x, status: x.status === 'completed' ? 'pending' : 'completed' } : x)))
  return (
    <div className="mx-2 mb-1 rounded-md border border-border bg-panel text-[12px] animate-fade-up" data-todo-list>
      <button type="button" onClick={() => setOpen((o) => !o)} className="w-full flex items-center gap-2 px-2 h-7 text-left" aria-expanded={open}><Chevron open={open} size={12} className="text-muted" /><span className="font-medium">Tasks</span><span className="text-faint">{done}/{todos.length}</span><span className="flex-1" /><span className="h-1 w-24 rounded bg-border overflow-hidden"><span className="block h-full bg-accent chat-progress" style={{ width: `${(done / todos.length) * 100}%` }} /></span></button>
      <Collapse open={open}><ul className="border-t border-border max-h-40 overflow-y-auto py-1">{todos.map((t) => (
        <li key={t.id} className="flex items-start gap-2 px-2 py-0.5 hover:bg-hover animate-fade">
          <button type="button" onClick={() => toggle(t)} className="mt-0.5 shrink-0" title={t.status}>
            {t.status === 'completed' ? <CircleCheck size={13} className="text-success animate-pop" /> : t.status === 'in_progress' ? <CircleDot size={13} className="text-accent animate-breathe" /> : t.status === 'cancelled' ? <Ban size={13} className="text-faint" /> : <Circle size={13} className="text-muted" />}
          </button>
          <span className={`leading-[1.4] ${t.status === 'completed' || t.status === 'cancelled' ? 'line-through text-faint' : ''}`}>{t.content}</span>
        </li>
      ))}</ul></Collapse>
    </div>
  )
})

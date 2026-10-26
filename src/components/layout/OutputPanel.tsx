import { Trash2 } from 'lucide-react'
import { useEffect, useRef } from 'react'
import { IconButton, cn } from '@/components/common'
import { useOutput } from '@/store/notifications'

export function OutputPanel() {
  const lines = useOutput((s) => s.lines)
  const clear = useOutput((s) => s.clear)
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => { const el = ref.current; if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 80) el.scrollTop = el.scrollHeight }, [lines.length])
  return (
    <div className="h-full flex flex-col">
      <div className="flex items-center justify-end px-2 h-6 shrink-0"><IconButton size="xs" icon={<Trash2 />} label="Clear Output" onClick={clear} /></div>
      <div ref={ref} className="flex-1 overflow-auto px-3 pb-2 mono text-[11px] leading-5 selectable">
        {lines.length === 0 && <div className="text-faint">No output yet.</div>}
        {lines.map((l, i) => (
          <div key={i} className={cn('whitespace-pre-wrap break-words', l.level === 'error' ? 'text-danger' : l.level === 'warn' ? 'text-warning' : l.level === 'debug' ? 'text-faint' : 'text-fg')}>
            <span className="text-faint">{new Date(l.at).toLocaleTimeString()} </span>{l.text}
          </div>
        ))}
      </div>
    </div>
  )
}

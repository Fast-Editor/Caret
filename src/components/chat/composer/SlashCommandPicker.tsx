import { useEffect, useMemo } from 'react'
import { Command } from 'cmdk'
import { Slash } from 'lucide-react'
import type { SlashCommandInfo } from '@/types'

export function SlashCommandPicker({ query, commands, onPick, onClose }: { query: string; commands: SlashCommandInfo[]; onPick: (c: SlashCommandInfo) => void; onClose: () => void }) {
  const list = useMemo(() => commands.filter((c) => c.name.toLowerCase().startsWith(query.toLowerCase())).slice(0, 30), [commands, query])
  useEffect(() => { const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.stopPropagation(); onClose() } }; window.addEventListener('keydown', onKey, true); return () => window.removeEventListener('keydown', onKey, true) }, [onClose])
  if (list.length === 0) return null
  return (
    <div className="absolute bottom-full left-0 right-0 mb-1 z-30 rounded-md border border-border bg-tooltip shadow-[var(--shadow)] overflow-hidden text-[12px]">
      <Command shouldFilter={false} loop label="Commands">
        <Command.Input value={query} readOnly className="sr-only" aria-hidden />
        <Command.List className="max-h-64 overflow-y-auto p-1">
          {list.map((c) => (
            <Command.Item key={`${c.source}:${c.name}`} value={c.name} onSelect={() => onPick(c)} className="flex items-center gap-2 px-2 h-7 rounded data-[selected=true]:bg-selected cursor-pointer">
              <Slash size={12} className="text-muted" /><span className="mono">/{c.name}</span>
              {c.argumentHint && <span className="text-faint">{c.argumentHint}</span>}
              {c.description && <span className="truncate-1 text-faint ml-1">{c.description}</span>}
              <span className="ml-auto text-[10px] text-faint uppercase">{c.source}</span>
            </Command.Item>
          ))}
        </Command.List>
      </Command>
    </div>
  )
}

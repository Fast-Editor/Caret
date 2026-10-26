import { Command } from 'cmdk'
import { useEffect, useMemo, useState } from 'react'
import { listCommands, onCommandsChanged, runCommand, type CommandSpec } from '@/lib/commands'
import { getKeybindingFor, onKeybindingsChanged } from '@/lib/keybindings'
import { Kbd } from '@/components/common'
import { PaletteShell, PaletteItem } from './PaletteShell'
import { useUi } from '@/store/ui'

const RECENT_KEY = 'caret.palette.recent'
function loadRecent(): string[] { try { return JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]') as string[] } catch { return [] } }
export function pushRecentCommand(id: string): void {
  const r = [id, ...loadRecent().filter((x) => x !== id)].slice(0, 12)
  localStorage.setItem(RECENT_KEY, JSON.stringify(r))
}

export function CommandPalette({ initial = '' }: { initial?: string }) {
  const close = useUi((s) => s.closePalette)
  const openPalette = useUi((s) => s.openPalette)
  const [value, setValue] = useState(initial)
  const [tick, setTick] = useState(0)
  useEffect(() => { const a = onCommandsChanged(() => setTick((t) => t + 1)); const b = onKeybindingsChanged(() => setTick((t) => t + 1)); return () => { a(); b() } }, [])
  const commands = useMemo(() => {
    const recent = loadRecent()
    const all = listCommands().filter((c) => !c.hidden && (!c.when || c.when()))
    const rank = (c: CommandSpec) => { const i = recent.indexOf(c.id); return i < 0 ? 999 : i }
    return all.sort((a, b) => rank(a) - rank(b) || (a.category ?? '').localeCompare(b.category ?? '') || a.title.localeCompare(b.title))
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick])
  const recentIds = useMemo(() => new Set(loadRecent()), [tick])

  // Prefix switching from the command palette input
  useEffect(() => {
    if (value.startsWith('>')) return
    if (value === '') return
    if (value.startsWith('@')) openPalette('symbols', value)
    else if (value.startsWith('#')) openPalette('search', value)
  }, [value, openPalette])

  const q = value.startsWith('>') ? value.slice(1).trimStart() : value
  const run = (c: CommandSpec) => { close(); pushRecentCommand(c.id); void runCommand(c.id) }
  return (
    <PaletteShell label="Command palette" value={value} onValueChange={setValue} placeholder="Type a command (⌘⇧P)… prefixes: > commands · @ symbols · # search" onClose={close}
      footer={<><span><Kbd keys="up" />/<Kbd keys="down" /> navigate</span><span><Kbd keys="enter" /> run</span><span><Kbd keys="escape" /> close</span></>}>
      <Command.Empty className="px-3 py-6 text-center text-xs text-muted">No matching commands</Command.Empty>
      {commands.map((c) => {
        const kb = c.keybinding ?? getKeybindingFor(c.id)
        return (
          <PaletteItem key={c.id} value={`${c.id} ${c.category ?? ''} ${c.title}`} keywords={[q]} onSelect={() => run(c)}>
            <span className="text-muted truncate">{c.category ? `${c.category}: ` : ''}<span className="text-fg">{c.title}</span></span>
            {recentIds.has(c.id) && <span className="text-[10px] text-faint">recently used</span>}
            <span className="ml-auto flex items-center gap-2">{kb && <Kbd keys={kb} />}</span>
          </PaletteItem>
        )
      })}
    </PaletteShell>
  )
}

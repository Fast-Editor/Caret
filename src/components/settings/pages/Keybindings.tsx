import { useEffect, useMemo, useState, type KeyboardEvent } from 'react'
import { Keyboard, RotateCcw, X, ExternalLink, Search } from 'lucide-react'
import { listCommands, onCommandsChanged, runCommand, type CommandSpec } from '@/lib/commands'
import { formatKeyChord, prettyKeybinding } from '@/lib/settingsSchema'
import { useSettings } from '@/store/settings'
import { api } from '@/lib/api'
import { Section, Button, IconButton, Dialog, notify, errorMessage, cn } from '../primitives'

type KbService = { listBindings?: () => { command: string; key: string; source: 'default' | 'user' }[]; setUserBinding?: (command: string, key: string | null) => Promise<void> | void }
const kbModules = import.meta.glob<KbService>('/src/lib/keybindings.ts')
async function loadKbService(): Promise<KbService | null> { const k = Object.keys(kbModules)[0]; if (!k) return null; try { return await kbModules[k]() } catch { return null } }

export function KeybindingsPage() {
  const [, bump] = useState(0)
  const [q, setQ] = useState('')
  const [editing, setEditing] = useState<CommandSpec | null>(null)
  const [svc, setSvc] = useState<KbService | null>(null)
  const overrides = useSettings((s) => s.settings?.keybindings ?? {})
  const update = useSettings((s) => s.update)
  useEffect(() => onCommandsChanged(() => bump((n) => n + 1)), [])
  useEffect(() => { void loadKbService().then(setSvc) }, [])

  const rows = useMemo(() => {
    const svcMap = new Map((svc?.listBindings?.() ?? []).map((b) => [b.command, b]))
    const all = listCommands().filter((c) => !c.hidden).map((c) => {
      const ov = overrides[c.id]
      const key = ov !== undefined ? ov : (svcMap.get(c.id)?.key ?? c.keybinding ?? null)
      return { cmd: c, key, source: ov !== undefined ? 'user' as const : 'default' as const }
    })
    const needle = q.trim().toLowerCase()
    return (needle ? all.filter((r) => `${r.cmd.title} ${r.cmd.id} ${r.cmd.category ?? ''} ${r.key ?? ''}`.toLowerCase().includes(needle)) : all).sort((a, b) => (a.cmd.category ?? '').localeCompare(b.cmd.category ?? '') || a.cmd.title.localeCompare(b.cmd.title))
  }, [q, overrides, svc])

  const setBinding = async (id: string, key: string | null) => {
    try {
      if (svc?.setUserBinding) await svc.setUserBinding(id, key)
      else await update({ keybindings: { [id]: key } })
      // settings-backed fallback: null = unbound; undefined would delete
    } catch (e) { notify('error', 'Could not save keybinding', errorMessage(e)) }
  }
  const resetBinding = async (id: string) => { try { await update({ keybindings: { [id]: undefined } as never }) } catch (e) { notify('error', 'Could not reset', errorMessage(e)) } }
  const openJson = async () => { try { const p = await api.settings.path('keybindings'); await runCommand('editor.openFile', { path: p }) } catch (e) { notify('error', 'Cannot open keybindings.json', errorMessage(e)) } }

  return (
    <>
      <div className="flex items-center gap-2 mb-3">
        <div className="relative flex-1 max-w-[420px]"><Search size={13} className="absolute left-2 top-1/2 -translate-y-1/2 text-muted" /><input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search commands or keys" className="h-7 w-full pl-7 pr-2 rounded-[var(--radius)] border border-border bg-input text-xs outline-none focus:border-accent" /></div>
        <Button onClick={openJson}><ExternalLink size={12} />Open keybindings.json</Button>
      </div>
      <Section title={`Commands (${rows.length})`} description="Click a shortcut to change it. User overrides are stored in settings and keybindings.json.">
        {rows.length === 0 && <div className="px-3 py-6 text-center text-xs text-muted">No commands registered yet.</div>}
        {rows.map(({ cmd, key, source }) => (
          <div key={cmd.id} className="px-3 py-1.5 flex items-center gap-3 hover:bg-hover/40">
            <div className="flex-1 min-w-0">
              <div className="text-xs truncate-1">{cmd.category ? <span className="text-muted">{cmd.category}: </span> : null}{cmd.title}</div>
              <div className="text-[10px] mono text-faint truncate-1">{cmd.id}</div>
            </div>
            <button onClick={() => setEditing(cmd)} className={cn('inline-flex items-center gap-1 min-w-[90px] justify-end', !key && 'text-faint')} title="Edit keybinding">
              {key ? prettyKeybinding(key).map((k, i) => <kbd key={i} className={cn('mono text-[11px] px-1.5 h-5 inline-flex items-center rounded border', source === 'user' ? 'border-accent/50 bg-accent/10' : 'border-border bg-hover', k === 'then' && 'border-0 bg-transparent px-0 text-faint')}>{k}</kbd>) : <span className="text-[11px]">—</span>}
            </button>
            <div className="flex gap-0.5 w-[52px] justify-end">
              {source === 'user' && <IconButton title="Reset to default" onClick={() => void resetBinding(cmd.id)}><RotateCcw size={12} /></IconButton>}
              {key && <IconButton title="Unbind" onClick={() => void setBinding(cmd.id, null)}><X size={12} /></IconButton>}
            </div>
          </div>
        ))}
      </Section>
      {editing && <CaptureDialog cmd={editing} onClose={() => setEditing(null)} onSave={(k) => { void setBinding(editing.id, k); setEditing(null) }} />}
    </>
  )
}

function CaptureDialog({ cmd, onClose, onSave }: { cmd: CommandSpec; onClose: () => void; onSave: (k: string) => void }) {
  const [chords, setChords] = useState<string[]>([])
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    e.preventDefault(); e.stopPropagation()
    if (e.key === 'Escape' && chords.length === 0) { onClose(); return }
    const c = formatKeyChord(e)
    if (!c) return
    setChords((prev) => (prev.length >= 2 ? [c] : [...prev, c]))
  }
  const value = chords.join(' ')
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose() }} title={`Keybinding for “${cmd.title}”`} description="Press the desired key combination (up to two chords). Enter saves, Escape cancels." width={460}
      footer={<><Button onClick={onClose}>Cancel</Button><Button onClick={() => setChords([])} disabled={!chords.length}>Clear</Button><Button variant="primary" disabled={!value} onClick={() => onSave(value)}>Save</Button></>}>
      <div tabIndex={0} autoFocus onKeyDown={(e) => { if (e.key === 'Enter' && value && !e.metaKey && !e.ctrlKey && !e.altKey) { onSave(value); return } onKey(e) }}
        className="h-16 rounded-md border border-dashed border-border-strong bg-input flex items-center justify-center gap-1 outline-none focus:border-accent">
        {value ? prettyKeybinding(value).map((k, i) => <kbd key={i} className={cn('mono text-sm px-2 h-7 inline-flex items-center rounded border border-border bg-hover', k === 'then' && 'border-0 bg-transparent text-faint text-xs')}>{k}</kbd>) : <span className="text-muted text-xs flex items-center gap-2"><Keyboard size={14} />Press keys…</span>}
      </div>
    </Dialog>
  )
}

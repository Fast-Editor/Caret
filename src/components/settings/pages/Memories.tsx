import { useEffect, useState } from 'react'
import { Database, Pencil, Plus, Trash2, Check, X } from 'lucide-react'
import { initRulesStore, useRules } from '@/store/rules'
import { api } from '@/lib/api'
import type { MemoryItem } from '@/types'
import { Section, Button, IconButton, Toggle, TextArea, Empty, confirmDialog, notify, errorMessage, relativeTime, Badge } from '../primitives'

export function MemoriesPage() {
  const memories = useRules((s) => s.memories)
  const reload = useRules((s) => s.loadMemories)
  const [adding, setAdding] = useState(false)
  const [draft, setDraft] = useState('')
  useEffect(() => { initRulesStore() }, [])

  const add = async () => {
    if (!draft.trim()) return
    try { await api.memories.add(draft.trim()); setDraft(''); setAdding(false); await reload() } catch (e) { notify('error', 'Could not add memory', errorMessage(e)) }
  }
  return (
    <Section title="Memories" description="Facts the agent saved about this project (via its update_memory tool) or that you added. Enabled memories are included in every prompt." actions={<Button onClick={() => setAdding(true)}><Plus size={12} />Add memory</Button>}>
      {adding && (
        <div className="p-3 space-y-2">
          <TextArea autoFocus value={draft} onChange={setDraft} rows={3} placeholder="e.g. We use pnpm, not npm. Tests live next to source files as *.test.ts." />
          <div className="flex justify-end gap-2"><Button onClick={() => { setAdding(false); setDraft('') }}>Cancel</Button><Button variant="primary" onClick={add} disabled={!draft.trim()}>Save</Button></div>
        </div>
      )}
      {memories.length === 0 && !adding && <Empty icon={<Database size={24} />} title="No memories yet" description="As you work, the agent can call update_memory to remember conventions, decisions and preferences for this workspace. You can also add them manually." />}
      {memories.map((m) => <MemoryRow key={m.id} m={m} onChanged={reload} />)}
    </Section>
  )
}

function MemoryRow({ m, onChanged }: { m: MemoryItem; onChanged: () => Promise<void> }) {
  const [editing, setEditing] = useState(false)
  const [text, setText] = useState(m.text)
  const save = async () => { try { await api.memories.update(m.id, text); setEditing(false); await onChanged() } catch (e) { notify('error', 'Update failed', errorMessage(e)) } }
  const del = async () => { if (!(await confirmDialog({ title: 'Delete memory?', message: m.text, danger: true, confirmLabel: 'Delete' }))) return; try { await api.memories.delete(m.id); await onChanged() } catch (e) { notify('error', 'Delete failed', errorMessage(e)) } }
  return (
    <div className="px-3 py-2.5 flex gap-3 items-start">
      <Toggle checked={m.enabled} onChange={async (v) => { try { await api.memories.toggle(m.id, v); await onChanged() } catch (e) { notify('error', 'Toggle failed', errorMessage(e)) } }} label="Enabled" />
      <div className="flex-1 min-w-0">
        {editing ? (
          <div className="space-y-1"><TextArea value={text} onChange={setText} rows={3} autoFocus /><div className="flex gap-1 justify-end"><Button size="sm" onClick={() => { setEditing(false); setText(m.text) }}><X size={11} />Cancel</Button><Button size="sm" variant="primary" onClick={save}><Check size={11} />Save</Button></div></div>
        ) : (
          <>
            {m.title && <div className="text-xs font-medium">{m.title}</div>}
            <div className="text-xs whitespace-pre-wrap leading-relaxed">{m.text}</div>
            <div className="text-[10px] text-muted mt-1 flex items-center gap-2"><Badge tone={m.source === 'agent' ? 'accent' : 'default'}>{m.source}</Badge>{relativeTime(m.createdAt)}</div>
          </>
        )}
      </div>
      {!editing && <div className="flex gap-0.5 shrink-0"><IconButton title="Edit" onClick={() => setEditing(true)}><Pencil size={12} /></IconButton><IconButton title="Delete" onClick={del}><Trash2 size={12} /></IconButton></div>}
    </div>
  )
}

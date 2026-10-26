import { useEffect, useMemo, useState } from 'react'
import { Bot, Pencil, Plus, Trash2, Wrench } from 'lucide-react'
import { useSettings } from '@/store/settings'
import { api } from '@/lib/api'
import type { CustomMode } from '@/types'
import { fieldsForPage } from '@/lib/settingsSchema'
import { SchemaPage } from '../SchemaPage'
import { ModelSelect } from '../FieldControl'
import { SandboxSection } from './SandboxSection'
import { Section, Button, IconButton, Toggle, TextInput, TextArea, Select, Badge, Dialog, Field, Empty, confirmDialog, notify, errorMessage, cn } from '../primitives'

type ToolDesc = { name: string; displayName: string; description: string; group: string; readonly: boolean }

export function AgentPage() {
  const settings = useSettings((s) => s.settings)
  const update = useSettings((s) => s.update)
  const [tools, setTools] = useState<ToolDesc[]>([])
  const [dialog, setDialog] = useState<{ open: boolean; mode?: CustomMode }>({ open: false })
  const [secretBrave, setSecretBrave] = useState(''); const [secretTavily, setSecretTavily] = useState('')
  useEffect(() => { api.chat.toolCatalog().then(setTools).catch(() => setTools([])) }, [])
  const modes = settings?.customModes ?? []
  const provider = settings?.agent.webSearchProvider
  const saveModes = (list: CustomMode[]) => update({ customModes: list } as never)
  const fields = useMemo(() => fieldsForPage('agent').filter((f) => !['agent.defaultModelId', 'agent.subagentModelId'].includes(f.id)), [])
  const groups = useMemo(() => { const m = new Map<string, ToolDesc[]>(); for (const t of tools) { if (!m.has(t.group)) m.set(t.group, []); m.get(t.group)!.push(t) } return [...m.entries()] }, [tools])

  const saveSecret = async (id: 'brave' | 'tavily', v: string) => { try { await api.settings.setSecret(id, v); notify('success', `${id} key saved`); if (id === 'brave') setSecretBrave(''); else setSecretTavily('') } catch (e) { notify('error', 'Could not save key', errorMessage(e)) } }

  return (
    <SchemaPage page="agent" fields={fields}
      before={
        <Section title="Model defaults" description="Models for new chats and subagents (also on the Models page).">
          <ModelRow path="agent.defaultModelId" title="Default model" description="Used for new chats and by Auto." />
          <ModelRow path="agent.subagentModelId" title="Subagent model" description="Used by the task tool. Empty uses the session model." />
        </Section>
      }
      after={
        <>
          <SandboxSection />
          {(provider === 'brave' || provider === 'tavily') && (
            <Section title={`${provider === 'brave' ? 'Brave Search' : 'Tavily'} API key`}>
              <div className="px-3 py-2.5 flex gap-2 max-w-[520px]"><TextInput mono type="password" value={provider === 'brave' ? secretBrave : secretTavily} onChange={provider === 'brave' ? setSecretBrave : setSecretTavily} placeholder="Paste API key" /><Button onClick={() => saveSecret(provider, provider === 'brave' ? secretBrave : secretTavily)}>Save</Button></div>
            </Section>
          )}
          <Section title="Custom modes" description="Create modes with their own instructions, tool set and model (e.g. “Reviewer”, “Docs writer”). They appear in the chat mode picker." actions={<Button onClick={() => setDialog({ open: true })}><Plus size={12} />New mode</Button>}>
            {modes.length === 0 && <Empty icon={<Bot size={22} />} title="No custom modes" description="Built-in modes: Agent, Ask, Plan, Debug." />}
            {modes.map((m) => (
              <div key={m.id} className="px-3 py-2 flex items-center gap-3">
                <div className="flex-1 min-w-0"><div className="text-xs font-medium flex items-center gap-2">{m.name}<Badge>{m.tools === 'all' ? 'all tools' : m.tools === 'readonly' ? 'read-only' : `${m.tools.length} tools`}</Badge>{m.autoRun && <Badge tone="warning">auto-run</Badge>}{m.modelId && <Badge tone="info">{m.modelId}</Badge>}</div>{m.description && <div className="text-[11px] text-muted truncate-1">{m.description}</div>}</div>
                <div className="flex gap-0.5"><IconButton title="Edit" onClick={() => setDialog({ open: true, mode: m })}><Pencil size={12} /></IconButton><IconButton title="Delete" onClick={async () => { if (await confirmDialog({ title: `Delete mode “${m.name}”?`, danger: true, confirmLabel: 'Delete' })) await saveModes(modes.filter((x) => x.id !== m.id)) }}><Trash2 size={12} /></IconButton></div>
              </div>
            ))}
          </Section>
          <Section title="Tool catalog" description="Built-in tools available to the agent (MCP tools are listed on the MCP page).">
            {groups.length === 0 && <div className="px-3 py-4 text-xs text-muted">Tool catalog unavailable.</div>}
            {groups.map(([g, list]) => (
              <div key={g} className="px-3 py-2"><div className="text-[10px] uppercase tracking-wide text-faint mb-1 flex items-center gap-1"><Wrench size={10} />{g}</div>
                <div className="grid sm:grid-cols-2 gap-x-4 gap-y-1">{list.map((t) => <div key={t.name} className="text-[11px]"><span className="mono text-fg">{t.name}</span>{t.readonly && <span className="ml-1 text-[9px] text-success">read-only</span>}<div className="text-muted truncate-1">{t.description}</div></div>)}</div>
              </div>
            ))}
          </Section>
          <ModeDialog state={dialog} tools={tools} onClose={() => setDialog({ open: false })} onSave={async (m) => { const exists = modes.some((x) => x.id === m.id); await saveModes(exists ? modes.map((x) => x.id === m.id ? m : x) : [...modes, m]); setDialog({ open: false }) }} />
        </>
      } />
  )
}

function ModelRow({ path, title, description }: { path: 'agent.defaultModelId' | 'agent.subagentModelId'; title: string; description: string }) {
  const value = useSettings((s) => s.settings?.agent[path === 'agent.defaultModelId' ? 'defaultModelId' : 'subagentModelId'])
  const update = useSettings((s) => s.update)
  const key = path === 'agent.defaultModelId' ? 'defaultModelId' : 'subagentModelId'
  return <div className="px-3 py-2.5 flex items-center justify-between gap-4"><div><div className="text-xs font-medium">{title}</div><div className="text-[11px] text-muted">{description}</div></div><ModelSelect value={value} onChange={(v) => void update({ agent: { [key]: v ?? null } } as never)} /></div>
}

function ModeDialog({ state, tools, onClose, onSave }: { state: { open: boolean; mode?: CustomMode }; tools: ToolDesc[]; onClose: () => void; onSave: (m: CustomMode) => Promise<void> }) {
  const editing = !!state.mode
  const blank = (): CustomMode => ({ id: '', name: '', instructions: '', tools: 'all' })
  const [m, setM] = useState<CustomMode>(state.mode ?? blank())
  const [toolMode, setToolMode] = useState<'all' | 'readonly' | 'custom'>('all')
  useEffect(() => { if (state.open) { const mm = state.mode ?? blank(); setM(mm); setToolMode(mm.tools === 'all' ? 'all' : mm.tools === 'readonly' ? 'readonly' : 'custom') } }, [state.open, state.mode])
  const picked = Array.isArray(m.tools) ? new Set(m.tools) : new Set<string>()
  const togglePick = (n: string) => { const s = new Set(picked); if (s.has(n)) s.delete(n); else s.add(n); setM({ ...m, tools: [...s] }) }
  const valid = m.name.trim() && m.instructions.trim()
  return (
    <Dialog open={state.open} onOpenChange={(o) => { if (!o) onClose() }} title={editing ? `Edit mode “${state.mode?.name}”` : 'New custom mode'} width={620} footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!valid} onClick={() => onSave({ ...m, id: editing ? m.id : m.name.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-'), name: m.name.trim(), tools: toolMode === 'custom' ? (Array.isArray(m.tools) ? m.tools : []) : toolMode })}>{editing ? 'Save' : 'Create'}</Button></>}>
      <div className="grid grid-cols-2 gap-x-4">
        <Field label="Name"><TextInput value={m.name} onChange={(v) => setM({ ...m, name: v })} placeholder="Reviewer" autoFocus /></Field>
        <Field label="Icon (lucide name)"><TextInput mono value={m.icon ?? ''} onChange={(v) => setM({ ...m, icon: v || undefined })} placeholder="shield-check" /></Field>
      </div>
      <Field label="Description"><TextInput value={m.description ?? ''} onChange={(v) => setM({ ...m, description: v || undefined })} placeholder="Shown in the mode picker" /></Field>
      <Field label="Instructions" hint="Appended to the system prompt when this mode is active."><TextArea value={m.instructions} onChange={(v) => setM({ ...m, instructions: v })} rows={6} placeholder="Review the diff for correctness, security and style. Do not edit files; report findings with file:line." /></Field>
      <div className="grid grid-cols-2 gap-x-4">
        <Field label="Model"><ModelSelect value={m.modelId} onChange={(v) => setM({ ...m, modelId: v })} allowEmpty="Session model" /></Field>
        <Field label="Auto-run commands"><div className="h-7 flex items-center"><Toggle checked={!!m.autoRun} onChange={(v) => setM({ ...m, autoRun: v })} label="Auto-run" /></div></Field>
      </div>
      <Field label="Tools"><Select value={toolMode} onChange={(v) => setToolMode((v ?? 'all') as typeof toolMode)} options={[{ value: 'all', label: 'All tools' }, { value: 'readonly', label: 'Read-only tools' }, { value: 'custom', label: 'Pick tools…' }]} className="w-full" /></Field>
      {toolMode === 'custom' && (
        <div className="max-h-[200px] overflow-auto rounded border border-border p-1 grid sm:grid-cols-2 gap-0.5">
          {tools.map((t) => <label key={t.name} className={cn('flex items-center gap-2 px-2 h-6 rounded text-[11px] hover:bg-hover', picked.has(t.name) && 'bg-selected')}><input type="checkbox" checked={picked.has(t.name)} onChange={() => togglePick(t.name)} className="accent-[var(--accent)]" /><span className="mono truncate-1">{t.name}</span></label>)}
          {tools.length === 0 && <div className="text-xs text-muted p-2">Tool catalog unavailable — you can still save with a typed list later.</div>}
        </div>
      )}
    </Dialog>
  )
}

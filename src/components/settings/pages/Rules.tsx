import { useEffect, useMemo, useState } from 'react'
import { FileText, Plus, Trash2, ExternalLink, RefreshCw } from 'lucide-react'
import { initRulesStore, useRules } from '@/store/rules'
import { useWorkspace } from '@/store/workspace'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import type { RuleInfo, RuleKind } from '@/types'
import { Section, Row, Button, IconButton, Toggle, TextArea, Badge, Empty, Dialog, Field, TextInput, Select, StringList, confirmDialog, notify, errorMessage, formatBytes } from '../primitives'
import { useField } from '../useField'

const KIND_LABEL: Record<RuleKind, { label: string; tone: 'accent' | 'info' | 'warning' | 'default'; help: string }> = {
  always: { label: 'Always', tone: 'accent', help: 'Included in every chat' },
  auto: { label: 'Auto Attached', tone: 'info', help: 'Included when matching files are referenced' },
  agent: { label: 'Agent Requested', tone: 'warning', help: 'Agent decides based on the description' },
  manual: { label: 'Manual', tone: 'default', help: 'Only when @-mentioned' },
}
const SOURCE_LABEL: Record<RuleInfo['source'], string> = { project: 'Project (.caret/rules)', 'cursor-project': 'Project (.cursor/rules)', nested: 'Nested rules', user: 'User (~/.caret/rules)', legacy: '.cursorrules', 'agents-md': 'AGENTS.md', 'claude-md': 'CLAUDE.md' }

export function RulesPage() {
  const rules = useRules((s) => s.rules)
  const reload = useRules((s) => s.loadRules)
  const ws = useWorkspace((s) => s.workspace)
  const userRules = useField<string>('userRules')
  const [draft, setDraft] = useState(userRules.value ?? '')
  const [open, setOpen] = useState(false)
  useEffect(() => { initRulesStore() }, [])
  useEffect(() => { setDraft(userRules.value ?? '') }, [userRules.value])

  const groups = useMemo(() => {
    const m = new Map<RuleInfo['source'], RuleInfo[]>()
    for (const r of rules) { if (!m.has(r.source)) m.set(r.source, []); m.get(r.source)!.push(r) }
    return [...m.entries()]
  }, [rules])
  const hasAgentsMd = rules.some((r) => r.source === 'agents-md')

  const createAgentsMd = async () => {
    if (!ws) return
    try {
      await api.fs.writeFile(`${ws.root}/AGENTS.md`, `# ${ws.name}\n\nGuidance for AI agents working in this repository.\n\n## Build & test\n\n- \n\n## Conventions\n\n- \n`)
      await reload(); await runCommand('editor.openFile', { path: `${ws.root}/AGENTS.md` })
    } catch (e) { notify('error', 'Could not create AGENTS.md', errorMessage(e)) }
  }

  return (
    <>
      <Section title="User Rules" description="Global instructions applied to every chat and Tab request, in every project. Markdown. Keep them short and specific." actions={<span className="text-[10px] text-muted">{draft.length} chars</span>}>
        <div className="p-3">
          <TextArea value={draft} onChange={setDraft} rows={6} placeholder={'e.g. Reply tersely. Prefer functional TypeScript. Never add comments that restate the code.'} onBlur={() => { if (draft !== (userRules.value ?? '')) void userRules.set(draft) }} />
          <div className="text-[10px] text-muted mt-1">Saved when you leave the field.</div>
        </div>
      </Section>

      <Section title="Project Rules" description="Rules live in .caret/rules/*.mdc (and .cursor/rules for compatibility). Frontmatter controls when they apply: alwaysApply, globs, description." actions={<><IconButton title="Refresh" onClick={() => void reload()}><RefreshCw size={12} /></IconButton><Button onClick={() => setOpen(true)} disabled={!ws}><Plus size={12} />New rule</Button></>}>
        {!ws && <Empty icon={<FileText size={24} />} title="Open a folder to manage project rules" />}
        {ws && rules.length === 0 && <Empty icon={<FileText size={24} />} title="No project rules" description="Create a rule to teach the agent project conventions, or add an AGENTS.md at the repo root." action={<div className="flex gap-2"><Button variant="primary" onClick={() => setOpen(true)}><Plus size={12} />New rule</Button><Button onClick={createAgentsMd}>Add AGENTS.md</Button></div>} />}
        {groups.map(([source, list]) => (
          <div key={source}>
            <div className="px-3 pt-2 pb-1 text-[10px] uppercase tracking-wide text-faint">{SOURCE_LABEL[source] ?? source}</div>
            {list.map((r) => <RuleRow key={r.id} r={r} onChanged={reload} />)}
          </div>
        ))}
        {ws && rules.length > 0 && !hasAgentsMd && <div className="px-3 py-2 flex items-center justify-between text-[11px] text-muted"><span>No AGENTS.md found — many agents (including Caret) read it automatically.</span><Button size="sm" onClick={createAgentsMd}>Add AGENTS.md</Button></div>}
      </Section>

      <Section title="How rules are matched">
        {(Object.keys(KIND_LABEL) as RuleKind[]).map((k) => <Row key={k} title={<span className="flex items-center gap-2"><Badge tone={KIND_LABEL[k].tone}>{KIND_LABEL[k].label}</Badge></span>} description={KIND_LABEL[k].help} />)}
      </Section>

      <NewRuleDialog open={open} onOpenChange={setOpen} onCreated={reload} />
    </>
  )
}

function RuleRow({ r, onChanged }: { r: RuleInfo; onChanged: () => Promise<void> }) {
  const k = KIND_LABEL[r.kind]
  const del = async () => {
    if (!(await confirmDialog({ title: `Delete rule “${r.name}”?`, message: r.path, danger: true, confirmLabel: 'Delete' }))) return
    try { await api.rules.delete(r.id); await onChanged() } catch (e) { notify('error', 'Delete failed', errorMessage(e)) }
  }
  return (
    <div className="px-3 py-2 flex items-center gap-3">
      <Toggle checked={r.enabled} onChange={async (v) => { try { await api.rules.toggle(r.id, v); await onChanged() } catch (e) { notify('error', 'Toggle failed', errorMessage(e)) } }} label="Enabled" />
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2"><span className="text-xs font-medium truncate-1">{r.name}</span><Badge tone={k.tone}>{k.label}</Badge><span className="text-[10px] text-faint">{formatBytes(r.sizeBytes)}</span></div>
        {r.description && <div className="text-[11px] text-muted truncate-1">{r.description}</div>}
        {r.globs.length > 0 && <div className="text-[10px] mono text-faint truncate-1">{r.globs.join(', ')}</div>}
      </div>
      <div className="flex gap-0.5 shrink-0">
        <IconButton title="Open" onClick={() => void runCommand('editor.openFile', { path: r.path })}><ExternalLink size={12} /></IconButton>
        {(r.source === 'project' || r.source === 'user' || r.source === 'nested') && <IconButton title="Delete" onClick={del}><Trash2 size={12} /></IconButton>}
      </div>
    </div>
  )
}

function NewRuleDialog({ open, onOpenChange, onCreated }: { open: boolean; onOpenChange: (o: boolean) => void; onCreated: () => Promise<void> }) {
  const hasWs = useWorkspace((s) => !!s.workspace)
  const [name, setName] = useState('')
  const [kind, setKind] = useState<RuleKind>('always')
  const [description, setDescription] = useState('')
  const [globs, setGlobs] = useState<string[]>([])
  const [content, setContent] = useState('')
  const [scope, setScope] = useState<'project' | 'user'>(hasWs ? 'project' : 'user')
  const [busy, setBusy] = useState(false)
  const valid = name.trim().length > 0 && content.trim().length > 0 && (kind !== 'auto' || globs.length > 0) && (kind !== 'agent' || description.trim().length > 0)
  const create = async () => {
    setBusy(true)
    try {
      const info = await api.rules.create({ name: name.trim(), kind, globs, description: description.trim() || undefined, content, scope })
      onOpenChange(false); setName(''); setContent(''); setGlobs([]); setDescription('')
      await onCreated(); notify('success', 'Rule created', info.path)
      void runCommand('editor.openFile', { path: info.path })
    } catch (e) { notify('error', 'Could not create rule', errorMessage(e)) } finally { setBusy(false) }
  }
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="New rule" description="Saved as <name>.mdc with frontmatter." footer={<><Button onClick={() => onOpenChange(false)}>Cancel</Button><Button variant="primary" disabled={!valid} loading={busy} onClick={create}>Create rule</Button></>}>
      <div className="grid grid-cols-2 gap-x-4">
        <Field label="Name"><TextInput value={name} onChange={setName} placeholder="typescript-style" autoFocus /></Field>
        <Field label="Scope"><Select value={scope} onChange={(v) => setScope((v ?? 'user') as 'project' | 'user')} options={[{ value: 'project', label: 'Project (.caret/rules)' }, { value: 'user', label: 'User (~/.caret/rules)' }]} /></Field>
      </div>
      <Field label="Rule type"><Select value={kind} onChange={(v) => setKind((v ?? 'always') as RuleKind)} options={(Object.keys(KIND_LABEL) as RuleKind[]).map((k) => ({ value: k, label: KIND_LABEL[k].label, description: KIND_LABEL[k].help }))} /></Field>
      {(kind === 'agent' || kind === 'auto' || kind === 'manual') && <Field label="Description" hint={kind === 'agent' ? 'Required — the agent uses this to decide when to fetch the rule.' : undefined}><TextInput value={description} onChange={setDescription} placeholder="When to use this rule" /></Field>}
      {kind === 'auto' && <Field label="File globs" hint="Rule attaches when referenced files match."><StringList value={globs} onChange={setGlobs} placeholder="src/**/*.tsx" /></Field>}
      <Field label="Content (markdown)"><TextArea value={content} onChange={setContent} rows={8} mono placeholder={'- Use early returns\n- Prefer named exports\n- Reference @src/utils/example.ts for patterns'} /></Field>
      <div className="flex items-center gap-2 text-[11px] text-muted"><FileText size={12} />Preview of frontmatter: <code className="mono">{`description: ${description || '—'} · globs: ${globs.join(',') || '—'} · alwaysApply: ${kind === 'always'}`}</code></div>
    </Dialog>
  )
}

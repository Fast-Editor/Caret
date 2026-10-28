import { useEffect, useMemo, useState } from 'react'
import { Brain, Check, Download, Eye, EyeOff, Pencil, Plus, Trash2, Wand2, Zap, KeyRound } from 'lucide-react'
import { useSettings } from '@/store/settings'
import { api } from '@/lib/api'
import type { ModelConfig, ProviderConfig, ProviderKind } from '@/types'
import { fieldsForPage } from '@/lib/settingsSchema'
import { FieldControl } from '../FieldControl'
import { AutoRoutingSection } from './AutoRouting'
import { Section, Row, Button, IconButton, Toggle, TextInput, NumberInput, Select, Badge, Dialog, Field, KeyValueEditor, confirmDialog, notify, errorMessage, Tooltip, cn } from '../primitives'

const KINDS: { value: ProviderKind; label: string; description: string; defaultBase?: string; auth: 'x-api-key' | 'bearer' | 'api-key' }[] = [
  { value: 'anthropic', label: 'Anthropic', description: 'Claude via the Messages API', auth: 'x-api-key' },
  { value: 'anthropic-compatible', label: 'Anthropic-compatible', description: 'Messages API at a custom base URL (Foundry, proxies)', auth: 'x-api-key' },
  { value: 'openai', label: 'OpenAI', description: 'GPT models', auth: 'bearer' },
  { value: 'openai-compatible', label: 'OpenAI-compatible', description: 'Any /v1/chat/completions server (LM Studio, vLLM, Lynkr, llama.cpp…)', defaultBase: 'http://localhost:8080/v1', auth: 'bearer' },
  { value: 'azure-openai', label: 'Azure OpenAI', description: 'Resource endpoint + api-version', auth: 'api-key' },
  { value: 'ollama', label: 'Ollama', description: 'Local models', defaultBase: 'http://localhost:11434/v1', auth: 'bearer' },
  { value: 'openrouter', label: 'OpenRouter', description: 'Many providers behind one key', defaultBase: 'https://openrouter.ai/api/v1', auth: 'bearer' },
  { value: 'gemini', label: 'Google Gemini', description: 'OpenAI-compatible Gemini endpoint', defaultBase: 'https://generativelanguage.googleapis.com/v1beta/openai', auth: 'bearer' },
]

export function ModelsPage() {
  const settings = useSettings((s) => s.settings)
  const update = useSettings((s) => s.update)
  const [secrets, setSecrets] = useState<string[]>([])
  const [providerDialog, setProviderDialog] = useState<{ open: boolean; provider?: ProviderConfig }>({ open: false })
  const [modelDialog, setModelDialog] = useState<{ open: boolean; model?: ModelConfig; providerId?: string }>({ open: false })
  const [fetchDialog, setFetchDialog] = useState<{ open: boolean; provider?: ProviderConfig; models: string[]; loading: boolean }>({ open: false, models: [], loading: false })
  const refreshSecrets = () => api.settings.listSecrets().then(setSecrets).catch(() => {})
  useEffect(() => { void refreshSecrets() }, [])

  const providers = settings?.providers ?? []
  const models = settings?.models ?? []
  const saveProviders = (list: ProviderConfig[]) => update({ providers: list } as never)
  const saveModels = (list: ModelConfig[]) => update({ models: list } as never)

  const removeProvider = async (p: ProviderConfig) => {
    const dependents = models.filter((m) => m.providerId === p.id)
    if (!(await confirmDialog({ title: `Remove provider “${p.name}”?`, message: dependents.length ? `${dependents.length} model(s) using it will also be removed.` : undefined, danger: true, confirmLabel: 'Remove' }))) return
    await saveProviders(providers.filter((x) => x.id !== p.id)); await saveModels(models.filter((m) => m.providerId !== p.id))
  }
  const fetchModels = async (p: ProviderConfig) => {
    setFetchDialog({ open: true, provider: p, models: [], loading: true })
    try { const ids = await api.settings.listRemoteModels(p.id); setFetchDialog({ open: true, provider: p, models: ids, loading: false }) } catch (e) { setFetchDialog({ open: false, models: [], loading: false }); notify('error', `Could not list models for ${p.name}`, errorMessage(e)) }
  }
  const setDefault = async (id: string) => saveModels(models.map((m) => ({ ...m, isDefault: m.id === id })))
  const testModel = async (m: ModelConfig) => {
    notify('info', `Testing ${m.displayName}…`)
    try { const r = await api.settings.testModel(m.id); notify(r.ok ? 'success' : 'error', r.ok ? `${m.displayName}: OK in ${r.latencyMs} ms` : `${m.displayName} failed`, r.message) } catch (e) { notify('error', `${m.displayName} failed`, errorMessage(e)) }
  }
  const defaultsFields = fieldsForPage('agent').filter((f) => ['agent.defaultModelId', 'agent.subagentModelId'].includes(f.id)).concat(fieldsForPage('tab').filter((f) => f.id === 'tab.modelId'), fieldsForPage('indexing').filter((f) => ['indexing.embeddingsProviderId', 'indexing.embeddingsModel'].includes(f.id)))

  return (
    <>
      <Section title="Providers" description="Where requests go. Each provider needs a base URL (except first-party) and usually an API key." actions={<Button variant="primary" onClick={() => setProviderDialog({ open: true })}><Plus size={12} />Add provider</Button>}>
        {providers.length === 0 && <div className="px-3 py-6 text-center text-xs text-muted">No providers configured.</div>}
        {providers.map((p) => <ProviderCard key={p.id} p={p} hasKey={!!p.apiKeyRef && secrets.includes(p.apiKeyRef)} modelCount={models.filter((m) => m.providerId === p.id).length}
          onToggle={(v) => saveProviders(providers.map((x) => x.id === p.id ? { ...x, enabled: v } : x))}
          onEdit={() => setProviderDialog({ open: true, provider: p })} onRemove={() => removeProvider(p)} onFetch={() => fetchModels(p)} onAddModel={() => setModelDialog({ open: true, providerId: p.id })} onKeySaved={refreshSecrets} />)}
      </Section>

      <Section title="Models" description="The model picker shows enabled models. Pick one as the default used by “Auto”." actions={<Button onClick={() => setModelDialog({ open: true })}><Plus size={12} />Add model</Button>}>
        <div className="overflow-x-auto">
          <table className="w-full text-xs">
            <thead className="text-[10px] uppercase tracking-wide text-faint text-left">
              <tr><th className="px-3 py-1.5 font-medium">Default</th><th className="px-2 py-1.5 font-medium">Model</th><th className="px-2 py-1.5 font-medium">Provider</th><th className="px-2 py-1.5 font-medium text-right">Context</th><th className="px-2 py-1.5 font-medium text-right">Max out</th><th className="px-2 py-1.5 font-medium">Capabilities</th><th className="px-2 py-1.5 font-medium text-right">$ in / out</th><th className="px-2 py-1.5 font-medium">On</th><th className="px-2 py-1.5"></th></tr>
            </thead>
            <tbody>
              {models.map((m) => {
                const p = providers.find((x) => x.id === m.providerId)
                return (
                  <tr key={m.id} className="border-t border-border hover:bg-hover/40">
                    <td className="px-3 py-1.5"><button aria-label="Set as default" onClick={() => void setDefault(m.id)} className={cn('h-3.5 w-3.5 rounded-full border inline-flex items-center justify-center', m.isDefault ? 'border-accent bg-accent' : 'border-border-strong')}>{m.isDefault && <Check size={9} className="text-accent-fg" />}</button></td>
                    <td className="px-2 py-1.5"><div className="font-medium flex items-center gap-1.5">{m.displayName}{m.isFast && <Tooltip label="Fast model (good for Tab)"><Zap size={11} className="text-warning" /></Tooltip>}</div><div className="mono text-[10px] text-faint">{m.model}</div></td>
                    <td className="px-2 py-1.5 text-muted">{p?.name ?? <span className="text-danger">{m.providerId} (missing)</span>}</td>
                    <td className="px-2 py-1.5 text-right mono">{fmtK(m.contextWindow)}</td>
                    <td className="px-2 py-1.5 text-right mono">{fmtK(m.maxOutput)}</td>
                    <td className="px-2 py-1.5"><div className="flex gap-1 flex-wrap">{m.supportsTools && <Badge>tools</Badge>}{m.supportsVision && <Badge>vision</Badge>}{m.supportsThinking && <Badge>thinking</Badge>}{m.supportsEffort && <Badge>effort</Badge>}</div></td>
                    <td className="px-2 py-1.5 text-right mono text-muted">{m.inputPricePerM != null ? `${m.inputPricePerM} / ${m.outputPricePerM ?? '—'}` : '—'}</td>
                    <td className="px-2 py-1.5"><Toggle checked={m.enabled} onChange={(v) => saveModels(models.map((x) => x.id === m.id ? { ...x, enabled: v } : x))} label="Enabled" /></td>
                    <td className="px-2 py-1.5"><div className="flex gap-0.5 justify-end"><IconButton title="Test" onClick={() => testModel(m)}><Wand2 size={12} /></IconButton><IconButton title="Edit" onClick={() => setModelDialog({ open: true, model: m })}><Pencil size={12} /></IconButton><IconButton title="Remove" onClick={async () => { if (await confirmDialog({ title: `Remove ${m.displayName}?`, danger: true, confirmLabel: 'Remove' })) await saveModels(models.filter((x) => x.id !== m.id)) }}><Trash2 size={12} /></IconButton></div></td>
                  </tr>
                )
              })}
              {models.length === 0 && <tr><td colSpan={9} className="px-3 py-6 text-center text-muted">No models. Add one or fetch from a provider.</td></tr>}
            </tbody>
          </table>
        </div>
      </Section>

      <Section title="Defaults" description="Which model each feature uses.">
        {defaultsFields.map((f) => <FieldControl key={f.id} field={f} />)}
      </Section>

      <AutoRoutingSection />

      <ProviderDialog state={providerDialog} onClose={() => setProviderDialog({ open: false })} onSave={async (p) => { const exists = providers.some((x) => x.id === p.id); await saveProviders(exists ? providers.map((x) => x.id === p.id ? p : x) : [...providers, p]); setProviderDialog({ open: false }) }} />
      <ModelDialog state={modelDialog} providers={providers} onClose={() => setModelDialog({ open: false })} onSave={async (m) => { const exists = models.some((x) => x.id === m.id); await saveModels(exists ? models.map((x) => x.id === m.id ? m : x) : [...models, m]); setModelDialog({ open: false }) }} />
      <FetchModelsDialog state={fetchDialog} existing={models} onClose={() => setFetchDialog({ open: false, models: [], loading: false })} onAdd={async (list) => { await saveModels([...models, ...list]); setFetchDialog({ open: false, models: [], loading: false }); notify('success', `Added ${list.length} model${list.length === 1 ? '' : 's'}`) }} />
    </>
  )
}

function fmtK(n: number): string { return n >= 1_000_000 ? `${(n / 1_000_000).toFixed(n % 1_000_000 ? 1 : 0)}M` : n >= 1000 ? `${Math.round(n / 1000)}K` : String(n) }

function ProviderCard({ p, hasKey, modelCount, onToggle, onEdit, onRemove, onFetch, onAddModel, onKeySaved }: { p: ProviderConfig; hasKey: boolean; modelCount: number; onToggle: (v: boolean) => void; onEdit: () => void; onRemove: () => void; onFetch: () => void; onAddModel: () => void; onKeySaved: () => void }) {
  const [key, setKey] = useState('')
  const [show, setShow] = useState(false)
  const [saving, setSaving] = useState(false)
  const kind = KINDS.find((k) => k.value === p.kind)
  const needsKey = p.kind !== 'ollama'
  const saveKey = async () => {
    if (!p.apiKeyRef) return
    setSaving(true)
    try { await api.settings.setSecret(p.apiKeyRef, key); setKey(''); onKeySaved(); notify('success', key ? 'API key saved' : 'API key removed') } catch (e) { notify('error', 'Could not save key', errorMessage(e)) } finally { setSaving(false) }
  }
  return (
    <div className="px-3 py-3">
      <div className="flex items-start gap-3">
        <Toggle checked={p.enabled} onChange={onToggle} label="Enabled" />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 flex-wrap">
            <span className="text-xs font-semibold">{p.name}</span>
            <Badge>{kind?.label ?? p.kind}</Badge>
            {needsKey && (hasKey ? <Badge tone="success">key set</Badge> : <Badge tone="warning">no key</Badge>)}
            <span className="text-[10px] text-muted">{modelCount} model{modelCount === 1 ? '' : 's'}</span>
          </div>
          <div className="text-[11px] text-muted mono truncate-1">{p.baseUrl ?? (p.kind === 'anthropic' ? 'https://api.anthropic.com' : p.kind === 'openai' ? 'https://api.openai.com/v1' : '—')}</div>
          {needsKey && (
            <div className="mt-2 flex items-center gap-1 max-w-[520px]">
              <div className="relative flex-1"><KeyRound size={12} className="absolute left-2 top-1/2 -translate-y-1/2 text-muted" /><TextInput mono type={show ? 'text' : 'password'} value={key} onChange={setKey} placeholder={hasKey ? '•••••••• (saved) — paste to replace' : `Paste ${p.name} API key`} className="pl-7 pr-7" onKeyDown={(e) => { if (e.key === 'Enter') void saveKey() }} /><button onClick={() => setShow(!show)} aria-label={show ? 'Hide' : 'Show'} className="absolute right-1.5 top-1/2 -translate-y-1/2 text-muted hover:text-fg">{show ? <EyeOff size={12} /> : <Eye size={12} />}</button></div>
              <Button size="md" onClick={saveKey} loading={saving} disabled={!key && !hasKey}>{key ? 'Save' : hasKey ? 'Clear' : 'Save'}</Button>
            </div>
          )}
        </div>
        <div className="flex gap-0.5 shrink-0">
          <Tooltip label="Fetch available models from the provider"><Button size="sm" onClick={onFetch}><Download size={11} />Fetch models</Button></Tooltip>
          <IconButton title="Add model" onClick={onAddModel}><Plus size={12} /></IconButton>
          <IconButton title="Edit provider" onClick={onEdit}><Pencil size={12} /></IconButton>
          <IconButton title="Remove provider" onClick={onRemove}><Trash2 size={12} /></IconButton>
        </div>
      </div>
    </div>
  )
}

function slug(s: string): string { return s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || 'provider' }

function ProviderDialog({ state, onClose, onSave }: { state: { open: boolean; provider?: ProviderConfig }; onClose: () => void; onSave: (p: ProviderConfig) => Promise<void> }) {
  const editing = !!state.provider
  const [p, setP] = useState<ProviderConfig>(() => state.provider ?? blankProvider())
  useEffect(() => { if (state.open) setP(state.provider ?? blankProvider()) }, [state.open, state.provider])
  const kind = KINDS.find((k) => k.value === p.kind)
  const setKind = (v: ProviderKind) => { const k = KINDS.find((x) => x.value === v)!; setP((cur) => ({ ...cur, kind: v, baseUrl: cur.baseUrl || k.defaultBase, authStyle: k.auth })) }
  const valid = p.name.trim() && (p.kind === 'anthropic' || p.kind === 'openai' || !!p.baseUrl?.trim())
  return (
    <Dialog open={state.open} onOpenChange={(o) => { if (!o) onClose() }} title={editing ? `Edit ${state.provider?.name}` : 'Add provider'} footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!valid} onClick={() => onSave({ ...p, id: editing ? p.id : uniqueId(slug(p.name)), apiKeyRef: p.apiKeyRef || (editing ? p.id : uniqueId(slug(p.name))), name: p.name.trim(), baseUrl: p.baseUrl?.trim() || undefined })}>{editing ? 'Save' : 'Add'}</Button></>}>
      <Field label="Type"><Select value={p.kind} onChange={(v) => v && setKind(v)} options={KINDS.map((k) => ({ value: k.value, label: k.label, description: k.description }))} className="w-full" /></Field>
      <Field label="Display name"><TextInput value={p.name} onChange={(v) => setP({ ...p, name: v })} placeholder={kind?.label} autoFocus /></Field>
      <Field label="Base URL" hint={p.kind === 'anthropic' ? 'Leave empty for api.anthropic.com' : p.kind === 'openai' ? 'Leave empty for api.openai.com' : p.kind === 'azure-openai' ? 'https://<resource>.openai.azure.com/openai/deployments/<deployment>' : undefined}><TextInput mono value={p.baseUrl ?? ''} onChange={(v) => setP({ ...p, baseUrl: v })} placeholder={kind?.defaultBase ?? 'https://…'} /></Field>
      {p.kind === 'azure-openai' && <Field label="API version"><TextInput mono value={p.apiVersion ?? ''} onChange={(v) => setP({ ...p, apiVersion: v })} placeholder="2024-10-21" /></Field>}
      <div className="grid grid-cols-2 gap-x-4">
        <Field label="Auth header"><Select value={p.authStyle ?? kind?.auth ?? 'bearer'} onChange={(v) => setP({ ...p, authStyle: v as ProviderConfig['authStyle'] })} options={[{ value: 'bearer', label: 'Authorization: Bearer' }, { value: 'x-api-key', label: 'x-api-key' }, { value: 'api-key', label: 'api-key' }]} className="w-full" /></Field>
        {(p.kind === 'anthropic' || p.kind === 'anthropic-compatible') && <Field label="Refusal fallbacks" hint="Server-side fallback to another Claude model when a request is declined (first-party API only)."><div className="h-7 flex items-center"><Toggle checked={p.enableFallbacks ?? p.kind === 'anthropic'} onChange={(v) => setP({ ...p, enableFallbacks: v })} label="Enable fallbacks" /></div></Field>}
      </div>
      <Field label="Extra headers" hint="Sent with every request (e.g. HTTP-Referer for OpenRouter)."><KeyValueEditor value={p.headers ?? {}} onChange={(h) => setP({ ...p, headers: Object.keys(h).length ? h : undefined })} keyPlaceholder="Header" valuePlaceholder="Value" /></Field>
    </Dialog>
  )
}
function blankProvider(): ProviderConfig { return { id: '', kind: 'openai-compatible', name: '', baseUrl: 'http://localhost:8080/v1', enabled: true, authStyle: 'bearer' } }
function uniqueId(base: string): string { return `${base}-${Math.random().toString(36).slice(2, 6)}` }

function ModelDialog({ state, providers, onClose, onSave }: { state: { open: boolean; model?: ModelConfig; providerId?: string }; providers: ProviderConfig[]; onClose: () => void; onSave: (m: ModelConfig) => Promise<void> }) {
  const editing = !!state.model
  const blank = (): ModelConfig => ({ id: '', providerId: state.providerId ?? providers[0]?.id ?? '', model: '', displayName: '', contextWindow: 128000, maxOutput: 16000, supportsTools: true, supportsVision: false, supportsThinking: false, supportsEffort: false, enabled: true })
  const [m, setM] = useState<ModelConfig>(() => state.model ?? blank())
  useEffect(() => { if (state.open) setM(state.model ?? blank()) }, [state.open, state.model, state.providerId]) // eslint-disable-line react-hooks/exhaustive-deps
  const valid = m.model.trim() && m.providerId
  const num = (k: keyof ModelConfig) => (v: number) => setM({ ...m, [k]: v })
  const optNum = (k: keyof ModelConfig) => (v: string) => setM({ ...m, [k]: v === '' ? undefined : Number(v) })
  return (
    <Dialog open={state.open} onOpenChange={(o) => { if (!o) onClose() }} title={editing ? `Edit ${state.model?.displayName}` : 'Add model'} width={600} footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!valid} onClick={() => onSave({ ...m, id: editing ? m.id : uniqueModelId(m), displayName: m.displayName.trim() || m.model.trim(), model: m.model.trim() })}>{editing ? 'Save' : 'Add'}</Button></>}>
      <div className="grid grid-cols-2 gap-x-4">
        <Field label="Provider"><Select value={m.providerId} onChange={(v) => v && setM({ ...m, providerId: v })} options={providers.map((p) => ({ value: p.id, label: p.name }))} className="w-full" /></Field>
        <Field label="Model id" hint="As the provider expects it."><TextInput mono value={m.model} onChange={(v) => setM({ ...m, model: v })} placeholder="claude-opus-5-5" autoFocus /></Field>
        <Field label="Display name"><TextInput value={m.displayName} onChange={(v) => setM({ ...m, displayName: v })} placeholder="Claude Opus 5.5" /></Field>
        <Field label="Tags"><TextInput value={(m.tags ?? []).join(', ')} onChange={(v) => setM({ ...m, tags: v.split(',').map((s) => s.trim()).filter(Boolean) })} placeholder="fast, cheap" /></Field>
        <Field label="Context window (tokens)"><NumberInput value={m.contextWindow} onChange={num('contextWindow')} min={1000} step={1000} /></Field>
        <Field label="Max output (tokens)"><NumberInput value={m.maxOutput} onChange={num('maxOutput')} min={256} step={256} /></Field>
        <Field label="Input $/M tokens"><TextInput mono value={m.inputPricePerM?.toString() ?? ''} onChange={optNum('inputPricePerM')} placeholder="—" /></Field>
        <Field label="Output $/M tokens"><TextInput mono value={m.outputPricePerM?.toString() ?? ''} onChange={optNum('outputPricePerM')} placeholder="—" /></Field>
        <Field label="Cache read $/M"><TextInput mono value={m.cacheReadPricePerM?.toString() ?? ''} onChange={optNum('cacheReadPricePerM')} placeholder="—" /></Field>
        <Field label="Cache write $/M"><TextInput mono value={m.cacheWritePricePerM?.toString() ?? ''} onChange={optNum('cacheWritePricePerM')} placeholder="—" /></Field>
      </div>
      <div className="grid grid-cols-3 gap-2 mt-1">
        {([['supportsTools', 'Tool use'], ['supportsVision', 'Vision'], ['supportsThinking', 'Thinking'], ['supportsEffort', 'Effort levels'], ['isFast', 'Fast (Tab)'], ['enabled', 'Enabled']] as [keyof ModelConfig, string][]).map(([k, label]) => (
          <label key={k} className="flex items-center justify-between gap-2 text-[11px] px-2 h-7 rounded border border-border bg-input"><span>{label}</span><Toggle checked={!!m[k]} onChange={(v) => setM({ ...m, [k]: v })} label={label} /></label>
        ))}
      </div>
    </Dialog>
  )
}
function uniqueModelId(m: ModelConfig): string { return `${m.providerId}/${m.model.trim()}`.replace(/\s+/g, '-') }

function FetchModelsDialog({ state, existing, onClose, onAdd }: { state: { open: boolean; provider?: ProviderConfig; models: string[]; loading: boolean }; existing: ModelConfig[]; onClose: () => void; onAdd: (list: ModelConfig[]) => Promise<void> }) {
  const [picked, setPicked] = useState<Set<string>>(new Set())
  const [filter, setFilter] = useState('')
  useEffect(() => { if (state.open) { setPicked(new Set()); setFilter('') } }, [state.open])
  const have = useMemo(() => new Set(existing.filter((m) => m.providerId === state.provider?.id).map((m) => m.model)), [existing, state.provider])
  const list = state.models.filter((id) => id.toLowerCase().includes(filter.toLowerCase()))
  const toggle = (id: string) => setPicked((s) => { const n = new Set(s); if (n.has(id)) n.delete(id); else n.add(id); return n })
  const add = () => onAdd([...picked].map((id) => ({ id: `${state.provider!.id}/${id}`, providerId: state.provider!.id, model: id, displayName: prettyModelName(id), contextWindow: guessContext(id), maxOutput: 16000, supportsTools: true, supportsVision: /gpt-4o|gpt-5|claude|gemini|vision|llava|pixtral/i.test(id), supportsThinking: /claude-(opus|sonnet|fable)|o[1-4]|gpt-5|thinking|reason|r1|qwq/i.test(id), supportsEffort: /claude-(opus|sonnet|fable)-[45]/i.test(id), enabled: true })))
  return (
    <Dialog open={state.open} onOpenChange={(o) => { if (!o) onClose() }} title={`Models from ${state.provider?.name ?? ''}`} description="Select models to add to the catalog. You can edit capabilities and prices afterwards." footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!picked.size} onClick={add}>Add {picked.size || ''} model{picked.size === 1 ? '' : 's'}</Button></>}>
      {state.loading && <div className="py-8 text-center text-xs text-muted flex items-center justify-center gap-2"><Brain size={14} className="pulse" />Fetching model list…</div>}
      {!state.loading && (
        <>
          <TextInput value={filter} onChange={setFilter} placeholder="Filter…" className="mb-2" autoFocus />
          <div className="max-h-[44vh] overflow-auto rounded border border-border divide-y divide-border">
            {list.map((id) => { const dup = have.has(id); return (
              <label key={id} className={cn('flex items-center gap-2 px-2 h-7 text-xs cursor-pointer', dup ? 'opacity-50' : 'hover:bg-hover')}>
                <input type="checkbox" disabled={dup} checked={picked.has(id)} onChange={() => toggle(id)} className="accent-[var(--accent)]" />
                <span className="mono truncate-1 flex-1">{id}</span>{dup && <Badge>added</Badge>}
              </label>) })}
            {list.length === 0 && <div className="px-2 py-4 text-center text-xs text-muted">No models returned.</div>}
          </div>
        </>
      )}
    </Dialog>
  )
}
function prettyModelName(id: string): string { return id.split('/').pop()!.replace(/[-_]/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase()) }
function guessContext(id: string): number { if (/claude-(opus|sonnet|fable)-[45]|gemini|gpt-5|gpt-4\.1/i.test(id)) return 1_000_000; if (/claude|o[1-4]|gpt-4o|deepseek|qwen|llama-?3\.[1-9]/i.test(id)) return 200_000; return 128_000 }

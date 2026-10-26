import { useMemo } from 'react'
import { useSettings } from '@/store/settings'
import { getPath, type SettingField } from '@/lib/settingsSchema'
import { useField } from './useField'
import { Row, Toggle, Select, NumberInput, TextInput, TextArea, StringList, KeyValueEditor, cn } from './primitives'
import type { ThemeId } from '@/types'

export const THEMES: { id: ThemeId; label: string; swatch: string[] }[] = [
  { id: 'caret-dark', label: 'Caret Dark', swatch: ['#141414', '#1a1a1a', '#ececec', '#cccccc'] },
  { id: 'caret-midnight', label: 'Caret Midnight', swatch: ['#0d1117', '#0f141b', '#79b8ff', '#d7dee8'] },
  { id: 'caret-light', label: 'Caret Light', swatch: ['#f6f6f8', '#ffffff', '#2f6fed', '#1f2328'] },
  { id: 'caret-high-contrast', label: 'High Contrast', swatch: ['#000000', '#0a0a0a', '#4cc2ff', '#ffffff'] },
]

export function ModelSelect({ value, onChange, allowEmpty = 'Default model', filter }: { value: string | undefined; onChange: (v: string | undefined) => void; allowEmpty?: string; filter?: 'fast' | 'vision' | 'tools' }) {
  const models = useSettings((s) => s.settings?.models ?? [])
  const providers = useSettings((s) => s.settings?.providers ?? [])
  const options = useMemo(() => models.filter((m) => m.enabled).filter((m) => !filter || (filter === 'fast' ? true : filter === 'vision' ? m.supportsVision : m.supportsTools)).map((m) => ({
    value: m.id, label: m.displayName || m.model, description: `${providers.find((p) => p.id === m.providerId)?.name ?? m.providerId}${m.isFast ? ' · fast' : ''}`,
  })), [models, providers, filter])
  return <Select value={value} onChange={onChange} options={options} allowEmpty={allowEmpty} placeholder="Select model" className="min-w-[220px]" />
}

export function ProviderSelect({ value, onChange, allowEmpty = 'None', kinds }: { value: string | undefined; onChange: (v: string | undefined) => void; allowEmpty?: string; kinds?: string[] }) {
  const providers = useSettings((s) => s.settings?.providers ?? [])
  const options = providers.filter((p) => p.enabled && (!kinds || kinds.includes(p.kind))).map((p) => ({ value: p.id, label: p.name, description: p.baseUrl ?? p.kind }))
  return <Select value={value} onChange={onChange} options={options} allowEmpty={allowEmpty} placeholder="Select provider" className="min-w-[220px]" />
}

export function ThemePicker({ value, onChange }: { value: ThemeId; onChange: (v: ThemeId) => void }) {
  return (
    <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 w-full">
      {THEMES.map((t) => (
        <button key={t.id} onClick={() => onChange(t.id)} className={cn('rounded-md border p-2 text-left hover:bg-hover transition-colors', value === t.id ? 'border-accent ring-1 ring-accent' : 'border-border')}>
          <div className="flex h-8 rounded overflow-hidden border border-border mb-1.5">
            {t.swatch.map((c, i) => <div key={i} style={{ background: c }} className="flex-1" />)}
          </div>
          <div className="text-[11px] font-medium">{t.label}</div>
        </button>
      ))}
    </div>
  )
}

/** Render one schema-driven field bound to the settings store. */
export function FieldControl({ field }: { field: SettingField }) {
  const { value, set, reset, modified } = useField(field)
  const dep = useSettings((s) => (field.dependsOn ? getPath(s.settings, field.dependsOn) : true))
  if (field.dependsOn && !dep) return null
  const vertical = field.type === 'stringList' || field.type === 'textarea' || field.type === 'keyValue' || field.type === 'theme'
  let control: React.ReactNode = null
  switch (field.type) {
    case 'boolean': control = <Toggle checked={!!value} onChange={(v) => set(v)} label={field.title} />; break
    case 'number': control = <NumberInput value={typeof value === 'number' ? value : Number(field.min ?? 0)} onChange={(v) => set(v)} min={field.min} max={field.max} step={field.step} unit={field.unit} />; break
    case 'string': control = <TextInput value={(value as string | undefined) ?? ''} onChange={(v) => set(field.optional && v === '' ? undefined : v)} placeholder={field.placeholder} className="min-w-[260px]" mono />; break
    case 'select': control = <Select value={value as string | undefined} onChange={(v) => set(v)} options={field.options ?? []} />; break
    case 'stringList': control = <StringList value={Array.isArray(value) ? (value as string[]) : []} onChange={(v) => set(v)} />; break
    case 'textarea': control = <TextArea value={(value as string | undefined) ?? ''} onChange={(v) => set(v)} placeholder={field.placeholder} mono />; break
    case 'model': control = <ModelSelect value={value as string | undefined} onChange={(v) => set(v)} />; break
    case 'provider': control = <ProviderSelect value={value as string | undefined} onChange={(v) => set(v)} />; break
    case 'theme': control = <ThemePicker value={(value as ThemeId) ?? 'caret-dark'} onChange={(v) => set(v)} />; break
    case 'keyValue': control = <KeyValueEditor value={(value as Record<string, string>) ?? {}} onChange={(v) => set(v)} />; break
  }
  return (
    <Row title={field.title} description={field.description} modified={modified} onReset={reset} vertical={vertical} warning={field.warning && field.type === 'select' && value === 'all' ? field.warning : undefined}>
      {control}
    </Row>
  )
}

import { useMemo, type ReactNode } from 'react'
import { fieldsForPage, type SettingsPageId, type SettingField } from '@/lib/settingsSchema'
import { FieldControl } from './FieldControl'
import { Section } from './primitives'

/** Renders all schema fields of a page grouped by `field.group`, with optional extra sections before/after. */
export function SchemaPage({ page, before, after, fields }: { page: SettingsPageId; before?: ReactNode; after?: ReactNode; fields?: SettingField[] }) {
  const groups = useMemo(() => {
    const list = fields ?? fieldsForPage(page)
    const map = new Map<string, SettingField[]>()
    for (const f of list) { const g = f.group ?? ''; if (!map.has(g)) map.set(g, []); map.get(g)!.push(f) }
    return [...map.entries()]
  }, [page, fields])
  return (
    <>
      {before}
      {groups.map(([group, list]) => (
        <Section key={group || '_'} title={group || 'Settings'}>
          {list.map((f) => <FieldControl key={f.id} field={f} />)}
        </Section>
      ))}
      {after}
    </>
  )
}

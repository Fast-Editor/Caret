import { ContextMenu as RC, DropdownMenu as RD } from 'radix-ui'
import { Check, ChevronRight } from 'lucide-react'
import type { ReactNode } from 'react'
import { Kbd } from './Kbd'
import { cn } from './cn'

export type MenuEntry =
  | { type?: 'item'; label: string; onSelect?: () => void; icon?: ReactNode; shortcut?: string; disabled?: boolean; danger?: boolean; checked?: boolean; hidden?: boolean }
  | { type: 'separator'; hidden?: boolean }
  | { type: 'submenu'; label: string; items: MenuEntry[]; icon?: ReactNode; disabled?: boolean; hidden?: boolean }
  | { type: 'label'; label: string; hidden?: boolean }

const itemCls = 'relative flex items-center gap-2 rounded px-2 h-[26px] text-xs outline-none select-none data-[highlighted]:bg-selected data-[disabled]:opacity-40 cursor-default'
const contentCls = 'z-[900] min-w-[200px] rounded-md border border-border bg-tooltip p-1 shadow-[var(--shadow)] max-h-[70vh] overflow-y-auto'

function renderEntries(entries: MenuEntry[], M: typeof RC | typeof RD): ReactNode {
  return entries.filter((e) => !e.hidden).map((e, i) => {
    if (e.type === 'separator') return <M.Separator key={i} className="my-1 h-px bg-border" />
    if (e.type === 'label') return <M.Label key={i} className="px-2 py-1 text-[10px] uppercase tracking-wide text-faint">{e.label}</M.Label>
    if (e.type === 'submenu') {
      return (
        <M.Sub key={i}>
          <M.SubTrigger disabled={e.disabled} className={itemCls}>
            <span className="w-4 [&_svg]:size-3.5 text-muted">{e.icon}</span><span className="flex-1">{e.label}</span><ChevronRight className="size-3.5 text-muted" />
          </M.SubTrigger>
          <M.Portal><M.SubContent className={contentCls} sideOffset={4}>{renderEntries(e.items, M)}</M.SubContent></M.Portal>
        </M.Sub>
      )
    }
    return (
      <M.Item key={i} disabled={e.disabled} onSelect={() => e.onSelect?.()} className={cn(itemCls, e.danger && 'text-danger')}>
        <span className="w-4 [&_svg]:size-3.5 text-muted flex items-center justify-center">{e.checked ? <Check /> : e.icon}</span>
        <span className="flex-1">{e.label}</span>
        {e.shortcut && <Kbd keys={e.shortcut} />}
      </M.Item>
    )
  })
}

/** Right-click context menu wrapper. */
export function ContextMenu({ items, children, disabled, onOpenChange }: { items: MenuEntry[] | (() => MenuEntry[]); children: ReactNode; disabled?: boolean; onOpenChange?: (o: boolean) => void }) {
  return (
    <RC.Root onOpenChange={onOpenChange}>
      <RC.Trigger asChild disabled={disabled}>{children}</RC.Trigger>
      <RC.Portal>
        <RC.Content className={contentCls} collisionPadding={8}>{renderEntries(typeof items === 'function' ? items() : items, RC)}</RC.Content>
      </RC.Portal>
    </RC.Root>
  )
}

/** Click-triggered dropdown menu. */
export function Menu({ items, children, align = 'start', side = 'bottom' }: { items: MenuEntry[] | (() => MenuEntry[]); children: ReactNode; align?: 'start' | 'center' | 'end'; side?: 'top' | 'bottom' | 'left' | 'right' }) {
  return (
    <RD.Root>
      <RD.Trigger asChild>{children}</RD.Trigger>
      <RD.Portal>
        <RD.Content className={contentCls} align={align} side={side} sideOffset={4} collisionPadding={8}>{renderEntries(typeof items === 'function' ? items() : items, RD)}</RD.Content>
      </RD.Portal>
    </RD.Root>
  )
}

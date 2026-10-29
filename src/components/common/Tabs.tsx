import type { ReactNode } from 'react'
import { cn } from './cn'

export interface TabItem<T extends string = string> { id: T; label: ReactNode; badge?: ReactNode; title?: string }
export function Tabs<T extends string>({ items, value, onChange, className, size = 'sm' }: { items: TabItem<T>[]; value: T; onChange: (id: T) => void; className?: string; size?: 'xs' | 'sm' }) {
  return (
    <div role="tablist" className={cn('flex items-center gap-0.5', className)}>
      {items.map((t) => (
        <button key={t.id} role="tab" aria-selected={value === t.id} title={t.title} onClick={() => onChange(t.id)}
          className={cn('inline-flex items-center gap-1.5 rounded-md px-2 text-muted hover:text-fg press', size === 'xs' ? 'h-5 text-[10px]' : 'h-6 text-[11px]', value === t.id && 'text-fg bg-active')}>
          {t.label}
          {t.badge != null && <span className="rounded-full bg-border-strong px-1.5 text-[10px]">{t.badge}</span>}
        </button>
      ))}
    </div>
  )
}

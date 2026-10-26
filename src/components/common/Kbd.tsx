import { formatKeybinding } from '@/lib/keybindings'
import { cn } from './cn'

/** Renders a keybinding like "mod+shift+p" as ⌘⇧P chips. */
export function Kbd({ keys, className }: { keys: string; className?: string }) {
  const parts = formatKeybinding(keys).split(' ')
  return (
    <span className={cn('inline-flex items-center gap-1', className)}>
      {parts.map((p, i) => (
        <kbd key={i} className="rounded border border-border-strong bg-input px-1 min-w-[18px] text-center text-[10px] leading-4 font-ui text-muted">{p}</kbd>
      ))}
    </span>
  )
}

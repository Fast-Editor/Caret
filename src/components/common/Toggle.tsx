import { Switch } from 'radix-ui'
import { cn } from './cn'

export function Toggle({ checked, onCheckedChange, disabled, label, className }: { checked: boolean; onCheckedChange: (v: boolean) => void; disabled?: boolean; label?: string; className?: string }) {
  return (
    <label className={cn('inline-flex items-center gap-2 text-xs', disabled && 'opacity-50', className)}>
      <Switch.Root checked={checked} onCheckedChange={onCheckedChange} disabled={disabled} aria-label={label}
        className="relative h-[16px] w-[28px] rounded-full bg-border-strong data-[state=checked]:bg-accent transition-colors outline-none focus-visible:ring-1 focus-visible:ring-accent">
        <Switch.Thumb className="block size-3 translate-x-[2px] rounded-full bg-white transition-transform data-[state=checked]:translate-x-[14px]" />
      </Switch.Root>
      {label && <span>{label}</span>}
    </label>
  )
}

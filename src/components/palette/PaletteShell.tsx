import { Command } from 'cmdk'
import { useEffect, useRef, type ReactNode } from 'react'
import { cn } from '@/components/common'

/** Shared overlay + cmdk root for palettes (top-centered, VS Code-like). */
export function PaletteShell({ value, onValueChange, placeholder, onClose, children, shouldFilter = true, footer, onKeyDown, label }: {
  value: string; onValueChange: (v: string) => void; placeholder: string; onClose: () => void; children: ReactNode; shouldFilter?: boolean; footer?: ReactNode; onKeyDown?: (e: React.KeyboardEvent) => void; label: string
}) {
  const inputRef = useRef<HTMLInputElement>(null)
  useEffect(() => { inputRef.current?.focus() }, [])
  return (
    <div className="fixed inset-0 z-[800]" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose() }}>
      <div className="absolute left-1/2 top-[8%] -translate-x-1/2 w-[640px] max-w-[92vw]" data-owns-keys>
        <Command label={label} loop shouldFilter={shouldFilter} className="rounded-lg surface-float overflow-hidden text-fg animate-pop"
          onKeyDown={(e) => { if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); onClose(); return } onKeyDown?.(e) }}>
          <Command.Input ref={inputRef} value={value} onValueChange={onValueChange} placeholder={placeholder} autoFocus
            className="w-full h-9 bg-input border-b border-border px-3 text-[13px] outline-none placeholder:text-faint" />
          <Command.List className={cn('max-h-[52vh] overflow-auto p-1 scroll-py-1')}>
            {children}
          </Command.List>
          {footer && <div className="border-t border-border px-3 h-6 flex items-center gap-3 text-[11px] text-muted">{footer}</div>}
        </Command>
      </div>
    </div>
  )
}

export function PaletteItem({ value, onSelect, children, className, keywords }: { value: string; onSelect: () => void; children: ReactNode; className?: string; keywords?: string[] }) {
  return (
    <Command.Item value={value} keywords={keywords} onSelect={onSelect} className={cn('flex items-center gap-2 rounded-md px-2 h-[30px] text-xs cursor-pointer data-[selected=true]:bg-selected', className)}>
      {children}
    </Command.Item>
  )
}

export function Highlight({ text, indices, className }: { text: string; indices?: number[]; className?: string }) {
  if (!indices?.length) return <span className={className}>{text}</span>
  const set = new Set(indices)
  const out: ReactNode[] = []
  let run = ''; let runHl = false
  const flush = (i: number) => { if (run) out.push(runHl ? <mark key={i} className="bg-transparent text-accent font-semibold">{run}</mark> : <span key={i}>{run}</span>); run = '' }
  for (let i = 0; i < text.length; i++) { const hl = set.has(i); if (hl !== runHl) { flush(i); runHl = hl } run += text[i] }
  flush(text.length)
  return <span className={className}>{out}</span>
}

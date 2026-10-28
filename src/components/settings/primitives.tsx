/** Shared form/UI primitives for the Settings and Source Control modules (radix-ui based, token-styled). */
import { useEffect, useId, useRef, useState, type ReactNode, type KeyboardEvent } from 'react'
import { createRoot } from 'react-dom/client'
import { AlertDialog, Dialog as RDialog, DropdownMenu, Select as RSelect, Switch, Tooltip as RTooltip } from 'radix-ui'
import { Check, ChevronDown, Loader2, Plus, X } from 'lucide-react'
import { clsx } from 'clsx'
import { runCommand, hasCommand } from '@/lib/commands'

export function cn(...a: (string | false | null | undefined)[]): string { return clsx(a) }

// ───────────────────────── notify / confirm ─────────────────────────
export function notify(level: 'info' | 'warn' | 'error' | 'success', title: string, message?: string): void {
  if (hasCommand('app.notify')) void runCommand('app.notify', { level, title, message })
  else (level === 'error' ? console.error : console.log)(`[${level}] ${title}${message ? ` — ${message}` : ''}`)
}
export function errorMessage(e: unknown): string { return (e as { message?: string })?.message ?? String(e) }

export interface ConfirmOptions { title: string; message?: ReactNode; confirmLabel?: string; cancelLabel?: string; danger?: boolean }
/** Imperative confirmation dialog (works without any host component). */
export function confirmDialog(opts: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    const host = document.createElement('div')
    document.body.appendChild(host)
    const root = createRoot(host)
    const done = (v: boolean) => { resolve(v); setTimeout(() => { root.unmount(); host.remove() }, 0) }
    root.render(
      <AlertDialog.Root open onOpenChange={(o) => { if (!o) done(false) }}>
        <AlertDialog.Portal>
          <AlertDialog.Overlay className="fixed inset-0 bg-black/40 z-[1000]" />
          <AlertDialog.Content className="fixed z-[1001] left-1/2 top-1/3 -translate-x-1/2 w-[420px] max-w-[92vw] rounded-md border border-border bg-panel p-4 shadow-[var(--shadow)] text-fg">
            <AlertDialog.Title className="text-sm font-semibold mb-1">{opts.title}</AlertDialog.Title>
            {opts.message && <AlertDialog.Description className="text-xs text-muted mb-4 whitespace-pre-wrap">{opts.message}</AlertDialog.Description>}
            <div className="flex justify-end gap-2 mt-3">
              <AlertDialog.Cancel asChild><Button onClick={() => done(false)} autoFocus={!opts.danger}>{opts.cancelLabel ?? 'Cancel'}</Button></AlertDialog.Cancel>
              <AlertDialog.Action asChild><Button variant={opts.danger ? 'danger' : 'primary'} onClick={() => done(true)} autoFocus={!!opts.danger}>{opts.confirmLabel ?? 'OK'}</Button></AlertDialog.Action>
            </div>
          </AlertDialog.Content>
        </AlertDialog.Portal>
      </AlertDialog.Root>,
    )
  })
}

/** Imperative single-line prompt dialog. */
export function promptDialog(opts: { title: string; message?: string; placeholder?: string; defaultValue?: string; confirmLabel?: string; validate?: (v: string) => string | null }): Promise<string | null> {
  return new Promise((resolve) => {
    const host = document.createElement('div')
    document.body.appendChild(host)
    const root = createRoot(host)
    const done = (v: string | null) => { resolve(v); setTimeout(() => { root.unmount(); host.remove() }, 0) }
    function Body() {
      const [v, setV] = useState(opts.defaultValue ?? '')
      const err = opts.validate ? opts.validate(v) : null
      return (
        <RDialog.Root open onOpenChange={(o) => { if (!o) done(null) }}>
          <RDialog.Portal>
            <RDialog.Overlay className="fixed inset-0 bg-black/40 z-[1000]" />
            <RDialog.Content className="fixed z-[1001] left-1/2 top-1/3 -translate-x-1/2 w-[440px] max-w-[92vw] rounded-md border border-border bg-panel p-4 shadow-[var(--shadow)] text-fg">
              <RDialog.Title className="text-sm font-semibold mb-1">{opts.title}</RDialog.Title>
              {opts.message && <RDialog.Description className="text-xs text-muted mb-3">{opts.message}</RDialog.Description>}
              <TextInput autoFocus value={v} placeholder={opts.placeholder} onChange={setV} onKeyDown={(e) => { if (e.key === 'Enter' && !err) done(v); if (e.key === 'Escape') done(null) }} />
              {err && v && <div className="text-danger text-[11px] mt-1">{err}</div>}
              <div className="flex justify-end gap-2 mt-3">
                <Button onClick={() => done(null)}>Cancel</Button>
                <Button variant="primary" disabled={!!err} onClick={() => done(v)}>{opts.confirmLabel ?? 'OK'}</Button>
              </div>
            </RDialog.Content>
          </RDialog.Portal>
        </RDialog.Root>
      )
    }
    root.render(<Body />)
  })
}

// ───────────────────────── basic controls ─────────────────────────
type BtnVariant = 'default' | 'primary' | 'danger' | 'ghost' | 'link'
export function Button({ variant = 'default', size = 'md', className, loading, children, ...rest }: React.ButtonHTMLAttributes<HTMLButtonElement> & { variant?: BtnVariant; size?: 'sm' | 'md'; loading?: boolean }) {
  return (
    <button
      {...rest}
      disabled={rest.disabled || loading}
      className={cn(
        'inline-flex items-center gap-1.5 rounded-[var(--radius)] border text-xs whitespace-nowrap transition-colors disabled:opacity-50 disabled:pointer-events-none',
        size === 'sm' ? 'h-6 px-2' : 'h-7 px-3',
        variant === 'default' && 'bg-input border-border hover:bg-hover',
        variant === 'primary' && 'bg-accent text-accent-fg border-transparent hover:bg-accent-hover font-medium',
        variant === 'danger' && 'bg-danger/90 text-white border-transparent hover:bg-danger',
        variant === 'ghost' && 'bg-transparent border-transparent hover:bg-hover text-muted hover:text-fg',
        variant === 'link' && 'bg-transparent border-transparent text-accent hover:underline px-0 h-auto',
        className,
      )}
    >
      {loading && <Loader2 size={12} className="spin" />}
      {children}
    </button>
  )
}

export function IconButton({ title, className, children, ...rest }: React.ButtonHTMLAttributes<HTMLButtonElement> & { title: string }) {
  return (
    <Tooltip label={title}>
      <button {...rest} aria-label={title} className={cn('inline-flex items-center justify-center h-6 w-6 rounded text-muted hover:text-fg hover:bg-hover disabled:opacity-40 disabled:pointer-events-none', className)}>{children}</button>
    </Tooltip>
  )
}

export function Tooltip({ label, children, side = 'top' }: { label: ReactNode; children: ReactNode; side?: 'top' | 'bottom' | 'left' | 'right' }) {
  return (
    <RTooltip.Provider delayDuration={400}>
      <RTooltip.Root>
        <RTooltip.Trigger asChild>{children}</RTooltip.Trigger>
        <RTooltip.Portal>
          <RTooltip.Content side={side} sideOffset={4} className="z-[1200] rounded bg-tooltip border border-border px-2 py-1 text-[11px] text-fg shadow-[var(--shadow)] max-w-[320px]">{label}</RTooltip.Content>
        </RTooltip.Portal>
      </RTooltip.Root>
    </RTooltip.Provider>
  )
}

export function Toggle({ checked, onChange, disabled, label }: { checked: boolean; onChange: (v: boolean) => void; disabled?: boolean; label?: string }) {
  return (
    <Switch.Root checked={checked} onCheckedChange={onChange} disabled={disabled} aria-label={label}
      className={cn('relative h-[18px] w-[32px] rounded-full border transition-colors disabled:opacity-50', checked ? 'bg-accent border-accent' : 'bg-input border-border-strong')}>
      <Switch.Thumb className={cn('block h-[14px] w-[14px] rounded-full bg-white shadow transition-transform translate-x-[1px]', checked && 'translate-x-[15px]')} />
    </Switch.Root>
  )
}

export function Select<T extends string>({ value, onChange, options, placeholder, disabled, className, allowEmpty }: { value: T | undefined; onChange: (v: T | undefined) => void; options: { value: T; label: string; description?: string }[]; placeholder?: string; disabled?: boolean; className?: string; allowEmpty?: string }) {
  const EMPTY = '__none__'
  return (
    <RSelect.Root value={value ?? (allowEmpty !== undefined ? EMPTY : undefined)} onValueChange={(v) => onChange(v === EMPTY ? undefined : (v as T))} disabled={disabled}>
      <RSelect.Trigger className={cn('inline-flex items-center justify-between gap-2 h-7 min-w-[160px] px-2 rounded-[var(--radius)] border border-border bg-input text-xs hover:bg-hover disabled:opacity-50 data-[placeholder]:text-muted', className)}>
        <RSelect.Value placeholder={placeholder ?? 'Select…'} />
        <RSelect.Icon><ChevronDown size={12} className="text-muted" /></RSelect.Icon>
      </RSelect.Trigger>
      <RSelect.Portal>
        <RSelect.Content position="popper" sideOffset={4} className="z-[1200] max-h-[320px] min-w-[var(--radix-select-trigger-width)] overflow-hidden rounded-md border border-border bg-panel shadow-[var(--shadow)]">
          <RSelect.Viewport className="p-1">
            {allowEmpty !== undefined && (
              <RSelect.Item value={EMPTY} className="relative flex items-center gap-2 pl-6 pr-2 py-1 rounded text-xs text-muted cursor-pointer select-none outline-none data-[highlighted]:bg-hover">
                <RSelect.ItemIndicator className="absolute left-1.5"><Check size={12} /></RSelect.ItemIndicator>
                <RSelect.ItemText>{allowEmpty}</RSelect.ItemText>
              </RSelect.Item>
            )}
            {options.map((o) => (
              <RSelect.Item key={o.value} value={o.value} className="relative flex flex-col items-start pl-6 pr-2 py-1 rounded text-xs cursor-pointer select-none outline-none data-[highlighted]:bg-hover">
                <RSelect.ItemIndicator className="absolute left-1.5 top-1.5"><Check size={12} /></RSelect.ItemIndicator>
                <RSelect.ItemText>{o.label}</RSelect.ItemText>
                {o.description && <span className="text-[10px] text-muted">{o.description}</span>}
              </RSelect.Item>
            ))}
          </RSelect.Viewport>
        </RSelect.Content>
      </RSelect.Portal>
    </RSelect.Root>
  )
}

export function TextInput({ value, onChange, className, mono, ...rest }: Omit<React.InputHTMLAttributes<HTMLInputElement>, 'onChange' | 'value'> & { value: string; onChange: (v: string) => void; mono?: boolean }) {
  return <input {...rest} value={value} onChange={(e) => onChange(e.target.value)} className={cn('h-7 w-full px-2 rounded-[var(--radius)] border border-border bg-input text-xs placeholder:text-faint focus:border-accent outline-none', mono && 'mono', className)} />
}

export function NumberInput({ value, onChange, min, max, step, unit, className }: { value: number; onChange: (v: number) => void; min?: number; max?: number; step?: number; unit?: string; className?: string }) {
  const [text, setText] = useState(String(value))
  useEffect(() => { setText(String(value)) }, [value])
  const commit = () => {
    let n = Number(text)
    if (!Number.isFinite(n)) { setText(String(value)); return }
    if (min !== undefined) n = Math.max(min, n)
    if (max !== undefined) n = Math.min(max, n)
    if (n !== value) onChange(n); else setText(String(n))
  }
  return (
    <div className={cn('inline-flex items-center h-7 rounded-[var(--radius)] border border-border bg-input focus-within:border-accent', className)}>
      <input type="number" value={text} min={min} max={max} step={step} onChange={(e) => setText(e.target.value)} onBlur={commit} onKeyDown={(e) => { if (e.key === 'Enter') commit() }}
        className="h-full w-[88px] px-2 bg-transparent text-xs outline-none mono [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none" />
      {unit && <span className="pr-2 text-[11px] text-muted">{unit}</span>}
    </div>
  )
}

export function TextArea({ value, onChange, className, mono, rows = 6, ...rest }: Omit<React.TextareaHTMLAttributes<HTMLTextAreaElement>, 'onChange' | 'value'> & { value: string; onChange: (v: string) => void; mono?: boolean }) {
  return <textarea {...rest} rows={rows} value={value} onChange={(e) => onChange(e.target.value)} className={cn('w-full p-2 rounded-[var(--radius)] border border-border bg-input text-xs placeholder:text-faint focus:border-accent outline-none resize-y leading-relaxed', mono && 'mono', className)} />
}

export function StringList({ value, onChange, placeholder, mono = true }: { value: string[]; onChange: (v: string[]) => void; placeholder?: string; mono?: boolean }) {
  const [draft, setDraft] = useState('')
  const add = () => { const v = draft.trim(); if (!v || value.includes(v)) { setDraft(''); return }; onChange([...value, v]); setDraft('') }
  return (
    <div className="w-full">
      <div className="flex flex-wrap gap-1 mb-1.5 min-h-[4px]">
        {value.map((v) => (
          <span key={v} className={cn('inline-flex items-center gap-1 h-5 pl-2 pr-1 rounded-full bg-hover border border-border text-[11px]', mono && 'mono')}>
            {v}
            <button aria-label={`Remove ${v}`} onClick={() => onChange(value.filter((x) => x !== v))} className="h-3.5 w-3.5 rounded-full inline-flex items-center justify-center text-muted hover:text-fg hover:bg-active"><X size={10} /></button>
          </span>
        ))}
      </div>
      <div className="flex gap-1">
        <TextInput value={draft} onChange={setDraft} placeholder={placeholder ?? 'Add… (Enter)'} mono={mono} onKeyDown={(e: KeyboardEvent<HTMLInputElement>) => { if (e.key === 'Enter') { e.preventDefault(); add() } }} />
        <Button size="md" onClick={add} disabled={!draft.trim()}><Plus size={12} />Add</Button>
      </div>
    </div>
  )
}

export function KeyValueEditor({ value, onChange, keyPlaceholder = 'Name', valuePlaceholder = 'Value', secret }: { value: Record<string, string>; onChange: (v: Record<string, string>) => void; keyPlaceholder?: string; valuePlaceholder?: string; secret?: boolean }) {
  const entries = Object.entries(value)
  const [k, setK] = useState(''); const [v, setV] = useState('')
  const add = () => { if (!k.trim()) return; onChange({ ...value, [k.trim()]: v }); setK(''); setV('') }
  return (
    <div className="w-full space-y-1">
      {entries.map(([ek, ev]) => (
        <div key={ek} className="flex gap-1 items-center">
          <span className="mono text-[11px] w-[40%] truncate-1">{ek}</span>
          <TextInput mono type={secret ? 'password' : 'text'} value={ev} onChange={(nv) => onChange({ ...value, [ek]: nv })} />
          <IconButton title="Remove" onClick={() => { const n = { ...value }; delete n[ek]; onChange(n) }}><X size={12} /></IconButton>
        </div>
      ))}
      <div className="flex gap-1 items-center">
        <TextInput mono value={k} onChange={setK} placeholder={keyPlaceholder} className="w-[40%]" />
        <TextInput mono value={v} onChange={setV} placeholder={valuePlaceholder} onKeyDown={(e: KeyboardEvent<HTMLInputElement>) => { if (e.key === 'Enter') add() }} />
        <IconButton title="Add" onClick={add} disabled={!k.trim()}><Plus size={12} /></IconButton>
      </div>
    </div>
  )
}

export function Badge({ children, tone = 'default', className }: { children: ReactNode; tone?: 'default' | 'accent' | 'success' | 'warning' | 'danger' | 'info'; className?: string }) {
  return (
    <span className={cn('inline-flex items-center h-[18px] px-1.5 rounded text-[10px] font-medium uppercase tracking-wide border',
      tone === 'default' && 'bg-hover border-border text-muted', tone === 'accent' && 'bg-accent/15 border-accent/40 text-accent', tone === 'success' && 'bg-success/15 border-success/40 text-success',
      tone === 'warning' && 'bg-warning/15 border-warning/40 text-warning', tone === 'danger' && 'bg-danger/15 border-danger/40 text-danger', tone === 'info' && 'bg-info/15 border-info/40 text-info', className)}>{children}</span>
  )
}

export function Spinner({ size = 14, className }: { size?: number; className?: string }) { return <Loader2 size={size} className={cn('spin text-muted', className)} /> }

export function Section({ title, description, actions, children, className }: { title: ReactNode; description?: ReactNode; actions?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={cn('mb-7', className)}>
      <div className="flex items-start justify-between gap-3 mb-2">
        <div>
          <h3 className="text-[13px] font-semibold">{title}</h3>
          {description && <p className="text-[11px] text-muted mt-0.5 max-w-[640px]">{description}</p>}
        </div>
        {actions && <div className="flex items-center gap-1.5 shrink-0">{actions}</div>}
      </div>
      <div className="rounded-md border border-border bg-panel/60 divide-y divide-border">{children}</div>
    </section>
  )
}

export function Row({ title, description, modified, onReset, children, warning, vertical }: { title: ReactNode; description?: ReactNode; modified?: boolean; onReset?: () => void; children?: ReactNode; warning?: ReactNode; vertical?: boolean }) {
  const id = useId()
  return (
    <div className={cn('px-3 py-2.5 flex gap-4', vertical ? 'flex-col' : 'items-center justify-between')}>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          {modified && <span title="Modified (click to reset)" onClick={onReset} className="h-1.5 w-1.5 rounded-full bg-accent shrink-0 cursor-pointer" />}
          <label htmlFor={id} className="text-xs font-medium">{title}</label>
          {modified && onReset && <button onClick={onReset} className="text-[10px] text-muted hover:text-fg ml-1">reset</button>}
        </div>
        {description && <div className="text-[11px] text-muted mt-0.5 leading-snug">{description}</div>}
        {warning && <div className="text-[11px] text-warning mt-1">{warning}</div>}
      </div>
      <div className={cn('shrink-0 flex items-center', vertical ? 'w-full' : 'max-w-[55%]')}>{children}</div>
    </div>
  )
}

export function Empty({ icon, title, description, action }: { icon?: ReactNode; title: string; description?: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center text-center py-8 px-4 text-muted">
      {icon && <div className="mb-2 text-faint">{icon}</div>}
      <div className="text-xs font-medium text-fg">{title}</div>
      {description && <div className="text-[11px] mt-1 max-w-[420px]">{description}</div>}
      {action && <div className="mt-3">{action}</div>}
    </div>
  )
}

// ───────────────────────── dialog ─────────────────────────
export function Dialog({ open, onOpenChange, title, description, children, footer, width = 560 }: { open: boolean; onOpenChange: (o: boolean) => void; title: ReactNode; description?: ReactNode; children: ReactNode; footer?: ReactNode; width?: number }) {
  return (
    <RDialog.Root open={open} onOpenChange={onOpenChange}>
      <RDialog.Portal>
        <RDialog.Overlay className="fixed inset-0 bg-black/40 z-[1000]" />
        <RDialog.Content style={{ width, maxWidth: '94vw' }} className="fixed z-[1001] left-1/2 top-[12vh] -translate-x-1/2 max-h-[78vh] flex flex-col rounded-md border border-border bg-panel shadow-[var(--shadow)] text-fg outline-none">
          <div className="flex items-start justify-between px-4 pt-3 pb-2 border-b border-border">
            <div>
              <RDialog.Title className="text-sm font-semibold">{title}</RDialog.Title>
              {description && <RDialog.Description className="text-[11px] text-muted mt-0.5">{description}</RDialog.Description>}
            </div>
            <RDialog.Close asChild><IconButton title="Close"><X size={14} /></IconButton></RDialog.Close>
          </div>
          <div className="px-4 py-3 overflow-auto flex-1">{children}</div>
          {footer && <div className="px-4 py-2.5 border-t border-border flex justify-end gap-2">{footer}</div>}
        </RDialog.Content>
      </RDialog.Portal>
    </RDialog.Root>
  )
}

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="block mb-3">
      <div className="text-[11px] font-medium mb-1">{label}</div>
      {children}
      {hint && <div className="text-[10px] text-muted mt-0.5">{hint}</div>}
    </label>
  )
}

export function Menu({ trigger, items, align = 'end' }: { trigger: ReactNode; items: ({ id: string; label: ReactNode; icon?: ReactNode; danger?: boolean; disabled?: boolean; onSelect: () => void } | 'sep')[]; align?: 'start' | 'end' }) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>{trigger}</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content align={align} sideOffset={4} className="z-[1200] min-w-[180px] rounded-md border border-border bg-panel p-1 shadow-[var(--shadow)] text-xs">
          {items.map((it, i) => it === 'sep' ? <DropdownMenu.Separator key={i} className="my-1 h-px bg-border" /> : (
            <DropdownMenu.Item key={it.id} disabled={it.disabled} onSelect={it.onSelect} className={cn('flex items-center gap-2 px-2 py-1 rounded outline-none cursor-pointer select-none data-[highlighted]:bg-hover data-[disabled]:opacity-40', it.danger && 'text-danger')}>
              {it.icon}<span>{it.label}</span>
            </DropdownMenu.Item>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  )
}

export function useDebouncedCallback<A extends unknown[]>(fn: (...a: A) => void, ms: number): (...a: A) => void {
  const t = useRef<number | null>(null)
  const ref = useRef(fn); ref.current = fn
  useEffect(() => () => { if (t.current) window.clearTimeout(t.current) }, [])
  return (...a: A) => { if (t.current) window.clearTimeout(t.current); t.current = window.setTimeout(() => ref.current(...a), ms) }
}

export function relativeTime(ts: number): string {
  const ms = ts > 1e12 ? ts : ts * 1000
  const diff = Date.now() - ms
  const s = Math.round(diff / 1000)
  if (s < 45) return 'just now'
  const m = Math.round(s / 60); if (m < 60) return `${m}m ago`
  const h = Math.round(m / 60); if (h < 24) return `${h}h ago`
  const d = Math.round(h / 24); if (d < 30) return `${d}d ago`
  const mo = Math.round(d / 30); if (mo < 12) return `${mo}mo ago`
  return `${Math.round(mo / 12)}y ago`
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1048576) return `${(n / 1024).toFixed(1)} KB`
  if (n < 1073741824) return `${(n / 1048576).toFixed(1)} MB`
  return `${(n / 1073741824).toFixed(2)} GB`
}

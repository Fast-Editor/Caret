import '@/components/common/motion.css'
import { Dialog } from 'radix-ui'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Command } from 'cmdk'
import { X } from 'lucide-react'
import { Button, Input, cn } from '@/components/common'
import { useDialogs, type DialogRequest } from './store'
export { confirm, prompt, pick, showDialog, type PickItem, type PickOptions, type ConfirmOptions, type PromptOptions } from './store'

/** Mount once at the app root. Renders the head of the dialog queue. */
export function DialogHost() {
  const current = useDialogs((s) => s.queue[0])
  if (!current) return null
  return <DialogFrame key={current.id} req={current} />
}

function DialogFrame({ req }: { req: DialogRequest }) {
  const pop = useDialogs((s) => s.pop)
  const close = (fn: () => void) => { fn(); pop(req.id) }
  const cancel = () => {
    if (req.kind === 'confirm') close(() => req.resolve(false))
    else if (req.kind === 'prompt' || req.kind === 'pick') close(() => req.resolve(null))
    else close(() => req.resolve())
  }
  const title = req.kind === 'custom' ? req.title : req.opts.title
  return (
    <Dialog.Root open onOpenChange={(o) => { if (!o) cancel() }}>
      <Dialog.Portal>
        <Dialog.Overlay data-caret-overlay className="fixed inset-0 z-[950] bg-black/40 backdrop-blur-sm" />
        <Dialog.Content aria-describedby={undefined} data-owns-keys data-caret-pop
          className={cn('fixed left-1/2 top-[18%] z-[951] -translate-x-1/2 rounded-lg surface-float outline-none text-fg', req.kind === 'custom' && req.wide ? 'w-[760px] max-w-[92vw]' : req.kind === 'pick' ? 'w-[560px] max-w-[92vw]' : 'w-[440px] max-w-[92vw]')}>
          <div className="flex items-center justify-between px-4 pt-3 pb-1">
            <Dialog.Title className="text-sm font-semibold">{title}</Dialog.Title>
            <Dialog.Close asChild><button aria-label="Close" className="rounded p-1 text-muted hover:text-fg hover:bg-hover"><X className="size-4" /></button></Dialog.Close>
          </div>
          {req.kind === 'confirm' && <ConfirmBody req={req} close={close} />}
          {req.kind === 'prompt' && <PromptBody req={req} close={close} />}
          {req.kind === 'pick' && <PickBody req={req} close={close} />}
          {req.kind === 'custom' && <div className="px-4 pb-4 max-h-[70vh] overflow-auto selectable">{req.render(() => close(() => req.resolve()))}</div>}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}

function ConfirmBody({ req, close }: { req: Extract<DialogRequest, { kind: 'confirm' }>; close: (fn: () => void) => void }) {
  const okRef = useRef<HTMLButtonElement>(null)
  useEffect(() => { okRef.current?.focus() }, [])
  return (
    <div className="px-4 pb-4">
      {req.opts.message && <div className="text-xs text-muted mb-1 selectable">{req.opts.message}</div>}
      {req.opts.detail && <pre className="mono text-[11px] text-muted bg-input rounded p-2 mt-2 max-h-40 overflow-auto selectable whitespace-pre-wrap">{req.opts.detail}</pre>}
      <div className="mt-4 flex justify-end gap-2">
        <Button variant="secondary" onClick={() => close(() => req.resolve(false))}>{req.opts.cancelLabel ?? 'Cancel'}</Button>
        <Button ref={okRef} variant={req.opts.danger ? 'danger' : 'primary'} onClick={() => close(() => req.resolve(true))}>{req.opts.okLabel ?? 'OK'}</Button>
      </div>
    </div>
  )
}

function PromptBody({ req, close }: { req: Extract<DialogRequest, { kind: 'prompt' }>; close: (fn: () => void) => void }) {
  const [value, setValue] = useState(req.opts.defaultValue ?? '')
  const [touched, setTouched] = useState(false)
  const ref = useRef<HTMLInputElement>(null)
  const error = req.opts.validate?.(value) ?? null
  useEffect(() => {
    const el = ref.current
    if (!el) return
    el.focus()
    if (req.opts.selectRange) el.setSelectionRange(req.opts.selectRange[0], req.opts.selectRange[1])
    else el.select()
  }, [req.opts.selectRange])
  const submit = () => { if (error) { setTouched(true); return } close(() => req.resolve(value)) }
  return (
    <form className="px-4 pb-4" onSubmit={(e) => { e.preventDefault(); submit() }}>
      {req.opts.message && <div className="text-xs text-muted mb-2">{req.opts.message}</div>}
      <Input ref={ref} value={value} placeholder={req.opts.placeholder} invalid={touched && !!error} onChange={(e) => { setValue(e.target.value); setTouched(true) }} onKeyDown={(e) => { if (e.key === 'Escape') { e.preventDefault(); close(() => req.resolve(null)) } }} />
      {touched && error && <div className="mt-1 text-[11px] text-danger">{error}</div>}
      <div className="mt-4 flex justify-end gap-2">
        <Button type="button" variant="secondary" onClick={() => close(() => req.resolve(null))}>Cancel</Button>
        <Button type="submit" variant="primary" disabled={!!error}>{req.opts.okLabel ?? 'OK'}</Button>
      </div>
    </form>
  )
}

function PickBody({ req, close }: { req: Extract<DialogRequest, { kind: 'pick' }>; close: (fn: () => void) => void }) {
  const [q, setQ] = useState('')
  const items = useMemo(() => req.opts.items, [req.opts.items])
  return (
    <Command label={req.opts.title} className="px-2 pb-2" loop>
      <div className="px-2 pb-2">
        <Command.Input autoFocus value={q} onValueChange={setQ} placeholder={req.opts.placeholder ?? 'Type to filter…'}
          className="w-full h-7 rounded-md border border-border bg-input px-2 text-xs outline-none focus:border-accent"
          onKeyDown={(e) => { if (e.key === 'Enter' && req.opts.allowFreeText && q && !items.some((i) => i.label.toLowerCase() === q.toLowerCase())) { e.preventDefault(); close(() => req.resolve(q)) } }} />
      </div>
      <Command.List className="max-h-[50vh] overflow-auto">
        <Command.Empty className="px-3 py-4 text-xs text-muted">{req.opts.allowFreeText ? 'Press Enter to use the typed value' : 'No matches'}</Command.Empty>
        {items.map((it) => (
          <Command.Item key={it.id} value={`${it.label} ${it.description ?? ''} ${it.keywords ?? ''}`} onSelect={() => close(() => req.resolve(it.id))}
            className="flex items-center gap-2 rounded px-2 h-7 text-xs cursor-pointer data-[selected=true]:bg-selected">
            {it.icon && <span className="text-muted [&_svg]:size-3.5">{it.icon}</span>}
            <span className="truncate">{it.label}</span>
            {it.description && <span className="text-muted truncate">{it.description}</span>}
            {it.detail && <span className="ml-auto text-faint truncate text-[11px]">{it.detail}</span>}
          </Command.Item>
        ))}
      </Command.List>
    </Command>
  )
}

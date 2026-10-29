import { Popover } from 'radix-ui'
import { AlertTriangle, Bell, BellOff, CheckCircle2, Info, X, XCircle, Trash2 } from 'lucide-react'
import { useEffect } from 'react'
import { IconButton, cn } from '@/components/common'
import { useNotifications, type Toast } from '@/store/notifications'
import { Events, on } from '@/lib/tauri'
import type { Notification } from '@/types'
import '@/components/common/motion.css'

const ICONS = { info: Info, warn: AlertTriangle, error: XCircle, success: CheckCircle2 } as const
const COLORS = { info: 'text-info', warn: 'text-warning', error: 'text-danger', success: 'text-success' } as const

function ToastCard({ t, compact }: { t: Toast; compact?: boolean }) {
  const { dismiss, actionHandlers } = useNotifications()
  const Icon = ICONS[t.level]
  return (
    <div role="status" data-leaving={!!t.leaving} className={cn('pointer-events-auto rounded-md surface-float text-xs', !compact && 'caret-toast', compact ? 'p-2' : 'p-3 w-[360px]')}>
      <div className="flex items-start gap-2">
        <Icon className={cn('size-4 shrink-0 mt-px', COLORS[t.level])} />
        <div className="flex-1 min-w-0">
          <div className="font-medium text-fg break-words">{t.title}</div>
          {t.message && <div className="mt-0.5 text-muted break-words whitespace-pre-wrap selectable max-h-32 overflow-auto">{t.message}</div>}
          {t.progress != null && <div className="mt-2 h-1 rounded bg-border overflow-hidden"><div className="h-full bg-accent transition-[width]" style={{ width: `${Math.round(t.progress * 100)}%` }} /></div>}
          {!!t.actions?.length && (
            <div className="mt-2 flex gap-2 flex-wrap">
              {t.actions.map((a) => (
                <button key={a.id} onClick={() => { actionHandlers.get(t.id)?.(a.id); dismiss(t.id) }} className="rounded border border-border-strong bg-input px-2 h-6 text-xs hover:bg-active">{a.label}</button>
              ))}
            </div>
          )}
        </div>
        {!compact && <button aria-label="Dismiss" onClick={() => dismiss(t.id)} className="rounded p-0.5 text-muted hover:text-fg hover:bg-hover"><X className="size-3.5" /></button>}
      </div>
    </div>
  )
}

/** Toast stack (bottom-right). Also bridges backend `caret://notify` events. */
export function Toasts() {
  const toasts = useNotifications((s) => s.toasts)
  const add = useNotifications((s) => s.add)
  useEffect(() => on<Notification>(Events.notify, (n) => { add({ ...n }) }), [add])
  return (
    <div className="pointer-events-none fixed bottom-8 right-3 z-[900] flex flex-col gap-2 items-end">
      {toasts.map((t) => <ToastCard key={t.id} t={t} />)}
    </div>
  )
}

/** Bell with a popover notification centre (used by the status bar). */
export function NotificationBell() {
  const { history, unread, centerOpen, setCenterOpen, clearHistory } = useNotifications()
  return (
    <Popover.Root open={centerOpen} onOpenChange={setCenterOpen}>
      <Popover.Trigger asChild>
        <button aria-label="Notifications" className="relative flex items-center h-full px-1.5 hover:bg-hover text-muted hover:text-fg">
          {history.length ? <Bell className="size-3.5" /> : <BellOff className="size-3.5" />}
          {unread > 0 && <span className="absolute top-0.5 right-0 rounded-full bg-accent text-accent-fg text-[9px] leading-[14px] min-w-[14px] px-0.5 text-center">{unread}</span>}
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content data-caret-pop side="top" align="end" sideOffset={6} className="z-[900] w-[380px] max-h-[60vh] rounded-md surface-float flex flex-col">
          <div className="flex items-center justify-between px-3 h-8 border-b border-border text-xs">
            <span className="font-medium">Notifications</span>
            <IconButton icon={<Trash2 />} label="Clear all" size="xs" onClick={clearHistory} />
          </div>
          <div className="overflow-auto p-2 flex flex-col gap-2">
            {history.length === 0 && <div className="text-muted text-xs px-1 py-3 text-center">No new notifications</div>}
            {history.map((t) => <ToastCard key={t.id} t={t} compact />)}
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}

import { DropdownMenu } from 'radix-ui'
import { Check, ChevronDown, ListPlus, MessageSquarePlus, Zap } from 'lucide-react'
import { useUi } from '@/store/ui'
import { isMac, MOD } from '../util'

export type SendMode = 'queue' | 'steer'

const itemCls = 'flex items-start gap-2 px-2 min-h-7 py-1 rounded text-[12px] outline-none data-[highlighted]:bg-selected cursor-pointer select-none'
const contentCls = 'z-40 min-w-[260px] rounded-md surface-float p-1 text-fg'
const ALT = isMac() ? '⌥' : 'Alt+'

/** Current default for Enter while a turn runs, from the persisted ui store. */
export function useDefaultSendMode(): SendMode { return useUi((s) => s.steerByDefault) ? 'steer' : 'queue' }

/**
 * Split control shown next to the send slot while the agent is running: the main button performs the default
 * (Queue or Steer), the chevron opens a menu to flip the default or send the draft in a new background thread.
 * Keyboard: Enter = default, Alt+Enter = the other one, Cmd/Ctrl+Shift+Enter = new thread.
 */
export function SendModeControl({ disabled, onSend, onSendInNewThread }: { disabled?: boolean; onSend: (mode: SendMode) => void; onSendInNewThread: () => void }) {
  const def = useDefaultSendMode()
  const set = useUi((s) => s.set)
  const Icon = def === 'steer' ? Zap : ListPlus
  const title = def === 'steer'
    ? `Steer (⏎): the agent reads this before its next step, inside the current turn. ${ALT}⏎ queues it for after the turn. ${MOD}⇧⏎ starts a new thread.`
    : `Queue (⏎): runs after the current turn finishes. ${ALT}⏎ steers it into the running turn instead. ${MOD}⇧⏎ starts a new thread.`
  return (
    <span className="inline-flex items-stretch h-6 rounded border border-border-strong bg-input text-[11px] overflow-hidden animate-pop" data-send-mode={def}>
      <button type="button" disabled={disabled} onClick={() => onSend(def)} title={title} className="flex items-center gap-1 px-1.5 hover:bg-hover disabled:opacity-40 disabled:hover:bg-transparent press">
        <Icon size={11} className={def === 'steer' ? 'text-warning' : 'text-accent'} />
        <span>{def === 'steer' ? 'Steer' : 'Queue'}</span>
      </button>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <button type="button" title="Change what Enter does while the agent is running" aria-label="Send options" className="flex items-center px-0.5 border-l border-border hover:bg-hover"><ChevronDown size={11} className="text-faint" /></button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content data-caret-pop side="top" align="end" sideOffset={6} className={contentCls}>
            <DropdownMenu.Label className="px-2 py-1 text-[10.5px] text-faint">While the agent is running, Enter will</DropdownMenu.Label>
            <DropdownMenu.Item className={itemCls} onSelect={() => set({ steerByDefault: false })}>
              <ListPlus size={13} className="mt-0.5 text-accent" />
              <div className="flex-1"><div>Queue</div><div className="text-[10.5px] text-faint">Runs after the current turn finishes. Editable and reorderable below the chat.</div></div>
              {def === 'queue' && <Check size={12} className="mt-0.5 text-accent" />}
            </DropdownMenu.Item>
            <DropdownMenu.Item className={itemCls} onSelect={() => set({ steerByDefault: true })}>
              <Zap size={13} className="mt-0.5 text-warning" />
              <div className="flex-1"><div>Steer</div><div className="text-[10.5px] text-faint">Injected before the agent's next step, inside the current turn. External agents fall back to send now.</div></div>
              {def === 'steer' && <Check size={12} className="mt-0.5 text-accent" />}
            </DropdownMenu.Item>
            <div className="px-2 pb-1 text-[10.5px] text-faint">{ALT}⏎ does the other one.</div>
            <DropdownMenu.Separator className="my-1 h-px bg-border" />
            <DropdownMenu.Item className={itemCls} disabled={disabled} onSelect={onSendInNewThread}>
              <MessageSquarePlus size={13} className="mt-0.5 text-muted" />
              <div className="flex-1"><div>Send in a new thread</div><div className="text-[10.5px] text-faint">Starts a background thread with this draft; you stay here.</div></div>
              <span className="text-[10.5px] text-faint whitespace-nowrap">{MOD}⇧⏎</span>
            </DropdownMenu.Item>
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </span>
  )
}

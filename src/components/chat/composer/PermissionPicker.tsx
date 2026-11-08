import { DropdownMenu } from 'radix-ui'
import { Check, ChevronDown, Shield, ShieldAlert, ShieldCheck, ShieldHalf, Sparkles } from 'lucide-react'
import type { PermissionMode } from '@/types'
import { useChat } from '@/store/chat'
import { useSettings } from '@/store/settings'
import { runCommand } from '@/lib/commands'
import { PERMISSION_MODES, effectivePermissionMode, isWithinCap, permissionModeLabel, permissionModeInfo, settingsPermissionMode } from '@/lib/permissionModes'

const ICON: Record<PermissionMode, typeof Shield> = { supervised: Shield, 'auto-accept-edits': ShieldCheck, auto: ShieldHalf, 'full-access': ShieldAlert }
const TONE: Record<PermissionMode, string> = { supervised: 'text-muted', 'auto-accept-edits': 'text-accent', auto: 'text-accent', 'full-access': 'text-warning' }

const itemCls = 'flex items-start gap-2 px-2 min-h-7 py-1 rounded text-[12px] outline-none data-[highlighted]:bg-selected cursor-pointer select-none data-[disabled]:opacity-45 data-[disabled]:cursor-not-allowed'
const contentCls = 'z-40 min-w-[260px] rounded-md surface-float p-1 text-fg'

/** Per-thread permission mode (supervised / auto-accept edits / auto / full access) with the settings cap applied. */
export function PermissionPicker({ sessionId }: { sessionId: string }) {
  const override = useChat((s) => s.loaded[sessionId]?.permissionMode ?? null)
  const setPermissionMode = useChat((s) => s.setPermissionMode)
  const agent = useSettings((s) => s.settings?.agent)
  const cap = agent?.maxPermissionMode
  const fromSettings = settingsPermissionMode(agent)
  const effective = effectivePermissionMode(agent, override)
  const Icon = ICON[effective]
  const capped = !!cap && !isWithinCap(override ?? fromSettings, cap)
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button type="button" data-testid="permission-picker" title={`Permissions: ${permissionModeLabel(effective)}${override ? ' (this thread)' : ' (default)'}${capped ? ` (capped at ${permissionModeLabel(cap!)})` : ''}`} className="flex items-center gap-1 h-6 px-1.5 rounded hover:bg-hover text-[11.5px] cursor-pointer">
          <Icon size={13} className={TONE[effective]} /><span className="whitespace-nowrap">{permissionModeInfo(effective).short}</span><ChevronDown size={11} className="text-faint" />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content data-caret-pop side="top" align="start" sideOffset={6} className={contentCls}>
          <DropdownMenu.Item className={itemCls} onSelect={() => void setPermissionMode(sessionId, null)}>
            <Sparkles size={13} className={override === null ? 'text-accent mt-0.5' : 'text-muted mt-0.5'} />
            <div className="flex-1"><div>Default (from settings)</div><div className="text-[10.5px] text-faint">Currently {permissionModeLabel(fromSettings)}{cap && !isWithinCap(fromSettings, cap) ? `, capped at ${permissionModeLabel(cap)}` : ''}</div></div>
            {override === null && <Check size={12} className="text-accent mt-1" />}
          </DropdownMenu.Item>
          <DropdownMenu.Separator className="my-1 h-px bg-border" />
          {PERMISSION_MODES.map((m) => {
            const I = ICON[m.id]
            const allowed = isWithinCap(m.id, cap)
            return (
              <DropdownMenu.Item key={m.id} className={itemCls} disabled={!allowed} onSelect={() => { if (allowed) void setPermissionMode(sessionId, m.id) }}>
                <I size={13} className={`mt-0.5 ${override === m.id ? TONE[m.id] : 'text-muted'}`} />
                <div className="flex-1"><div>{m.label}{!allowed && <span className="ml-1 text-[10px] text-faint">above cap</span>}</div><div className="text-[10.5px] text-faint">{m.description}</div></div>
                {override === m.id && <Check size={12} className="text-accent mt-1" />}
              </DropdownMenu.Item>
            )
          })}
          <DropdownMenu.Separator className="my-1 h-px bg-border" />
          <DropdownMenu.Item className={itemCls} onSelect={() => void runCommand('settings.open', { page: 'agent', query: 'permission' })}><Shield size={13} className="text-muted mt-0.5" /><span>Permission settings…</span></DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  )
}

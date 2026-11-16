import { useEffect, useState } from 'react'
import { DropdownMenu } from 'radix-ui'
import { Check, ChevronDown, ChevronRight, Cloud, CloudDownload, CloudUpload, Laptop, Loader2, Plus, Settings2, SquareTerminal, Trash2, Unlink } from 'lucide-react'
import type { SandboxBinding, WarrenInstance } from '@/types'
import { useChat } from '@/store/chat'
import { useSettings } from '@/store/settings'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import { subscribe } from '@/lib/bus'
import { confirmDialog } from '@/components/settings/primitives'
import { bindingStatusLine, isTransitional, pickerLabel, sortInstances, statusTone } from '@/lib/warren'

/** Bus topic the `sandbox.pick` palette command emits to open this picker for a thread. */
export const SANDBOX_PICK_TOPIC = 'chat:sandbox-pick'

const itemCls = 'flex items-start gap-2 px-2 min-h-7 py-1 rounded text-[12px] outline-none data-[highlighted]:bg-selected cursor-pointer select-none data-[disabled]:opacity-45 data-[disabled]:cursor-not-allowed'
const contentCls = 'z-40 min-w-[260px] rounded-md surface-float p-1 text-fg'
const DOT: Record<string, string> = { success: 'bg-success', warning: 'bg-warning', danger: 'bg-danger', muted: 'bg-faint' }

const toast = (level: 'info' | 'warn' | 'error' | 'success', title: string, message?: string) => void runCommand('app.notify', { level, title, message })
const errText = (e: unknown) => (e instanceof Error ? e.message : String(e))

/** Where this thread's commands run: this Mac or a Warren instance (new or existing). Bound threads get sync and terminal actions. */
export function SandboxPicker({ sessionId }: { sessionId: string }) {
  const enabled = useSettings((s) => s.settings?.warren?.enabled ?? false)
  const binding = useChat((s) => s.loaded[sessionId]?.sandbox ?? null)
  const setSandbox = useChat((s) => s.setSandbox)
  const [open, setOpen] = useState(false)
  const [instances, setInstances] = useState<WarrenInstance[] | null>(null)
  const [binding_, setBindingBusy] = useState<'new' | 'existing' | null>(null)
  const [syncing, setSyncing] = useState<'push' | 'pull' | null>(null)
  const [unbinding, setUnbinding] = useState(false)
  const provisioning = binding_ !== null

  useEffect(() => subscribe<{ sessionId: string }>(SANDBOX_PICK_TOPIC, (p) => { if (p.sessionId === sessionId && enabled) setOpen(true) }), [sessionId, enabled])
  useEffect(() => {
    if (!open || !enabled) return
    let alive = true
    api.warren.listInstances().then((l) => { if (alive) setInstances(sortInstances(l)) }).catch(() => { if (alive) setInstances([]) })
    return () => { alive = false }
  }, [open, enabled])

  const bind = async (instanceId: string | null) => {
    setBindingBusy(instanceId ? 'existing' : 'new')
    try {
      const b = await api.warren.bindThread(sessionId, instanceId)
      setSandbox(sessionId, b)
      toast('success', 'Sandbox ready', `Commands in this thread run in ${b.instanceName || b.instanceId}`)
    } catch (e) { toast('error', 'Could not bind the sandbox', errText(e)) }
    finally { setBindingBusy(null) }
  }
  const unbind = async (deleteInstance: boolean) => {
    if (!binding) return
    if (deleteInstance) {
      const ok = await confirmDialog({ title: `Delete ${binding.instanceName || binding.instanceId}?`, message: 'The instance and everything on its disk are destroyed. Files on this Mac are untouched.', confirmLabel: 'Unbind and delete', danger: true })
      if (!ok) return
    }
    setUnbinding(true)
    try { const view = await api.warren.unbindThread(sessionId, deleteInstance); setSandbox(sessionId, view.sandbox ?? null); toast('info', deleteInstance ? 'Instance deleted' : 'Sandbox unbound', 'Commands in this thread now run on this Mac.') }
    catch (e) { toast('error', 'Could not unbind the sandbox', errText(e)) }
    finally { setUnbinding(false) }
  }
  const sync = async (direction: 'push' | 'pull') => {
    if (!binding) return
    setSyncing(direction)
    try {
      const r = await api.warren.sync(sessionId, direction)
      const at = Math.floor(Date.now() / 1000)
      const cur = useChat.getState().loaded[sessionId]?.sandbox
      if (cur) setSandbox(sessionId, direction === 'push' ? { ...cur, lastPushAt: at } : { ...cur, lastPullAt: at })
      toast('success', direction === 'push' ? 'Workspace pushed' : 'Changes pulled', `${r.files} file${r.files === 1 ? '' : 's'}${r.deleted ? `, ${r.deleted} deleted` : ''} in ${r.ms} ms${direction === 'pull' && r.files ? ' — review them in the pending changes list' : ''}`)
    } catch (e) { toast('error', direction === 'push' ? 'Push failed' : 'Pull failed', errText(e)) }
    finally { setSyncing(null) }
  }
  const openTerminal = async () => {
    if (!binding) return
    try { const { url } = await api.warren.terminalUrl(binding.instanceId); await runCommand('editor.openBrowser', { url }) }
    catch (e) { toast('error', 'Could not open the terminal', errText(e)) }
  }

  const label = pickerLabel(binding)
  const busyAny = provisioning || unbinding || syncing !== null
  const tone = !binding ? 'text-muted' : binding.status === 'error' ? 'text-danger' : binding.status === 'ready' ? 'text-accent' : 'text-warning'

  if (!enabled) {
    return (
      <span title="Enable Warren in Settings → Sandboxes" className="inline-flex">
        <button type="button" disabled aria-disabled data-testid="sandbox-picker" className="flex items-center gap-1 h-6 px-1.5 rounded text-[11.5px] opacity-50 cursor-not-allowed pointer-events-none">
          <Cloud size={13} className="text-muted" /><span className="whitespace-nowrap">{label}</span><ChevronDown size={11} className="text-faint" />
        </button>
      </span>
    )
  }

  return (
    <DropdownMenu.Root open={open} onOpenChange={setOpen}>
      <DropdownMenu.Trigger asChild>
        <button type="button" data-testid="sandbox-picker" title={binding ? `Sandbox: ${binding.instanceName || binding.instanceId} (${binding.status})` : 'Sandbox: commands run on this Mac'} className="flex items-center gap-1 h-6 px-1.5 rounded hover:bg-hover text-[11.5px] cursor-pointer">
          {provisioning ? <Loader2 size={13} className="spin text-accent" /> : <Cloud size={13} className={tone} />}
          <span className="whitespace-nowrap">{provisioning ? 'Provisioning…' : label}</span>
          <ChevronDown size={11} className="text-faint" />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content data-caret-pop side="top" align="start" sideOffset={6} className={contentCls}>
          <DropdownMenu.Item className={itemCls} disabled={busyAny || !binding} onSelect={() => { if (binding) void unbind(false) }}>
            <Laptop size={13} className={!binding ? 'text-accent mt-0.5' : 'text-muted mt-0.5'} />
            <div className="flex-1"><div>Local (this Mac)</div><div className="text-[10.5px] text-faint">Commands run here, with the usual prompts</div></div>
            {!binding && <Check size={12} className="text-accent mt-1" />}
          </DropdownMenu.Item>
          <DropdownMenu.Separator className="my-1 h-px bg-border" />
          <DropdownMenu.Item className={itemCls} disabled={busyAny} onSelect={() => void bind(null)}>
            {binding_ === 'new' ? <Loader2 size={13} className="spin text-accent mt-0.5" /> : <Plus size={13} className="text-muted mt-0.5" />}
            <div className="flex-1"><div>Warren: new instance</div><div className="text-[10.5px] text-faint">{binding ? 'Replaces the current binding; the old instance stays' : 'Named after this folder, using the defaults in Settings'}</div></div>
          </DropdownMenu.Item>
          <DropdownMenu.Sub>
            <DropdownMenu.SubTrigger className={itemCls} disabled={busyAny}>
              {binding_ === 'existing' ? <Loader2 size={13} className="spin text-accent mt-0.5" /> : <Cloud size={13} className="text-muted mt-0.5" />}
              <div className="flex-1"><div>Warren: existing instance…</div><div className="text-[10.5px] text-faint">{instances ? `${instances.length} in the workspace` : 'Loading…'}</div></div>
              <ChevronRight size={12} className="text-faint mt-1" />
            </DropdownMenu.SubTrigger>
            <DropdownMenu.Portal>
              <DropdownMenu.SubContent data-caret-pop sideOffset={4} className={`${contentCls} max-h-72 overflow-y-auto`}>
                {!instances ? <div className="px-2 py-2 text-[11.5px] text-faint">Loading…</div>
                  : instances.length === 0 ? <div className="px-2 py-2 text-[11.5px] text-faint">No instances yet</div>
                  : instances.filter((i) => i.status !== 'deleting' && i.status !== 'deleted').map((i) => (
                    <DropdownMenu.Item key={i.id} className={itemCls} disabled={i.id === binding?.instanceId} onSelect={() => void bind(i.id)}>
                      <span className={`mt-1.5 h-2 w-2 rounded-full shrink-0 ${DOT[statusTone(i.status)]} ${isTransitional(i.status) ? 'animate-breathe' : ''}`} />
                      <div className="flex-1 min-w-0"><div className="truncate-1">{i.name || i.id}</div><div className="text-[10.5px] text-faint mono truncate-1">{i.id} · {i.status}</div></div>
                      {i.id === binding?.instanceId && <Check size={12} className="text-accent mt-1" />}
                    </DropdownMenu.Item>
                  ))}
              </DropdownMenu.SubContent>
            </DropdownMenu.Portal>
          </DropdownMenu.Sub>
          {binding && <BoundSection binding={binding} syncing={syncing} busy={busyAny} onSync={sync} onTerminal={openTerminal} onUnbind={unbind} />}
          <DropdownMenu.Separator className="my-1 h-px bg-border" />
          <DropdownMenu.Item className={itemCls} onSelect={() => void runCommand('settings.open', { page: 'sandboxes' })}><Settings2 size={13} className="text-muted mt-0.5" /><span>Sandbox settings…</span></DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  )
}

function BoundSection({ binding, syncing, busy, onSync, onTerminal, onUnbind }: { binding: SandboxBinding; syncing: 'push' | 'pull' | null; busy: boolean; onSync: (d: 'push' | 'pull') => void; onTerminal: () => void; onUnbind: (del: boolean) => void }) {
  const statusTone_ = binding.status === 'error' ? 'text-danger' : binding.status === 'ready' ? 'text-success' : 'text-warning'
  return (
    <>
      <DropdownMenu.Separator className="my-1 h-px bg-border" />
      <div className="px-2 py-1 text-[10.5px]" data-testid="sandbox-status">
        <div className="flex items-center gap-1.5"><span className={`h-1.5 w-1.5 rounded-full ${statusTone_.replace('text-', 'bg-')} ${binding.status === 'syncing' || binding.status === 'running' ? 'animate-breathe' : ''}`} /><span className="font-medium truncate-1">{binding.instanceName || binding.instanceId}</span><span className="mono text-faint">{binding.instanceId}</span></div>
        <div className={`mt-0.5 ${binding.status === 'error' ? 'text-danger' : 'text-faint'}`}>{bindingStatusLine(binding)}{binding.pushedFiles ? ` · ${binding.pushedFiles} files tracked` : ''}</div>
      </div>
      <DropdownMenu.Item className={itemCls} disabled={busy} onSelect={() => onSync('push')}>{syncing === 'push' ? <Loader2 size={13} className="spin text-accent mt-0.5" /> : <CloudUpload size={13} className="text-muted mt-0.5" />}<div className="flex-1"><div>Sync now (push)</div><div className="text-[10.5px] text-faint">Upload files changed since the last push</div></div></DropdownMenu.Item>
      <DropdownMenu.Item className={itemCls} disabled={busy} onSelect={() => onSync('pull')}>{syncing === 'pull' ? <Loader2 size={13} className="spin text-accent mt-0.5" /> : <CloudDownload size={13} className="text-muted mt-0.5" />}<div className="flex-1"><div>Pull changes</div><div className="text-[10.5px] text-faint">Bring remote edits back as pending changes</div></div></DropdownMenu.Item>
      <DropdownMenu.Item className={itemCls} onSelect={onTerminal}><SquareTerminal size={13} className="text-muted mt-0.5" /><div className="flex-1"><div>Open terminal</div><div className="text-[10.5px] text-faint">Browser shell in the instance</div></div></DropdownMenu.Item>
      <DropdownMenu.Separator className="my-1 h-px bg-border" />
      <DropdownMenu.Item className={itemCls} disabled={busy} onSelect={() => onUnbind(false)}><Unlink size={13} className="text-muted mt-0.5" /><span>Unbind (keep instance)</span></DropdownMenu.Item>
      <DropdownMenu.Item className={`${itemCls} text-danger`} disabled={busy} onSelect={() => onUnbind(true)}><Trash2 size={13} className="mt-0.5" /><span>Unbind and delete instance…</span></DropdownMenu.Item>
    </>
  )
}

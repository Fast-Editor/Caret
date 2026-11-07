import { useEffect, useMemo, useState } from 'react'
import * as DropdownMenu from '@radix-ui/react-dropdown-menu'
import { Bot, Check, Download, Settings2, Sparkles, AlertTriangle, ArrowRightLeft } from 'lucide-react'
import type { AcpAgentStatus } from '@/types'
import { isSessionBusy, useChat } from '@/store/chat'
import { useSettings } from '@/store/settings'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import { emit as busEmit, subscribe } from '@/lib/bus'

const itemCls = 'flex items-center gap-2 px-2 min-h-7 py-1 rounded text-[12px] outline-none data-[highlighted]:bg-selected cursor-pointer select-none data-[disabled]:opacity-50 data-[disabled]:cursor-default'
const contentCls = 'z-40 min-w-[240px] rounded-md surface-float p-1 text-fg'

export const CARET_AGENT = 'caret'

function stateDot(s?: AcpAgentStatus) {
  if (!s) return 'bg-faint'
  if (!s.installed || s.state === 'unsupported') return 'bg-faint'
  if (s.state === 'ready') return 'bg-success'
  if (s.state === 'starting') return 'bg-warning'
  if (s.state === 'error') return 'bg-danger'
  return 'bg-muted'
}

/** Which agent drives this chat: Caret's built-in agent or an external ACP agent (Claude Code, Codex, Gemini…). */
export function AgentPicker({ sessionId }: { sessionId: string }) {
  const enabled = useSettings((s) => s.settings?.acp?.enabled ?? true)
  const agentId = useChat((s) => s.draft(sessionId).agentId ?? CARET_AGENT)
  const update = useChat((s) => s.updateDraft)
  const session = useChat((s) => s.loaded[sessionId])
  const handoff = useChat((s) => s.handoffSession)
  const [agents, setAgents] = useState<AcpAgentStatus[]>([])
  const [open, setOpen] = useState(false)
  /** Pending "hand off this thread?" confirmation (agent id + display name). */
  const [pending, setPending] = useState<{ id: string; name: string } | null>(null)
  const [busyHandoff, setBusyHandoff] = useState(false)
  const load = () => api.acp.agents().then(setAgents).catch(() => setAgents([]))
  useEffect(() => { if (enabled) void load() }, [enabled])
  useEffect(() => { if (open && enabled) void load() }, [open, enabled])
  useEffect(() => subscribe('acp:changed', () => void load()), [])
  // Status events for this session keep the dot live.
  useEffect(() => subscribe('chat:agent-status', (p: unknown) => { const e = p as { sessionId: string; status: AcpAgentStatus }; if (e.sessionId !== sessionId) return; setAgents((prev) => prev.some((a) => a.id === e.status.id) ? prev.map((a) => (a.id === e.status.id ? e.status : a)) : [...prev, e.status]) }), [sessionId])
  const current = useMemo(() => agents.find((a) => a.id === agentId), [agents, agentId])
  if (!enabled) return null
  const label = agentId === CARET_AGENT ? 'Caret' : current?.name ?? agentId
  const nameOf = (id: string) => (id === CARET_AGENT ? 'Caret' : agents.find((a) => a.id === id)?.name ?? id)
  const apply = (id: string) => { update(sessionId, { agentId: id }); if (session) busEmit('chat:agent-changed', { sessionId, agentId: id }) }
  // Switching the agent of a thread that already has messages is a handoff: confirm, then let the backend transfer
  // the conversation within the budget. Empty threads just change the draft.
  const choose = (id: string) => {
    const sessionAgent = session?.agentId ?? CARET_AGENT
    const hasMessages = (session?.messages.length ?? 0) > 0
    if (!session || !hasMessages || id === sessionAgent) { apply(id); return }
    setPending({ id, name: nameOf(id) })
  }
  const confirmHandoff = async () => {
    if (!pending) return
    if (isSessionBusy(session?.status)) { void runCommand('app.notify', { level: 'warn', title: 'Agent is running', message: 'Wait for the agent to finish (or stop it) before handing off the thread.' }); return }
    setBusyHandoff(true)
    try { await handoff(sessionId, { agentId: pending.id }); apply(pending.id); setPending(null) }
    catch (e) { void runCommand('app.notify', { level: 'error', title: 'Handoff failed', message: e instanceof Error ? e.message : String(e) }) }
    finally { setBusyHandoff(false) }
  }
  return (
    <div className="relative">
    {pending && (
      <div role="dialog" aria-label="Hand off thread" className="absolute bottom-full left-0 mb-2 z-50 w-[300px] rounded-md surface-float p-3 text-[12px] text-fg animate-fade-up" data-testid="handoff-confirm">
        <div className="flex items-start gap-2"><ArrowRightLeft size={14} className="text-accent mt-0.5 shrink-0" /><div className="flex-1"><div className="font-medium">Hand off this thread to {pending.name}?</div><div className="mt-1 text-muted">The conversation is transferred within the budget: recent messages verbatim, older ones summarised. The thread stays in this list.</div></div></div>
        <div className="flex justify-end gap-1.5 mt-3"><button type="button" onClick={() => setPending(null)} className="px-2 h-6 rounded border border-border hover:bg-hover press">Cancel</button><button type="button" onClick={() => void confirmHandoff()} disabled={busyHandoff} className="px-2 h-6 rounded bg-accent text-accent-fg hover:bg-accent-hover disabled:opacity-50 press">{busyHandoff ? 'Handing off…' : 'Hand off'}</button></div>
      </div>
    )}
    <DropdownMenu.Root open={open} onOpenChange={setOpen}>
      <DropdownMenu.Trigger asChild>
        <button type="button" title={`Agent: ${label}${current?.version ? ` v${current.version}` : ''}`} className="flex items-center gap-1 h-6 px-1.5 rounded hover:bg-hover text-[11.5px] cursor-pointer" data-testid="agent-picker">
          {agentId === CARET_AGENT ? <Sparkles size={13} className="text-accent" /> : <Bot size={13} className="text-accent" />}
          <span className="max-w-[110px] truncate">{label}</span>
          {agentId !== CARET_AGENT && <span className={`inline-block w-1.5 h-1.5 rounded-full ${stateDot(current)}`} />}
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content data-caret-pop side="top" align="start" sideOffset={6} className={contentCls}>
          <DropdownMenu.Label className="px-2 py-1 text-[10.5px] uppercase tracking-wide text-faint">Agent</DropdownMenu.Label>
          <DropdownMenu.Item className={itemCls} onSelect={() => choose(CARET_AGENT)}>
            <Sparkles size={13} className={agentId === CARET_AGENT ? 'text-accent' : 'text-muted'} />
            <div className="flex-1"><div>Caret</div><div className="text-[10.5px] text-faint">Built-in agent with your configured models</div></div>
            {agentId === CARET_AGENT && <Check size={12} className="text-accent" />}
          </DropdownMenu.Item>
          {agents.map((a) => {
            const installed = a.installed && a.state !== 'unsupported'
            return (
              <DropdownMenu.Item key={a.id} className={itemCls} disabled={!installed} onSelect={() => (installed ? choose(a.id) : void runCommand('settings.open', { page: 'agents', query: a.name }))}>
                <span className={`inline-block w-2 h-2 rounded-full ${stateDot(a)}`} />
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-1.5">{a.name}{a.version && <span className="text-[10px] text-faint">v{a.version}</span>}{a.authRequired && <AlertTriangle size={11} className="text-warning" />}</div>
                  <div className="text-[10.5px] text-faint truncate">{!installed ? 'Not installed — open Agents settings' : a.state === 'error' ? a.error ?? 'Error' : a.state === 'ready' ? 'Connected' : 'Starts on first message'}</div>
                </div>
                {!installed && <Download size={12} className="text-muted" />}
                {a.id === agentId && <Check size={12} className="text-accent" />}
              </DropdownMenu.Item>
            )
          })}
          <DropdownMenu.Separator className="my-1 h-px bg-border" />
          <DropdownMenu.Item className={itemCls} onSelect={() => void runCommand('settings.open', { page: 'agents' })}><Settings2 size={13} className="text-muted" /> Manage agents…</DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
    </div>
  )
}

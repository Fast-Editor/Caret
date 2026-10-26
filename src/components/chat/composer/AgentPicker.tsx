import { useEffect, useMemo, useState } from 'react'
import * as DropdownMenu from '@radix-ui/react-dropdown-menu'
import { Bot, Check, Download, Settings2, Sparkles, AlertTriangle } from 'lucide-react'
import type { AcpAgentStatus } from '@/types'
import { useChat } from '@/store/chat'
import { useSettings } from '@/store/settings'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import { emit as busEmit, subscribe } from '@/lib/bus'

const itemCls = 'flex items-center gap-2 px-2 min-h-7 py-1 rounded text-[12px] outline-none data-[highlighted]:bg-selected cursor-pointer select-none data-[disabled]:opacity-50 data-[disabled]:cursor-default'
const contentCls = 'z-40 min-w-[240px] rounded-md border border-border bg-tooltip p-1 shadow-[var(--shadow)] text-fg'

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
  const [agents, setAgents] = useState<AcpAgentStatus[]>([])
  const [open, setOpen] = useState(false)
  const load = () => api.acp.agents().then(setAgents).catch(() => setAgents([]))
  useEffect(() => { if (enabled) void load() }, [enabled])
  useEffect(() => { if (open && enabled) void load() }, [open, enabled])
  useEffect(() => subscribe('acp:changed', () => void load()), [])
  // Status events for this session keep the dot live.
  useEffect(() => subscribe('chat:agent-status', (p: unknown) => { const e = p as { sessionId: string; status: AcpAgentStatus }; if (e.sessionId !== sessionId) return; setAgents((prev) => prev.some((a) => a.id === e.status.id) ? prev.map((a) => (a.id === e.status.id ? e.status : a)) : [...prev, e.status]) }), [sessionId])
  const current = useMemo(() => agents.find((a) => a.id === agentId), [agents, agentId])
  if (!enabled) return null
  const label = agentId === CARET_AGENT ? 'Caret' : current?.name ?? agentId
  const choose = (id: string) => { update(sessionId, { agentId: id }); if (session) busEmit('chat:agent-changed', { sessionId, agentId: id }) }
  return (
    <DropdownMenu.Root open={open} onOpenChange={setOpen}>
      <DropdownMenu.Trigger asChild>
        <button type="button" title={`Agent: ${label}${current?.version ? ` v${current.version}` : ''}`} className="flex items-center gap-1 h-6 px-1.5 rounded hover:bg-hover text-[11.5px] cursor-pointer" data-testid="agent-picker">
          {agentId === CARET_AGENT ? <Sparkles size={13} className="text-accent" /> : <Bot size={13} className="text-accent" />}
          <span className="max-w-[110px] truncate">{label}</span>
          {agentId !== CARET_AGENT && <span className={`inline-block w-1.5 h-1.5 rounded-full ${stateDot(current)}`} />}
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content side="top" align="start" sideOffset={6} className={contentCls}>
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
  )
}

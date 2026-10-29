import { useEffect, useMemo, useState } from 'react'
import { Popover } from 'radix-ui'
import { Check, ChevronDown, Cpu, Gauge, Search, Settings2, ShieldCheck } from 'lucide-react'
import type { AcpAgentStatus, AcpConfigOption } from '@/types'
import { useChat } from '@/store/chat'
import { api } from '@/lib/api'
import { subscribe } from '@/lib/bus'
import { CARET_AGENT } from './AgentPicker'

const itemCls = 'flex items-center gap-2 px-2 min-h-7 py-1 rounded text-[12px] outline-none data-[highlighted]:bg-selected cursor-pointer select-none'
const contentCls = 'z-40 min-w-[220px] rounded-md surface-float p-1 text-fg'

function iconFor(category?: string, id?: string) {
  if (category === 'model' || id === 'model') return Cpu
  if (category === 'thought_level' || id === 'effort') return Gauge
  if (category === 'mode' || id === 'mode') return ShieldCheck
  return Settings2
}

/**
 * Model / effort / permission-mode pickers for the external ACP agent driving this chat. They render the agent's own
 * "session config options" (ACP `session/new` → `configOptions`, changed with `session/set_config_option`), so the
 * list is whatever the agent offers (Claude Code: model + effort + permission mode; OpenCode: every configured model).
 * The agent session is prepared as soon as an external agent is picked so the options are known before the first
 * message; picks are remembered per agent for future chats (settings `acp.sessionDefaults`).
 */
export function AgentOptions({ sessionId }: { sessionId: string }) {
  const agentId = useChat((s) => s.draft(sessionId).agentId ?? CARET_AGENT)
  const hasSession = useChat((s) => !!s.loaded[sessionId])
  const [status, setStatus] = useState<AcpAgentStatus | null>(null)
  const [error, setError] = useState<string | null>(null)
  const external = agentId !== CARET_AGENT
  useEffect(() => {
    if (!external || !hasSession) { setStatus(null); return }
    let alive = true
    setError(null)
    api.acp.sessionInfo(sessionId).then((info) => {
      if (!alive) return
      if (info && info.agentId === agentId && info.configOptions) {
        setStatus({ id: agentId, name: agentId, installed: true, state: 'ready', configOptions: info.configOptions, modes: info.modes?.availableModes, currentMode: info.modes?.currentModeId })
        return
      }
      return api.acp.prepareSession(sessionId, agentId).then((st) => { if (alive) setStatus(st) })
    }).catch((e) => { if (alive) setError(String((e as Error).message ?? e)) })
    return () => { alive = false }
  }, [external, agentId, sessionId, hasSession])
  useEffect(() => subscribe('chat:agent-status', (p: unknown) => {
    const e = p as { sessionId: string; agentId: string; status: AcpAgentStatus }
    if (e.sessionId === sessionId && e.agentId === agentId && (e.status.configOptions || e.status.modes)) setStatus(e.status)
  }), [sessionId, agentId])
  if (!external) return null
  const opts = (status?.configOptions ?? []).filter((o) => o.type === 'select' && o.options?.length > 0)
  const hasModeOpt = opts.some((o) => o.category === 'mode' || o.id === 'mode')
  const modes = !hasModeOpt && status?.modes?.length ? status.modes : null
  const setOpt = async (configId: string, value: string) => { try { setStatus(await api.acp.setConfigOption(sessionId, configId, value)) } catch (e) { setError(String((e as Error).message ?? e)) } }
  const setMode = async (modeId: string) => { try { setStatus(await api.acp.setMode(sessionId, modeId)) } catch (e) { setError(String((e as Error).message ?? e)) } }
  if (!opts.length && !modes) return error ? <span title={error} className="text-[10.5px] text-danger px-1 truncate max-w-[200px]">agent options unavailable</span> : null
  return (
    <>
      {opts.map((o) => <OptionPicker key={o.id} option={o} onPick={(v) => void setOpt(o.id, v)} />)}
      {modes && (
        <OptionPicker option={{ id: 'mode', name: 'Mode', category: 'mode', type: 'select', currentValue: status?.currentMode, options: modes.map((m) => ({ value: m.id, name: m.name, description: m.description })) }} onPick={(v) => void setMode(v)} />
      )}
      {error && <span title={error} className="text-[10.5px] text-danger px-1 truncate max-w-[160px]">{error}</span>}
    </>
  )
}

function OptionPicker({ option, onPick }: { option: AcpConfigOption; onPick: (value: string) => void }) {
  const Icon = iconFor(option.category, option.id)
  const curValue = option.currentValue == null ? '' : String(option.currentValue)
  const cur = option.options.find((c) => c.value === curValue)
  const label = cur?.name ?? (curValue || option.name)
  const [open, setOpen] = useState(false)
  const [q, setQ] = useState('')
  const searchable = option.options.length > 10
  const filtered = useMemo(() => (q ? option.options.filter((c) => `${c.name} ${c.value} ${c.description ?? ''}`.toLowerCase().includes(q.toLowerCase())) : option.options), [option.options, q])
  const pick = (v: string) => { onPick(v); setOpen(false); setQ('') }
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild>
        <button type="button" title={`${option.name}: ${label}${option.description ? ` — ${option.description}` : ''}`} className="flex items-center gap-1 h-6 px-1.5 rounded hover:bg-hover text-[11.5px] max-w-[180px] cursor-pointer" data-testid={`agent-option-${option.id}`}>
          <Icon size={12} className="text-muted" /><span className="truncate-1">{label}</span><ChevronDown size={11} className="text-faint" />
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content data-caret-pop side="top" align="start" sideOffset={6} className={`${contentCls} w-[300px] p-0`}>
          {searchable
            ? <div className="flex items-center gap-1.5 px-2 h-8 border-b border-border"><Search size={12} className="text-faint" /><input autoFocus value={q} onChange={(e) => setQ(e.target.value)} placeholder={`Search ${option.name.toLowerCase()}…`} className="flex-1 bg-transparent outline-none text-[12px]" /></div>
            : <div className="px-2 py-1 text-[10.5px] uppercase tracking-wide text-faint border-b border-border">{option.name}</div>}
          <div className="max-h-80 overflow-y-auto p-1">
            {filtered.map((c) => {
              const sel = c.value === curValue
              return (
                <div key={c.value} role="option" aria-selected={sel} onClick={() => pick(c.value)} className={`${itemCls} hover:bg-selected`}>
                  <div className="flex-1 min-w-0"><div className="truncate-1">{c.name}</div>{(c.description || c.value.toLowerCase() !== c.name.toLowerCase()) && <div className="text-[10.5px] text-faint truncate-1">{c.description ?? c.value}</div>}</div>
                  {sel && <Check size={12} className="text-accent" />}
                </div>
              )
            })}
            {filtered.length === 0 && <div className="px-2 py-3 text-faint text-center text-[11.5px]">No matches</div>}
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}

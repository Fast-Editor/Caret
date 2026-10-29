import { useEffect, useMemo, useState } from 'react'
import { DropdownMenu, Popover } from 'radix-ui'
import { Bot, Brain, Bug, Check, ChevronDown, Gauge, ListChecks, MessageCircle, Search, Sparkles, Star, WandSparkles, Zap } from 'lucide-react'
import type { Effort, ModeId, ThinkingPref } from '@/types'
import { AUTO_MODEL, useChat, useModelGroups, type ModeDescriptor } from '@/store/chat'
import { runCommand } from '@/lib/commands'
import { autoSummary, initRouterStore, useRouter } from '@/store/router'
import { fmtTokens } from '../util'

const MODE_ICON: Record<string, typeof Bot> = { bot: Bot, 'message-circle': MessageCircle, 'list-checks': ListChecks, bug: Bug, sparkles: Sparkles }
export function modeIcon(m: ModeDescriptor | undefined) { return (m?.icon && MODE_ICON[m.icon]) || (m?.id === 'ask' ? MessageCircle : m?.id === 'plan' ? ListChecks : m?.id === 'debug' ? Bug : Bot) }

const itemCls = 'flex items-center gap-2 px-2 min-h-7 py-1 rounded text-[12px] outline-none data-[highlighted]:bg-selected cursor-pointer select-none'
const contentCls = 'z-40 min-w-[220px] rounded-md surface-float p-1 text-fg'

export function ModeSelector({ sessionId }: { sessionId: string }) {
  const modes = useChat((s) => s.modes)
  const mode = useChat((s) => s.draft(sessionId).mode)
  const setMode = useChat((s) => s.setMode)
  const cur = modes.find((m) => m.id === mode)
  const Icon = modeIcon(cur)
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button type="button" title={`Mode: ${cur?.name ?? mode} (⌘. to cycle)`} className="flex items-center gap-1 h-6 px-1.5 rounded hover:bg-hover text-[11.5px] cursor-pointer"><Icon size={13} className="text-accent" /><span>{cur?.name ?? mode}</span><ChevronDown size={11} className="text-faint" /></button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content data-caret-pop side="top" align="start" sideOffset={6} className={contentCls}>
          {modes.map((m) => { const I = modeIcon(m); return (
            <DropdownMenu.Item key={m.id} className={itemCls} onSelect={() => void setMode(sessionId, m.id as ModeId)}>
              <I size={13} className={m.id === mode ? 'text-accent' : 'text-muted'} />
              <div className="flex-1"><div>{m.name}{!m.builtin && <span className="ml-1 text-[10px] text-faint">custom</span>}</div><div className="text-[10.5px] text-faint">{m.description}</div></div>
              {m.id === mode && <Check size={12} className="text-accent" />}
            </DropdownMenu.Item>
          ) })}
          <DropdownMenu.Separator className="my-1 h-px bg-border" />
          <DropdownMenu.Item className={itemCls} onSelect={() => void runCommand('settings.open', { page: 'agent', query: 'custom modes' })}><Sparkles size={13} className="text-muted" /> Add custom mode…</DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  )
}

export function ModelPicker({ sessionId }: { sessionId: string }) {
  const groups = useModelGroups()
  const modelId = useChat((s) => s.draft(sessionId).modelId)
  const setModel = useChat((s) => s.setModel)
  const favs = useChat((s) => s.favoriteModels)
  const toggleFav = useChat((s) => s.toggleFavoriteModel)
  const [q, setQ] = useState('')
  const [open, setOpen] = useState(false)
  const routerStatus = useRouter((s) => s.status)
  useEffect(() => { initRouterStore() }, [])
  const auto = autoSummary(routerStatus)
  const all = useMemo(() => groups.flatMap((g) => g.models.map((m) => ({ ...m, providerName: g.providerName }))), [groups])
  const cur = all.find((m) => m.id === modelId)
  const filtered = useMemo(() => (q ? all.filter((m) => `${m.displayName} ${m.model} ${m.providerName}`.toLowerCase().includes(q.toLowerCase())) : all), [all, q])
  const favList = filtered.filter((m) => favs.includes(m.id))
  const byProvider = groups.map((g) => ({ ...g, models: filtered.filter((m) => m.providerId === g.providerId) })).filter((g) => g.models.length)
  const pick = (id: string) => { void setModel(sessionId, id); setOpen(false); setQ('') }
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild>
        <button type="button" title="Model" className="flex items-center gap-1 h-6 px-1.5 rounded hover:bg-hover text-[11.5px] max-w-[180px] cursor-pointer">
          {cur?.isFast && <Zap size={11} className="text-warning" />}<span className="truncate-1">{modelId === AUTO_MODEL ? 'Auto' : cur?.displayName ?? modelId}</span><ChevronDown size={11} className="text-faint" />
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content data-caret-pop side="top" align="start" sideOffset={6} className={`${contentCls} w-[300px] p-0`}>
          <div className="flex items-center gap-1.5 px-2 h-8 border-b border-border"><Search size={12} className="text-faint" /><input autoFocus value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search models…" className="flex-1 bg-transparent outline-none text-[12px]" /></div>
          <div className="max-h-80 overflow-y-auto p-1">
            <button type="button" onClick={() => pick(AUTO_MODEL)} className={`${itemCls} w-full hover:bg-selected`}><WandSparkles size={13} className="text-accent" /><div className="flex-1 text-left"><div>Auto{auto.routed && <span className="ml-1 text-[10px] text-success">● routed</span>}</div><div className="text-[10.5px] text-faint">{auto.text}</div></div>{modelId === AUTO_MODEL && <Check size={12} className="text-accent" />}</button>
            {favList.length > 0 && <div className="px-2 pt-1.5 pb-0.5 text-[10.5px] text-faint">Favorites</div>}
            {favList.map((m) => <ModelRow key={`fav-${m.id}`} m={m} selected={m.id === modelId} fav onPick={pick} onFav={toggleFav} />)}
            {byProvider.map((g) => (
              <div key={g.providerId}><div className="px-2 pt-1.5 pb-0.5 text-[10.5px] text-faint">{g.providerName}</div>{g.models.map((m) => <ModelRow key={m.id} m={{ ...m, providerName: g.providerName }} selected={m.id === modelId} fav={favs.includes(m.id)} onPick={pick} onFav={toggleFav} />)}</div>
            ))}
            {all.length === 0 && <div className="px-2 py-3 text-faint text-center text-[11.5px]">No models enabled</div>}
          </div>
          <div className="border-t border-border p-1"><button type="button" onClick={() => { setOpen(false); void runCommand('settings.open', { page: 'models' }) }} className={`${itemCls} w-full hover:bg-selected text-accent`}>Add or configure models…</button></div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}

function ModelRow({ m, selected, fav, onPick, onFav }: { m: { id: string; displayName: string; model: string; contextWindow: number; inputPricePerM?: number; outputPricePerM?: number; supportsThinking: boolean; supportsVision: boolean; isFast?: boolean; providerName: string }; selected: boolean; fav: boolean; onPick: (id: string) => void; onFav: (id: string) => void }) {
  return (
    <div className={`${itemCls} group/row hover:bg-selected`} onClick={() => onPick(m.id)} role="option" aria-selected={selected}>
      <div className="flex-1 min-w-0"><div className="flex items-center gap-1 truncate-1">{m.displayName}{m.isFast && <Zap size={10} className="text-warning" />}{m.supportsThinking && <Brain size={10} className="text-faint" />}</div>
        <div className="text-[10.5px] text-faint truncate-1">{fmtTokens(m.contextWindow)} ctx{m.inputPricePerM !== undefined ? ` · $${m.inputPricePerM}/$${m.outputPricePerM ?? '?'} per M` : ''}{m.supportsVision ? ' · vision' : ''}</div></div>
      <button type="button" onClick={(e) => { e.stopPropagation(); onFav(m.id) }} title={fav ? 'Unfavorite' : 'Favorite'} className={`${fav ? 'text-warning' : 'opacity-0 group-hover/row:opacity-100 text-faint'}`}><Star size={11} fill={fav ? 'currentColor' : 'none'} /></button>
      {selected && <Check size={12} className="text-accent" />}
    </div>
  )
}

const EFFORTS: Effort[] = ['low', 'medium', 'high', 'xhigh', 'max']
export function ThinkingControls({ sessionId, supportsThinking, supportsEffort }: { sessionId: string; supportsThinking: boolean; supportsEffort: boolean }) {
  const thinking = useChat((s) => s.draft(sessionId).thinking)
  const effort = useChat((s) => s.draft(sessionId).effort)
  const update = useChat((s) => s.updateDraft)
  if (!supportsThinking && !supportsEffort) return null
  const cycle: ThinkingPref[] = ['auto', 'on', 'off']
  return (
    <div className="flex items-center gap-0.5">
      {supportsThinking && <button type="button" onClick={() => update(sessionId, { thinking: cycle[(cycle.indexOf(thinking) + 1) % 3] })} title={`Thinking: ${thinking}`} className={`flex items-center gap-1 h-6 px-1.5 rounded hover:bg-hover text-[11px] cursor-pointer ${thinking === 'off' ? 'text-faint' : 'text-fg'}`}><Brain size={12} className={thinking === 'off' ? '' : 'text-accent'} />{thinking !== 'auto' && <span>{thinking}</span>}</button>}
      {supportsEffort && (
        <DropdownMenu.Root>
          <DropdownMenu.Trigger asChild><button type="button" title={`Effort: ${effort}`} className="flex items-center gap-1 h-6 px-1.5 rounded hover:bg-hover text-[11px] cursor-pointer"><Gauge size={12} className="text-muted" /><span>{effort}</span></button></DropdownMenu.Trigger>
          <DropdownMenu.Portal><DropdownMenu.Content data-caret-pop side="top" align="start" sideOffset={6} className={contentCls}>
            {EFFORTS.map((e) => <DropdownMenu.Item key={e} className={itemCls} onSelect={() => update(sessionId, { effort: e })}><span className="flex-1 capitalize">{e}</span>{e === effort && <Check size={12} className="text-accent" />}</DropdownMenu.Item>)}
          </DropdownMenu.Content></DropdownMenu.Portal>
        </DropdownMenu.Root>
      )}
    </div>
  )
}

export function ContextRing({ used, total, onClick }: { used: number; total: number; onClick?: () => void }) {
  const pct = total > 0 ? Math.min(1, used / total) : 0
  const r = 6; const c = 2 * Math.PI * r
  const color = pct > 0.9 ? 'var(--danger)' : pct > 0.8 ? 'var(--warning)' : 'var(--accent)'
  return (
    <button type="button" onClick={onClick} title={`Context: ${fmtTokens(used)} / ${fmtTokens(total)} tokens (${Math.round(pct * 100)}%). Click to summarize.`} className="flex items-center gap-1 h-6 px-1 rounded hover:bg-hover text-[10.5px] text-muted">
      <svg width="16" height="16" viewBox="0 0 16 16"><circle cx="8" cy="8" r={r} fill="none" stroke="var(--border-strong)" strokeWidth="2" /><circle cx="8" cy="8" r={r} fill="none" stroke={color} strokeWidth="2" strokeDasharray={`${c * pct} ${c}`} strokeLinecap="round" transform="rotate(-90 8 8)" /></svg>
      <span>{Math.round(pct * 100)}%</span>
    </button>
  )
}

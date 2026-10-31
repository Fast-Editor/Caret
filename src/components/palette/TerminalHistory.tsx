import { Command } from 'cmdk'
import { useEffect, useRef, useState } from 'react'
import type { HistorySearchHit } from '@/types'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import { exitBadge, isNaturalLanguage, matchIndices, queryTokens, relativeCwd, timeAgo } from '@/lib/terminalHistory'
import { Kbd } from '@/components/common'
import { terminalRegistry } from '@/components/terminal/registry'
import { useTerminals } from '@/store/terminal'
import { useUi } from '@/store/ui'
import { useWorkspace } from '@/store/workspace'
import { notify } from '@/store/notifications'
import { PaletteShell, PaletteItem, Highlight } from './PaletteShell'

const DEBOUNCE_MS = 250
const RECENT_LIMIT = 60
const SEARCH_LIMIT = 50

async function copyText(text: string): Promise<void> {
  try { const { writeText } = await import('@tauri-apps/plugin-clipboard-manager'); await writeText(text) } catch { await navigator.clipboard.writeText(text).catch(() => {}) }
}

/** Type the command into the active terminal (creating one when none exists); `run` appends a newline. */
async function insertIntoTerminal(command: string, run: boolean): Promise<void> {
  if (useTerminals.getState().activeTerminalId) await runCommand('terminal.focus')
  else await runCommand('terminal.new')
  const id = useTerminals.getState().activeTerminalId
  if (!id) return
  const send = (tries: number) => {
    const h = terminalRegistry.get(id)
    if (h?.ptyId()) { h.sendInput(run ? `${command}\n` : command); h.focus() } else if (tries > 0) setTimeout(() => send(tries - 1), 100)
  }
  send(30)
}

/**
 * ⌘⌥H — persisted, cross-session command history. 1–2 word queries are lexical; 3+ words go through the
 * model-assisted search (`nlSearch`). Enter inserts without running, ⌘Enter runs, ⌘C copies the highlighted row.
 */
export function TerminalHistoryPalette({ initial = '' }: { initial?: string }) {
  const close = useUi((s) => s.closePalette)
  const root = useWorkspace((s) => s.workspace?.root)
  const activeCwd = useTerminals((s) => (s.activeTerminalId ? s.sessions[s.activeTerminalId]?.cwd : undefined))
  const [value, setValue] = useState(initial)
  const [hits, setHits] = useState<HistorySearchHit[]>([])
  const [keywords, setKeywords] = useState<string[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const seq = useRef(0)
  const runOnSelect = useRef(false)
  const byId = useRef(new Map<string, HistorySearchHit>())

  const q = value.trim()
  useEffect(() => {
    const my = ++seq.current
    const t = window.setTimeout(async () => {
      setLoading(true)
      try {
        let next: HistorySearchHit[]
        let kw: string[] = []
        if (!q) next = (await api.terminalHistory.recent({ cwd: activeCwd, limit: RECENT_LIMIT })).map((entry) => ({ entry, score: 0, why: '' }))
        else if (isNaturalLanguage(q)) { const r = await api.terminalHistory.nlSearch(q, SEARCH_LIMIT); next = r.hits; kw = r.usedModel ? r.keywords : [] }
        else next = await api.terminalHistory.search(q, SEARCH_LIMIT)
        if (my !== seq.current) return
        setHits(next); setKeywords(kw); setError(null)
      } catch (e) {
        if (my !== seq.current) return
        setHits([]); setKeywords([]); setError(String((e as Error).message ?? e))
      } finally { if (my === seq.current) setLoading(false) }
    }, q ? DEBOUNCE_MS : 0)
    return () => window.clearTimeout(t)
  }, [q, activeCwd])

  byId.current = new Map(hits.map((h) => [h.entry.id.toLowerCase(), h]))
  const tokens = queryTokens(q).concat(keywords)

  const pick = (hit: HistorySearchHit) => { const run = runOnSelect.current; runOnSelect.current = false; close(); void insertIntoTerminal(hit.entry.command, run) }
  const highlighted = (rootEl: HTMLElement): HistorySearchHit | undefined => {
    const v = rootEl.querySelector('[cmdk-item][data-selected="true"]')?.getAttribute('data-value')
    return v ? byId.current.get(v.toLowerCase()) : undefined
  }
  const onKeyDown = (e: React.KeyboardEvent) => {
    const mod = e.metaKey || e.ctrlKey
    // cmdk dispatches the item select on Enter regardless of modifiers; remember whether ⌘ was held so `pick` can run it.
    if (e.key === 'Enter') { runOnSelect.current = mod; return }
    if (mod && e.key.toLowerCase() === 'c' && !window.getSelection()?.toString()) {
      const hit = highlighted(e.currentTarget as HTMLElement)
      if (!hit) return
      e.preventDefault(); e.stopPropagation()
      void copyText(hit.entry.command).then(() => notify('success', 'Command copied', undefined, { timeoutMs: 1200 }))
    }
  }

  const count = `${hits.length} ${hits.length === 1 ? 'command' : 'commands'}`
  return (
    <PaletteShell label="Terminal command history" value={value} onValueChange={setValue} onClose={close} shouldFilter={false} onKeyDown={onKeyDown}
      placeholder="Search terminal history… (describe it in a few words for AI-assisted search)"
      footer={<><span><Kbd keys="enter" /> insert</span><span><Kbd keys="mod+enter" /> run</span><span><Kbd keys="mod+c" /> copy</span><span className="ml-auto">{loading ? 'Searching…' : count}</span></>}>
      {keywords.length > 0 && <div className="px-2 py-1 text-[10.5px] text-faint truncate" data-testid="history-ai-keywords">AI keywords: {keywords.join(', ')}</div>}
      {error && <div className="px-2 py-1 text-[11px] text-danger truncate">{error}</div>}
      <Command.Empty className="px-3 py-6 text-center text-xs text-muted">
        {loading ? 'Searching…' : q ? 'No matching commands' : 'No command history yet — run a command in a terminal with shell integration.'}
      </Command.Empty>
      {hits.map((h) => {
        const e = h.entry
        const badge = exitBadge(e)
        return (
          <PaletteItem key={e.id} value={e.id} onSelect={() => pick(h)}>
            <span className="font-mono text-[12px] text-fg truncate flex-1 min-w-0" title={h.why ? `${e.command}\n${h.why}` : e.command}>
              <Highlight text={e.command} indices={matchIndices(e.command, tokens)} />
            </span>
            <span className="ml-auto flex items-center gap-2 shrink-0 text-[10px]">
              {badge && <span className="rounded px-1 bg-danger/15 text-danger">{badge}</span>}
              <span className="text-faint truncate max-w-[200px]" title={e.cwd}>{relativeCwd(e.cwd, root)}</span>
              <span className="text-muted tabular-nums">{timeAgo(e.ts)}</span>
            </span>
          </PaletteItem>
        )
      })}
    </PaletteShell>
  )
}

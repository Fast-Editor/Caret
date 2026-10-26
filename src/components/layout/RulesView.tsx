import { BookMarked, Brain, Database, Plug, ChevronRight, Terminal, Sparkles, Users } from 'lucide-react'
import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import { Events, on } from '@/lib/tauri'
import { useWorkspace } from '@/store/workspace'

/** Compact "Rules & Context" sidebar: counts of rules, memories, MCP servers, skills, commands, subagents, index state → deep links into settings. */
export function RulesView() {
  const ws = useWorkspace((s) => s.workspace)
  const [counts, setCounts] = useState<Record<string, number | string>>({})
  const load = async () => {
    const safe = async <T,>(p: Promise<T>, f: T) => { try { return await p } catch { return f } }
    const [rules, mems, mcp, skills, cmds, subs, idx, docs] = await Promise.all([
      safe(api.rules.list(), []), safe(api.memories.list(), []), safe(api.mcp.servers(), []), safe(api.skills.list(), []), safe(api.commands.list(), []), safe(api.subagents.list(), []), safe(api.index.status(), null), safe(api.docs.list(), []),
    ])
    setCounts({ rules: rules.length, memories: mems.length, mcp: `${mcp.filter((m) => m.state === 'connected').length}/${mcp.length}`, skills: skills.length, commands: cmds.length, subagents: subs.length, index: idx ? `${idx.state}${idx.state === 'indexing' || idx.state === 'embedding' ? ` ${Math.round(idx.progress * 100)}%` : ''}` : '—', docs: docs.length })
  }
  useEffect(() => { void load(); const offs = [on(Events.rulesChanged, () => void load()), on(Events.mcpChanged, () => void load()), on(Events.indexStatus, () => void load())]; return () => offs.forEach((o) => o()) // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ws?.root])
  const rows = [
    { icon: BookMarked, label: 'Rules', value: counts.rules, page: 'rules' }, { icon: Brain, label: 'Memories', value: counts.memories, page: 'memories' },
    { icon: Plug, label: 'MCP servers', value: counts.mcp, page: 'mcp' }, { icon: Database, label: 'Codebase index', value: counts.index, page: 'indexing' }, { icon: Database, label: 'Docs', value: counts.docs, page: 'indexing' },
    { icon: Sparkles, label: 'Skills', value: counts.skills, page: 'skills' }, { icon: Terminal, label: 'Commands', value: counts.commands, page: 'skills' }, { icon: Users, label: 'Subagents', value: counts.subagents, page: 'skills' },
  ]
  return (
    <div className="flex flex-col h-full text-xs">
      <div className="h-[26px] px-3 flex items-center text-[11px] font-semibold uppercase tracking-wide text-muted">Rules & Context</div>
      {!ws && <div className="px-3 text-muted">Open a folder to see project rules, MCP servers and index status.</div>}
      {rows.map((r) => (
        <button key={r.label} onClick={() => void runCommand('settings.open', { page: r.page })} className="flex items-center gap-2 h-7 px-3 hover:bg-hover text-left">
          <r.icon className="size-3.5 text-muted" /><span className="flex-1">{r.label}</span><span className="text-muted">{r.value ?? '—'}</span><ChevronRight className="size-3.5 text-faint" />
        </button>
      ))}
      <div className="mt-3 px-3 text-[11px] text-faint">Caret reads <span className="mono">.caret/</span> and Cursor-compatible <span className="mono">.cursor/</span> project directories (rules, mcp.json, hooks, commands, skills, agents).</div>
    </div>
  )
}

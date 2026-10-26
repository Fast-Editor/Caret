import { useEffect, useState } from 'react'
import { ChevronDown, ChevronRight, ListTree } from 'lucide-react'
import { useEditor, activeTab, activeEditor } from '@/store/editor'
import { getDocumentSymbols, flattenSymbols, type FlatSymbol } from './symbols'
import { SymbolIcon } from './SymbolIcon'
import { subscribe } from '@/lib/bus'

export function OutlineView() {
  const activeId = useEditor((s) => s.groups.find((g) => g.id === s.activeGroupId)?.activeTabId)
  const [symbols, setSymbols] = useState<FlatSymbol[]>([])
  const [filter, setFilter] = useState('')
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set())
  const [cursorLine, setCursorLine] = useState(1)
  useEffect(() => {
    let alive = true; let timer: ReturnType<typeof setTimeout> | null = null
    const load = async () => { const ed = activeEditor(); const m = ed?.getModel(); if (!m) { setSymbols([]); return } const syms = await getDocumentSymbols(m); if (alive) setSymbols(flattenSymbols(syms)) }
    const schedule = () => { if (timer) clearTimeout(timer); timer = setTimeout(() => void load(), 400) }
    void load()
    const off = subscribe<{ path: string }>('editor:changed', (e) => { if (e.path === activeTab()?.path) schedule() })
    const ed = activeEditor(); const d = ed?.onDidChangeCursorPosition((e) => setCursorLine(e.position.lineNumber))
    return () => { alive = false; if (timer) clearTimeout(timer); off(); d?.dispose() }
  }, [activeId])
  const f = filter.toLowerCase()
  const visible = symbols.filter((s) => {
    if (f) return s.name.toLowerCase().includes(f)
    // hide children of collapsed ancestors
    const parts = s.id.split('.'); for (let i = 1; i < parts.length; i++) if (collapsed.has(parts.slice(0, i).join('.'))) return false
    return true
  })
  const go = (s: FlatSymbol) => { const ed = activeEditor(); if (!ed) return; ed.revealRangeInCenterIfOutsideViewport(s.selectionRange); ed.setSelection(s.selectionRange); ed.focus() }
  return (
    <div className="h-full flex flex-col text-xs">
      <div className="px-2 py-1.5 border-b border-border flex items-center gap-1"><ListTree size={12} className="text-muted" /><input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="Filter symbols" className="flex-1 h-6 px-2 rounded bg-input border border-border outline-none focus:border-accent" /></div>
      <div className="flex-1 overflow-auto py-1">
        {!visible.length && <div className="px-3 py-2 text-muted">{symbols.length ? 'No matches' : 'No symbols found in the active editor'}</div>}
        {visible.map((s) => {
          const inside = s.range.startLineNumber <= cursorLine && s.range.endLineNumber >= cursorLine
          const hasChildren = s.children.length > 0
          return (
            <div key={s.id} className={`flex items-center gap-1 h-[22px] pr-2 hover:bg-hover cursor-default ${inside ? 'bg-selected' : ''}`} style={{ paddingLeft: 6 + s.depth * 12 }} onClick={() => go(s)}>
              <button className="w-4 h-4 flex items-center justify-center text-muted" onClick={(e) => { e.stopPropagation(); setCollapsed((c) => { const n = new Set(c); n.has(s.id) ? n.delete(s.id) : n.add(s.id); return n }) }}>{hasChildren ? (collapsed.has(s.id) ? <ChevronRight size={11} /> : <ChevronDown size={11} />) : null}</button>
              <SymbolIcon kind={s.kind} />
              <span className="truncate">{s.name}</span>
              {s.detail && <span className="text-faint truncate">{s.detail}</span>}
            </div>
          )
        })}
      </div>
    </div>
  )
}

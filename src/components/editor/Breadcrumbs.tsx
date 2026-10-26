import { useEffect, useState } from 'react'
import { ChevronRight } from 'lucide-react'
import { monaco } from '@/lib/monaco/setup'
import { relPath } from '@/store/workspace'
import { editorForGroup, type EditorTab } from '@/store/editor'
import { runCommand } from '@/lib/commands'
import { getDocumentSymbols, flattenSymbols, type FlatSymbol } from './symbols'
import { SymbolIcon } from './SymbolIcon'

export function Breadcrumbs({ tab, groupId }: { tab: EditorTab; groupId: string }) {
  const [symbolPath, setSymbolPath] = useState<FlatSymbol[]>([])
  useEffect(() => {
    if (tab.kind !== 'file' || !tab.path) { setSymbolPath([]); return }
    const editor = editorForGroup(groupId); if (!editor) return
    let timer: ReturnType<typeof setTimeout> | null = null
    let cache: { version: number; flat: FlatSymbol[] } | null = null
    const update = async () => {
      const model = editor.getModel(); const pos = editor.getPosition(); if (!model || !pos || model.uri.path !== tab.path) return
      if (!cache || cache.version !== model.getVersionId()) { const syms = await getDocumentSymbols(model); cache = { version: model.getVersionId(), flat: flattenSymbols(syms) } }
      const line = pos.lineNumber
      const chain = cache.flat.filter((s) => s.range.startLineNumber <= line && s.range.endLineNumber >= line).sort((a, b) => a.depth - b.depth)
      setSymbolPath(chain.slice(0, 4))
    }
    const schedule = () => { if (timer) clearTimeout(timer); timer = setTimeout(() => void update(), 250) }
    const d = [editor.onDidChangeCursorPosition(schedule), editor.onDidChangeModelContent(schedule)]
    schedule()
    return () => { if (timer) clearTimeout(timer); d.forEach((x) => x.dispose()) }
  }, [tab.id, tab.path, tab.kind, groupId])
  if (!tab.path) return null
  const parts = relPath(tab.path).split('/')
  return (
    <div className="h-[22px] shrink-0 flex items-center px-2 text-[11px] text-muted border-b border-border bg-editor overflow-hidden whitespace-nowrap">
      {parts.map((p, i) => (
        <span key={i} className="flex items-center">
          {i > 0 && <ChevronRight size={11} className="text-faint mx-0.5" />}
          <button className="hover:text-fg truncate max-w-[180px]" onClick={() => { if (i < parts.length - 1) void runCommand('view.explorer', { reveal: tab.path }) }}>{p}</button>
        </span>
      ))}
      {symbolPath.map((s) => (
        <span key={s.id} className="flex items-center">
          <ChevronRight size={11} className="text-faint mx-0.5" />
          <button className="hover:text-fg flex items-center gap-1" onClick={() => { const ed = editorForGroup(groupId); ed?.revealRangeInCenter(s.range); ed?.setPosition({ lineNumber: s.range.startLineNumber, column: s.range.startColumn }); ed?.focus() }}><SymbolIcon kind={s.kind} />{s.name}</button>
        </span>
      ))}
    </div>
  )
}

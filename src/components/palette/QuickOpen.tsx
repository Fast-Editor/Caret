import { Command } from 'cmdk'
import { File, Clock } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import { api } from '@/lib/api'
import { runCommand, hasCommand } from '@/lib/commands'
import { Kbd } from '@/components/common'
import { useUi } from '@/store/ui'
import { relPath } from '@/store/workspace'
import { PaletteShell, PaletteItem, Highlight } from './PaletteShell'
import { parseQuickOpenInput } from './parse'
import type { QuickOpenResult } from '@/types'
import { fileIconFor } from '@/components/explorer/fileIcons'

type Row = { path: string; relPath: string; indices?: number[]; recent?: boolean; ignored?: boolean }

export function QuickOpen({ initial = '' }: { initial?: string }) {
  const close = useUi((s) => s.closePalette)
  const openPalette = useUi((s) => s.openPalette)
  const [value, setValue] = useState(initial)
  const [rows, setRows] = useState<Row[]>([])
  const [loading, setLoading] = useState(false)
  const seq = useRef(0)
  const parsed = parseQuickOpenInput(value)

  useEffect(() => {
    if (parsed.mode === 'commands') { openPalette('commands', value); return }
    if (parsed.mode === 'symbols') { openPalette('symbols', value); return }
    if (parsed.mode === 'search') { close(); void runCommand('search.findInFiles', { query: parsed.query }); return }
    if (parsed.mode === 'line') { close(); void runCommand('editor.goToLine', { line: parsed.line }); return }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [parsed.mode])

  useEffect(() => {
    const my = ++seq.current
    if (parsed.mode === 'recent') {
      setLoading(false)
      void (async () => {
        if (!hasCommand('editor.getRecentFiles')) { setRows([]); return }
        const recent = (await runCommand<string[]>('editor.getRecentFiles')) ?? []
        if (my === seq.current) setRows(recent.slice(0, 30).map((p) => ({ path: p, relPath: relPath(p), recent: true })))
      })()
      return
    }
    if (parsed.mode !== 'files') return
    setLoading(true)
    const t = setTimeout(async () => {
      try {
        const res: QuickOpenResult[] = await api.fs.quickOpen(parsed.query, 60)
        if (my === seq.current) setRows(res.map((r) => ({ path: r.path, relPath: r.relPath, indices: r.indices, ignored: r.ignored })))
      } catch { if (my === seq.current) setRows([]) } finally { if (my === seq.current) setLoading(false) }
    }, 60)
    return () => clearTimeout(t)
  }, [parsed.query, parsed.mode])

  const open = (row: Row, side: boolean) => {
    close()
    void runCommand('editor.openFile', { path: row.path, line: parsed.line, column: parsed.column, group: side ? 1 : undefined, focus: true })
  }
  return (
    <PaletteShell label="Go to file" value={value} onValueChange={setValue} shouldFilter={false} onClose={close}
      placeholder="Search files by name (append :line:col to jump)"
      onKeyDown={(e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { const el = document.querySelector<HTMLElement>('[cmdk-item][data-selected=true]'); const p = el?.dataset.path; const r = rows.find((x) => x.path === p); if (r) { e.preventDefault(); open(r, true) } } }}
      footer={<><span><Kbd keys="enter" /> open</span><span><Kbd keys="mod+enter" /> open to the side</span><span className="ml-auto">{loading ? 'searching…' : `${rows.length} results`}</span></>}>
      {rows.length === 0 && !loading && <Command.Empty className="px-3 py-6 text-center text-xs text-muted">{parsed.mode === 'recent' ? 'No recently opened files' : 'No matching files'}</Command.Empty>}
      {rows.map((r) => {
        const name = r.relPath.split('/').pop() ?? r.relPath
        const dir = r.relPath.slice(0, Math.max(0, r.relPath.length - name.length - 1))
        const off = r.relPath.length - name.length
        const nameIdx = r.indices?.filter((i) => i >= off).map((i) => i - off)
        const dirIdx = r.indices?.filter((i) => i < off)
        return (
          <PaletteItem key={r.path} value={r.path} onSelect={() => open(r, false)}>
            <span className="text-muted [&_svg]:size-4" data-path={r.path}>{r.recent ? <Clock /> : fileIconFor(name, false) ?? <File />}</span>
            <Highlight text={name} indices={nameIdx} className={r.ignored ? 'opacity-60' : undefined} />
            <Highlight text={dir} indices={dirIdx} className="text-muted truncate text-[11px]" />
            {r.ignored && <span className="ml-auto shrink-0 text-faint text-[10px]" title="Matched by an ignore file; not indexed for search or AI context">ignored</span>}
          </PaletteItem>
        )
      })}
    </PaletteShell>
  )
}

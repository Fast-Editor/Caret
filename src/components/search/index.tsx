import { CaseSensitive, ChevronDown, ChevronRight, CopyMinus, CopyPlus, Ellipsis, Regex, Replace, ReplaceAll, WholeWord, X, Loader2, RefreshCw, Ban } from 'lucide-react'
import { useEffect, useMemo, useRef } from 'react'
import { IconButton, Tooltip, cn } from '@/components/common'
import { confirm } from '@/components/dialogs'
import { registerCommands, runCommand, hasCommand } from '@/lib/commands'
import { useSearch } from '@/store/search'
import { useUi } from '@/store/ui'
import { relPath, baseName } from '@/store/workspace'
import { notify } from '@/store/notifications'
import { fileIconFor } from '@/components/explorer/fileIcons'
import { previewParts } from './group'
import type { SearchMatch } from '@/types'

function ToggleBtn({ on, onClick, label, children }: { on: boolean; onClick: () => void; label: string; children: React.ReactNode }) {
  return <Tooltip label={label}><button aria-pressed={on} aria-label={label} onClick={onClick} className={cn('size-5 rounded flex items-center justify-center text-muted hover:text-fg [&_svg]:size-3.5', on && 'bg-selected text-accent')}>{children}</button></Tooltip>
}

export function SearchView() {
  const s = useSearch()
  const inputRef = useRef<HTMLInputElement>(null)
  useEffect(() => { inputRef.current?.focus(); inputRef.current?.select() }, [])
  const runDebounced = useRef<number | null>(null)
  const scheduleRun = () => { if (runDebounced.current) clearTimeout(runDebounced.current); runDebounced.current = window.setTimeout(() => void useSearch.getState().run(), 250) }

  const flat = useMemo(() => {
    const out: { key: string; file?: string; match?: SearchMatch; idx?: number }[] = []
    for (const p of s.order) { out.push({ key: `f:${p}`, file: p }); if (!s.collapsed[p]) (s.results[p] ?? []).forEach((m, i) => out.push({ key: `m:${p}:${i}`, match: m, idx: i })) }
    return out
  }, [s.order, s.results, s.collapsed])

  const onKeyNav = (e: React.KeyboardEvent) => {
    if (!flat.length) return
    const cur = flat.findIndex((r) => r.key === s.selectedKey)
    if (e.key === 'ArrowDown') { e.preventDefault(); s.set({ selectedKey: flat[Math.min(flat.length - 1, cur + 1)].key }) }
    else if (e.key === 'ArrowUp') { e.preventDefault(); s.set({ selectedKey: flat[Math.max(0, cur - 1)].key }) }
    else if (e.key === 'Enter') { const r = flat[cur]; if (r?.match) void runCommand('editor.openFile', { path: r.match.path, line: r.match.line, column: r.match.column, focus: true }); else if (r?.file) s.toggleCollapsed(r.file) }
  }
  const replaceAll = async () => {
    const ok = await confirm({ title: `Replace ${s.total} occurrence${s.total === 1 ? '' : 's'} in ${s.fileCount} file${s.fileCount === 1 ? '' : 's'}?`, message: <span>Replace <code className="mono">{s.query}</code> with <code className="mono">{s.replacement || '(empty)'}</code>. This cannot be undone from here.</span>, okLabel: 'Replace All', danger: true })
    if (!ok) return
    try { const r = await s.replaceAll(); notify('success', `Replaced ${r.replacements} occurrence${r.replacements === 1 ? '' : 's'} in ${r.files} file${r.files === 1 ? '' : 's'}`) } catch (e) { notify('error', 'Replace failed', String((e as Error).message ?? e)) }
  }
  const replaceInFile = async (path: string) => {
    try { const r = await s.replaceAll([path]); notify('success', `Replaced ${r.replacements} in ${baseName(path)}`) } catch (e) { notify('error', 'Replace failed', String((e as Error).message ?? e)) }
  }

  return (
    <div className="flex flex-col h-full" data-keyctx="search">
      <div className="flex items-center h-[26px] px-3 text-[11px] font-semibold text-muted shrink-0">Search
        <div className="ml-auto flex items-center">
          <IconButton size="xs" icon={<RefreshCw />} label="Refresh" onClick={() => void s.run()} />
          <IconButton size="xs" icon={<X />} label="Clear Search Results" onClick={() => s.clear()} />
          <IconButton size="xs" icon={<CopyMinus />} label="Collapse All" onClick={() => s.collapseAll(true)} />
          <IconButton size="xs" icon={<CopyPlus />} label="Expand All" onClick={() => s.collapseAll(false)} />
        </div>
      </div>
      <div className="px-2 flex gap-1 shrink-0">
        <button aria-label="Toggle Replace" onClick={() => s.set({ showReplace: !s.showReplace })} className="w-4 flex items-center justify-center text-muted hover:text-fg">{s.showReplace ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}</button>
        <div className="flex-1 min-w-0 flex flex-col gap-1">
          <div className="flex items-center h-7 rounded-md border border-border bg-input px-1.5 focus-within:border-accent">
            <input ref={inputRef} value={s.query} placeholder="Search" spellCheck={false} className="flex-1 min-w-0 bg-transparent text-xs outline-none placeholder:text-faint" list="caret-search-history"
              onChange={(e) => { s.set({ query: e.target.value }); scheduleRun() }} onKeyDown={(e) => { if (e.key === 'Enter') void s.run(); if (e.key === 'ArrowDown' && flat.length) { e.preventDefault(); s.set({ selectedKey: flat[0].key }); (e.currentTarget.closest('[data-keyctx=search]')?.querySelector('[data-results]') as HTMLElement | null)?.focus() } }} />
            <datalist id="caret-search-history">{s.history.map((h) => <option key={h} value={h} />)}</datalist>
            <ToggleBtn on={s.caseSensitive} label="Match Case" onClick={() => { s.set({ caseSensitive: !s.caseSensitive }); scheduleRun() }}><CaseSensitive /></ToggleBtn>
            <ToggleBtn on={s.wholeWord} label="Match Whole Word" onClick={() => { s.set({ wholeWord: !s.wholeWord }); scheduleRun() }}><WholeWord /></ToggleBtn>
            <ToggleBtn on={s.isRegex} label="Use Regular Expression" onClick={() => { s.set({ isRegex: !s.isRegex }); scheduleRun() }}><Regex /></ToggleBtn>
          </div>
          {s.showReplace && (
            <div className="flex items-center gap-1">
              <div className="flex-1 flex items-center h-7 rounded-md border border-border bg-input px-1.5 focus-within:border-accent">
                <input value={s.replacement} placeholder="Replace" spellCheck={false} className="flex-1 min-w-0 bg-transparent text-xs outline-none placeholder:text-faint" onChange={(e) => s.set({ replacement: e.target.value })} onKeyDown={(e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) void replaceAll() }} />
              </div>
              <IconButton size="sm" icon={<ReplaceAll />} label="Replace All" disabled={!s.total || s.running} onClick={() => void replaceAll()} />
            </div>
          )}
        </div>
      </div>
      <div className="px-2 pt-1 shrink-0">
        <button onClick={() => s.set({ showDetails: !s.showDetails })} className="flex items-center gap-1 text-[11px] text-muted hover:text-fg"><Ellipsis className="size-3.5" />{s.showDetails ? 'Hide' : 'Show'} search details</button>
        {s.showDetails && (
          <div className="mt-1 flex flex-col gap-1 text-[11px]">
            <label className="text-muted">files to include<input value={s.includes} placeholder="e.g. src/**/*.ts, *.md" spellCheck={false} onChange={(e) => { s.set({ includes: e.target.value }); scheduleRun() }} className="mt-0.5 w-full h-6 rounded-md border border-border bg-input px-1.5 text-xs outline-none focus:border-accent" /></label>
            <label className="text-muted">files to exclude<input value={s.excludes} placeholder="e.g. **/dist/**" spellCheck={false} onChange={(e) => { s.set({ excludes: e.target.value }); scheduleRun() }} className="mt-0.5 w-full h-6 rounded-md border border-border bg-input px-1.5 text-xs outline-none focus:border-accent" /></label>
            <div className="flex items-center gap-3 text-muted">
              <label className="flex items-center gap-1"><input type="checkbox" checked={s.useIgnoreFiles} onChange={(e) => { s.set({ useIgnoreFiles: e.target.checked }); scheduleRun() }} />Use .gitignore</label>
              <label className="flex items-center gap-1">Context<input type="number" min={0} max={5} value={s.contextLines} onChange={(e) => s.set({ contextLines: Number(e.target.value) })} className="w-10 h-5 rounded border border-border bg-input px-1 text-xs" /></label>
            </div>
          </div>
        )}
      </div>
      <div className="px-3 h-6 flex items-center gap-2 text-[11px] text-muted shrink-0">
        {s.running ? <><Loader2 className="size-3 spin" /> Searching…<button onClick={() => void s.cancel()} className="ml-1 underline hover:text-fg flex items-center gap-1"><Ban className="size-3" />cancel</button></>
          : s.error ? <span className="text-danger truncate" title={s.error}>{s.error}</span>
          : s.query ? <span>{s.total} result{s.total === 1 ? '' : 's'} in {s.fileCount} file{s.fileCount === 1 ? '' : 's'}{s.limited ? ' (limited)' : ''}{s.elapsedMs ? ` · ${s.elapsedMs} ms` : ''}</span> : null}
      </div>
      <div data-results tabIndex={0} onKeyDown={onKeyNav} className="flex-1 min-h-0 overflow-auto outline-none pb-6">
        {s.order.map((p) => (
          <div key={p} className="animate-fade-up">
            <div className={cn('group flex items-center gap-1 h-[22px] px-1 text-xs hover:bg-hover cursor-pointer transition-base', s.selectedKey === `f:${p}` && 'bg-selected')} onClick={() => { s.set({ selectedKey: `f:${p}` }); s.toggleCollapsed(p) }}>
              <span className="text-muted"><ChevronRight className="size-3.5 caret-chevron" data-open={!s.collapsed[p]} /></span>
              <span className="flex [&_svg]:size-4">{fileIconFor(baseName(p), false)}</span>
              <span className="truncate">{baseName(p)}</span>
              <span className="truncate text-faint text-[11px]">{relPath(p).split('/').slice(0, -1).join('/')}</span>
              <span className="ml-auto flex items-center gap-1">
                {s.showReplace && <IconButton size="xs" icon={<Replace />} label="Replace in file" className="opacity-0 group-hover:opacity-100" onClick={(e) => { e.stopPropagation(); void replaceInFile(p) }} />}
                <IconButton size="xs" icon={<X />} label="Dismiss" className="opacity-0 group-hover:opacity-100" onClick={(e) => { e.stopPropagation(); s.dismissFile(p) }} />
                <span className="rounded-full bg-border-strong px-1.5 text-[10px] text-muted">{s.results[p]?.length ?? 0}</span>
              </span>
            </div>
            {!s.collapsed[p] && (s.results[p] ?? []).map((m, i) => {
              const parts = previewParts(m)
              const key = `m:${p}:${i}`
              return (
                <div key={key} className={cn('group flex items-center h-[22px] pl-9 pr-1 text-xs hover:bg-hover cursor-pointer transition-base', s.selectedKey === key && 'bg-selected')} title={`${relPath(p)}:${m.line}:${m.column}`}
                  onClick={() => { s.set({ selectedKey: key }); void runCommand('editor.openFile', { path: m.path, line: m.line, column: m.column, preview: true }) }}
                  onDoubleClick={() => void runCommand('editor.openFile', { path: m.path, line: m.line, column: m.column, focus: true })}>
                  <span className="mono truncate flex-1 whitespace-pre text-muted">{parts.truncatedStart ? '…' : ''}{parts.before}<span className={cn('rounded-[2px] bg-warning/25 text-fg', s.showReplace && s.replacement !== undefined && 'line-through decoration-danger')}>{parts.match}</span>{s.showReplace && <span className="rounded-[2px] bg-success/25 text-fg">{s.replacement}</span>}{parts.after}</span>
                  <IconButton size="xs" icon={<X />} label="Dismiss" className="opacity-0 group-hover:opacity-100 ml-1" onClick={(e) => { e.stopPropagation(); s.dismissMatch(p, i) }} />
                </div>
              )
            })}
          </div>
        ))}
        {!s.running && s.query && s.order.length === 0 && !s.error && <div className="px-3 py-4 text-xs text-muted">No results found. Review your settings for configured exclusions and check your gitignore files.</div>}
      </div>
    </div>
  )
}

export function registerSearchCommands(): () => void {
  return registerCommands([
    { id: 'search.findInFiles', title: 'Find in Files', category: 'Search', keybinding: 'mod+shift+f', run: async (args) => {
      const a = (args ?? {}) as { query?: string; include?: string }
      useUi.getState().showSidebar('search')
      let q = a.query
      if (q === undefined && hasCommand('editor.getSelection')) { try { const sel = await runCommand<{ text?: string } | undefined>('editor.getSelection'); if (sel?.text && !sel.text.includes('\n')) q = sel.text } catch { /* ignore */ } }
      const patch: Partial<ReturnType<typeof useSearch.getState>> = {}
      if (q !== undefined) patch.query = q
      if (a.include !== undefined) { patch.includes = a.include; patch.showDetails = true }
      useSearch.setState(patch)
      setTimeout(() => { const el = document.querySelector<HTMLInputElement>('[data-keyctx=search] input'); el?.focus(); el?.select(); if (q) void useSearch.getState().run() }, 30)
    } },
    { id: 'search.replaceInFiles', title: 'Replace in Files', category: 'Search', keybinding: 'mod+shift+h', run: () => { useUi.getState().showSidebar('search'); useSearch.setState({ showReplace: true }); setTimeout(() => document.querySelector<HTMLInputElement>('[data-keyctx=search] input')?.focus(), 30) } },
  ])
}

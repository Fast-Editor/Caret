import { ChevronDown, ChevronRight, Loader2 } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import { subscribe } from '@/lib/bus'
import { Events, on } from '@/lib/tauri'
import { useExplorer } from '@/store/explorer'
import { useGit } from '@/store/git'
import { useSettings } from '@/store/settings'
import { useWorkspace, relPath } from '@/store/workspace'
import { ContextMenu, VirtualList, cn, type MenuEntry, type VirtualListHandle } from '@/components/common'
import { confirm } from '@/components/dialogs'
import { notify } from '@/store/notifications'
import type { DirEntry, FsChangeEvent, GitFileChange, GitStatus } from '@/types'
import { fileIconFor } from './fileIcons'
import { flattenTree, type TreeRow } from './compact'

const ROW_H = 22
type GitDeco = { letter: string; cls: string }
const GIT_DECO: Record<string, GitDeco> = {
  modified: { letter: 'M', cls: 'text-warning' }, added: { letter: 'A', cls: 'text-success' }, untracked: { letter: 'U', cls: 'text-success' }, deleted: { letter: 'D', cls: 'text-danger' },
  renamed: { letter: 'R', cls: 'text-warning' }, copied: { letter: 'C', cls: 'text-success' }, conflicted: { letter: '!', cls: 'text-danger' }, typechange: { letter: 'T', cls: 'text-warning' }, ignored: { letter: '', cls: 'text-faint' },
}

function gitDecorations(status: GitStatus | null, root: string): { files: Map<string, GitDeco>; dirs: Set<string> } {
  const files = new Map<string, GitDeco>(); const dirs = new Set<string>()
  if (!status?.isRepo) return { files, dirs }
  const add = (c: GitFileChange) => {
    const abs = c.path.startsWith('/') ? c.path : `${root}/${c.path}`
    const deco = GIT_DECO[c.status]
    if (deco && !files.has(abs)) files.set(abs, deco)
    if (c.status !== 'ignored') { let p = abs; for (;;) { const i = p.lastIndexOf('/'); if (i <= root.length) break; p = p.slice(0, i); dirs.add(p) } }
  }
  status.conflicted.forEach(add); status.staged.forEach(add); status.unstaged.forEach(add); status.untracked.forEach(add)
  return { files, dirs }
}

function uniqueName(existing: Set<string>, name: string): string {
  if (!existing.has(name)) return name
  const dot = name.lastIndexOf('.')
  const base = dot > 0 ? name.slice(0, dot) : name; const ext = dot > 0 ? name.slice(dot) : ''
  for (let i = 1; i < 1000; i++) { const n = `${base} copy${i > 1 ? ` ${i}` : ''}${ext}`; if (!existing.has(n)) return n }
  return `${base}-${Date.now()}${ext}`
}

export function FileTree() {
  const root = useWorkspace((s) => s.workspace?.root)
  const ex = useExplorer()
  const settings = useSettings((s) => s.settings)
  const gitStatus = useGit((s) => s.status)
  const listRef = useRef<VirtualListHandle>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const [dropTarget, setDropTarget] = useState<string | null>(null)

  const rows = useMemo<TreeRow[]>(() => root ? flattenTree({ root, children: ex.children, expanded: ex.expanded, loading: ex.loading, errors: ex.errors, showHidden: ex.showHidden, filter: ex.filter, compact: settings?.ui.compactFolders ?? true, globalIgnore: settings?.general.globalIgnore }) : [], [root, ex.children, ex.expanded, ex.loading, ex.errors, ex.showHidden, ex.filter, settings?.ui.compactFolders, settings?.general.globalIgnore])
  const deco = useMemo(() => gitDecorations(gitStatus, root ?? ''), [gitStatus, root])

  // Insert the inline "create" row right after its parent (or at top for root).
  const displayRows = useMemo(() => {
    if (!ex.editing || ex.editing.kind === 'rename' || !root) return rows
    const parent = ex.editing.parent
    const fake: TreeRow = { entry: { name: '', path: `${parent}/__new__`, kind: ex.editing.kind === 'create-dir' ? 'dir' : 'file', isHidden: false, isIgnored: false }, depth: 0, expanded: false, loading: false, label: '', nodePath: `${parent}/__new__`, chain: [] }
    if (parent === root) return [fake, ...rows]
    const idx = rows.findIndex((r) => r.nodePath === parent || r.chain.includes(parent))
    if (idx < 0) return [fake, ...rows]
    fake.depth = rows[idx].depth + 1
    return [...rows.slice(0, idx + 1), fake, ...rows.slice(idx + 1)]
  }, [rows, ex.editing, root])

  // Load root + react to fs changes
  useEffect(() => { if (root) { ex.reset(); void ex.loadDir(root) } // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [root])
  useEffect(() => on<FsChangeEvent>(Events.fsChanged, (e) => useExplorer.getState().refreshPaths(e.paths)), [])
  useEffect(() => subscribe<{ path: string }>('editor.active', ({ path }) => {
    const st = useExplorer.getState()
    if (!st.followActive || !root || !path?.startsWith(root)) return
    void st.expandTo(path, root).then(() => { const i = useExplorerRowsIndex(path); if (i >= 0) listRef.current?.scrollToIndex(i) })
  }), [root])
  const useExplorerRowsIndex = (path: string) => rows.findIndex((r) => r.nodePath === path || r.entry.path === path)

  const rowPaths = useMemo(() => displayRows.map((r) => r.nodePath), [displayRows])
  const selectedDir = useCallback((): string => {
    const f = ex.focused
    if (!f || !root) return root ?? ''
    const row = rows.find((r) => r.nodePath === f)
    if (row?.entry.kind === 'dir') return row.nodePath
    return f.slice(0, f.lastIndexOf('/')) || root
  }, [ex.focused, rows, root])

  // ───── actions ─────
  const openRow = (r: TreeRow, preview = true) => {
    if (r.entry.kind === 'dir') { ex.toggle(r.nodePath); return }
    void runCommand('editor.openFile', { path: r.entry.path, preview, focus: !preview })
  }
  const startCreate = (kind: 'create-file' | 'create-dir', parent?: string) => {
    const p = parent ?? selectedDir()
    if (!p) return
    if (p !== root) ex.expand(p)
    ex.set({ editing: { kind, parent: p } })
  }
  const commitEdit = async (value: string) => {
    const e = ex.editing
    ex.set({ editing: null })
    if (!e || !value.trim()) return
    try {
      if (e.kind === 'rename') {
        const dir = e.path.slice(0, e.path.lastIndexOf('/'))
        const to = `${dir}/${value.trim()}`
        if (to !== e.path) { await api.fs.rename(e.path, to); ex.refreshPaths([e.path, to]); ex.select(to) }
      } else {
        const target = `${e.parent}/${value.trim().replace(/^\/+/, '')}`
        await api.fs.create(target, e.kind === 'create-dir' ? 'dir' : 'file')
        ex.refreshPaths([target])
        ex.select(target)
        if (e.kind === 'create-file') void runCommand('editor.openFile', { path: target, preview: false, focus: true })
        else ex.expand(target)
      }
    } catch (err) { notify('error', 'File operation failed', String((err as Error).message ?? err)) }
  }
  const selectedPaths = (): string[] => { const sel = Object.keys(ex.selected); return sel.length ? sel : ex.focused ? [ex.focused] : [] }
  const deleteSelected = async (paths = selectedPaths()) => {
    if (!paths.length) return
    const ok = (settings?.ui.confirmDelete ?? true) ? await confirm({ title: paths.length === 1 ? `Move "${paths[0].split('/').pop()}" to Trash?` : `Move ${paths.length} items to Trash?`, message: 'You can restore from the Trash.', okLabel: 'Move to Trash', danger: true }) : true
    if (!ok) return
    try { await api.fs.delete(paths, true); ex.refreshPaths(paths); ex.set({ selected: {}, focused: null }) } catch (err) { notify('error', 'Delete failed', String((err as Error).message ?? err)) }
  }
  const copyPathToClipboard = async (p: string, rel: boolean) => { try { const { writeText } = await import('@tauri-apps/plugin-clipboard-manager'); await writeText(rel ? relPath(p) : p) } catch { await navigator.clipboard.writeText(rel ? relPath(p) : p) } }
  const pasteInto = async (dir: string) => {
    const cb = ex.clipboard
    if (!cb) return
    const existing = new Set((ex.children[dir] ?? []).map((c) => c.name))
    try {
      for (const src of cb.paths) {
        const name = src.split('/').pop()!
        const dest = `${dir}/${cb.cut ? name : uniqueName(existing, name)}`
        if (dest === src || dest.startsWith(src + '/')) continue
        if (cb.cut) await api.fs.move(src, dest); else await api.fs.copy(src, dest)
        existing.add(dest.split('/').pop()!)
      }
      ex.refreshPaths([...cb.paths, dir + '/x'])
      if (cb.cut) ex.set({ clipboard: null })
    } catch (err) { notify('error', 'Paste failed', String((err as Error).message ?? err)) }
  }
  const duplicate = async (p: string) => {
    const dir = p.slice(0, p.lastIndexOf('/'))
    const existing = new Set((ex.children[dir] ?? []).map((c) => c.name))
    const dest = `${dir}/${uniqueName(existing, p.split('/').pop()!)}`
    try { await api.fs.copy(p, dest); ex.refreshPaths([dest]); ex.select(dest) } catch (err) { notify('error', 'Duplicate failed', String((err as Error).message ?? err)) }
  }
  const moveTo = async (paths: string[], dir: string) => {
    try {
      for (const src of paths) {
        if (src === dir || dir.startsWith(src + '/')) continue
        const dest = `${dir}/${src.split('/').pop()}`
        if (dest === src) continue
        if (await api.fs.exists(dest)) { const ok = await confirm({ title: `Replace "${dest.split('/').pop()}"?`, message: 'A file or folder with the same name already exists in the destination.', okLabel: 'Replace', danger: true }); if (!ok) continue }
        await api.fs.move(src, dest)
      }
      ex.refreshPaths([...paths, dir + '/x'])
    } catch (err) { notify('error', 'Move failed', String((err as Error).message ?? err)) }
  }

  // Expose explorer commands (registered once)
  useEffect(() => {
    const handlers = {
      rename: () => { const f = useExplorer.getState().focused; if (f && f !== root) useExplorer.setState({ editing: { kind: 'rename', path: f } }) },
      delete: () => void deleteSelected(),
      copy: () => useExplorer.setState({ clipboard: { paths: selectedPaths(), cut: false } }),
      cut: () => useExplorer.setState({ clipboard: { paths: selectedPaths(), cut: true } }),
      paste: () => void pasteInto(selectedDir()),
      newFile: (args?: unknown) => startCreate('create-file', (args as { parent?: string } | undefined)?.parent),
      newFolder: (args?: unknown) => startCreate('create-dir', (args as { parent?: string } | undefined)?.parent),
      reveal: (args?: unknown) => { const p = (args as { path?: string } | undefined)?.path; if (p && root) void useExplorer.getState().expandTo(p, root) },
    }
    explorerActions.current = handlers
  })

  // ───── keyboard ─────
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (ex.editing) return
    const idx = displayRows.findIndex((r) => r.nodePath === ex.focused)
    const focusIdx = (i: number) => { const r = displayRows[Math.max(0, Math.min(displayRows.length - 1, i))]; if (r) { ex.select(r.nodePath, { range: e.shiftKey, rows: rowPaths }); listRef.current?.scrollToIndex(Math.max(0, Math.min(displayRows.length - 1, i))) } }
    switch (e.key) {
      case 'ArrowDown': e.preventDefault(); focusIdx(idx + 1); break
      case 'ArrowUp': e.preventDefault(); focusIdx(idx - 1); break
      case 'ArrowRight': { e.preventDefault(); const r = displayRows[idx]; if (r?.entry.kind === 'dir') { if (r.expanded) focusIdx(idx + 1); else ex.expand(r.nodePath) } break }
      case 'ArrowLeft': { e.preventDefault(); const r = displayRows[idx]; if (r?.entry.kind === 'dir' && r.expanded) ex.collapse(r.nodePath); else if (r) { const parent = r.nodePath.slice(0, r.nodePath.lastIndexOf('/')); const pi = displayRows.findIndex((x) => x.nodePath === parent || x.chain.includes(parent)); if (pi >= 0) focusIdx(pi) } break }
      case 'Enter': case ' ': { e.preventDefault(); const r = displayRows[idx]; if (r) openRow(r, e.key === ' '); break }
      case 'Home': e.preventDefault(); focusIdx(0); break
      case 'End': e.preventDefault(); focusIdx(displayRows.length - 1); break
      case 'Escape': ex.set({ selected: {}, filter: ex.filterOpen ? ex.filter : '' }); break
      default: break
    }
  }

  const menuFor = (r: TreeRow | null): MenuEntry[] => {
    const isDir = r?.entry.kind === 'dir'
    const dirForNew = r ? (isDir ? r.nodePath : r.entry.path.slice(0, r.entry.path.lastIndexOf('/'))) : root!
    const sel = r && ex.selected[r.nodePath] ? selectedPaths() : r ? [r.entry.path] : []
    return [
      { label: 'New File…', onSelect: () => startCreate('create-file', dirForNew) },
      { label: 'New Folder…', onSelect: () => startCreate('create-dir', dirForNew) },
      { type: 'separator' },
      { label: 'Open to the Side', hidden: !r || isDir, onSelect: () => void runCommand('editor.openFile', { path: r!.entry.path, group: 1, focus: true }) },
      { label: 'Reveal in Finder', onSelect: () => void api.app.showInFolder(r ? r.entry.path : root!) },
      { label: 'Open in Integrated Terminal', onSelect: () => void runCommand('terminal.new', { cwd: dirForNew }) },
      { type: 'separator' },
      { label: sel.length > 1 ? `Add ${sel.length} Files to Chat` : 'Add to Chat', hidden: !r, onSelect: () => { for (const p of sel) void runCommand('chat.addFile', { path: p }) } },
      { label: 'Find in Folder…', hidden: !isDir, onSelect: () => void runCommand('search.findInFiles', { include: `${relPath(r!.nodePath)}/**` }) },
      { type: 'separator' },
      { label: 'Cut', shortcut: 'mod+x', hidden: !r, onSelect: () => ex.set({ clipboard: { paths: sel, cut: true } }) },
      { label: 'Copy', shortcut: 'mod+c', hidden: !r, onSelect: () => ex.set({ clipboard: { paths: sel, cut: false } }) },
      { label: 'Paste', shortcut: 'mod+v', disabled: !ex.clipboard, onSelect: () => void pasteInto(dirForNew) },
      { label: 'Duplicate', hidden: !r, onSelect: () => void duplicate(r!.entry.path) },
      { type: 'separator' },
      { label: 'Copy Path', hidden: !r, onSelect: () => void copyPathToClipboard(r!.entry.path, false) },
      { label: 'Copy Relative Path', hidden: !r, onSelect: () => void copyPathToClipboard(r!.entry.path, true) },
      { type: 'separator' },
      { label: 'Rename…', shortcut: 'f2', hidden: !r, onSelect: () => ex.set({ editing: { kind: 'rename', path: r!.entry.path } }) },
      { label: 'Delete', shortcut: 'mod+backspace', hidden: !r, danger: true, onSelect: () => void deleteSelected(sel) },
    ]
  }

  if (!root) return null
  return (
    <div ref={containerRef} data-keyctx="explorer" tabIndex={0} onKeyDown={onKeyDown} className="flex-1 min-h-0 outline-none"
      onDragOver={(e) => { if (e.dataTransfer.types.includes('application/x-caret-path')) { e.preventDefault(); setDropTarget(root) } }} onDragLeave={() => setDropTarget(null)}
      onDrop={(e) => { e.preventDefault(); setDropTarget(null); const paths = (e.dataTransfer.getData('application/x-caret-paths') || e.dataTransfer.getData('application/x-caret-path')).split('\n').filter(Boolean); if (paths.length) void moveTo(paths, root) }}>
      <ContextMenu items={() => menuFor(null)}>
        <div className={cn('h-full', dropTarget === root && 'bg-selected/40')}>
          <VirtualList ref={listRef} count={displayRows.length} rowHeight={ROW_H} className="pb-6"
            emptyState={<div className="px-3 py-3 text-[11px] text-faint">{ex.loading[root] ? 'Loading…' : ex.filter ? 'No matching files' : 'Empty folder'}</div>}
            renderRow={(i, style) => <Row key={displayRows[i].nodePath} r={displayRows[i]} style={style} deco={deco} dropTarget={dropTarget} setDropTarget={setDropTarget} rowPaths={rowPaths} onOpen={openRow} onCommitEdit={commitEdit} onMove={moveTo} menuFor={menuFor} />} />
        </div>
      </ContextMenu>
    </div>
  )
}

/** Imperative actions bridged to commands (filled by the mounted tree). */
export const explorerActions: { current: Partial<Record<'rename' | 'delete' | 'copy' | 'cut' | 'paste' | 'newFile' | 'newFolder' | 'reveal', (args?: unknown) => void>> } = { current: {} }

function Row({ r, style, deco, dropTarget, setDropTarget, rowPaths, onOpen, onCommitEdit, onMove, menuFor }: {
  r: TreeRow; style: React.CSSProperties; deco: { files: Map<string, GitDeco>; dirs: Set<string> }; dropTarget: string | null; setDropTarget: (p: string | null) => void; rowPaths: string[]
  onOpen: (r: TreeRow, preview?: boolean) => void; onCommitEdit: (v: string) => void; onMove: (paths: string[], dir: string) => void; menuFor: (r: TreeRow) => MenuEntry[]
}) {
  const ex = useExplorer()
  const isDir = r.entry.kind === 'dir'
  const editing = ex.editing && ((ex.editing.kind === 'rename' && ex.editing.path === r.entry.path) || (ex.editing.kind !== 'rename' && r.nodePath.endsWith('/__new__')))
  const selected = !!ex.selected[r.nodePath]
  const focused = ex.focused === r.nodePath
  const fileDeco = deco.files.get(r.entry.path)
  const dirDeco = isDir && (r.chain.some((p) => deco.dirs.has(p)))
  const dimmed = r.entry.isIgnored || fileDeco?.letter === ''
  return (
    <ContextMenu items={() => menuFor(r)} onOpenChange={(o) => { if (o && !selected) ex.select(r.nodePath) }}>
      <div style={{ ...style, paddingLeft: 8 + r.depth * 12 }} draggable={!editing} title={r.entry.path}
        className={cn('flex items-center gap-1 pr-2 text-xs cursor-pointer select-none group/row', selected ? 'bg-selected' : 'hover:bg-hover', focused && 'outline-1 -outline-offset-1 outline-accent/60', dropTarget === r.nodePath && isDir && 'bg-selected/60', dimmed && 'opacity-50')}
        onClick={(e) => { ex.select(r.nodePath, { toggle: e.metaKey || e.ctrlKey, range: e.shiftKey, rows: rowPaths }); if (!e.metaKey && !e.ctrlKey && !e.shiftKey) onOpen(r, true) }}
        onDoubleClick={() => { if (!isDir) onOpen(r, false) }}
        onDragStart={(e) => { const paths = selected ? Object.keys(ex.selected) : [r.entry.path]; e.dataTransfer.setData('application/x-caret-path', r.entry.path); e.dataTransfer.setData('application/x-caret-paths', paths.join('\n')); e.dataTransfer.setData('text/plain', r.entry.path); e.dataTransfer.effectAllowed = 'copyMove' }}
        onDragOver={(e) => { if (isDir && e.dataTransfer.types.includes('application/x-caret-path')) { e.preventDefault(); e.stopPropagation(); setDropTarget(r.nodePath) } }}
        onDrop={(e) => { if (!isDir) return; e.preventDefault(); e.stopPropagation(); setDropTarget(null); const paths = (e.dataTransfer.getData('application/x-caret-paths') || e.dataTransfer.getData('application/x-caret-path')).split('\n').filter(Boolean); if (paths.length) onMove(paths, r.nodePath) }}>
        <span className="size-4 shrink-0 flex items-center justify-center text-muted">{isDir ? (r.loading ? <Loader2 className="size-3.5 spin" /> : r.expanded ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />) : null}</span>
        <span className="shrink-0 flex [&_svg]:size-4">{fileIconFor(r.entry.name || (isDir ? 'dir' : 'file'), isDir, r.expanded)}</span>
        {editing ? <InlineEdit initial={ex.editing!.kind === 'rename' ? r.entry.name : ''} isDir={isDir} onCommit={onCommitEdit} onCancel={() => ex.set({ editing: null })} />
          : <span className={cn('truncate flex-1', fileDeco?.cls)}>{r.label}</span>}
        {!editing && fileDeco?.letter && <span className={cn('text-[10px] font-semibold shrink-0', fileDeco.cls)}>{fileDeco.letter}</span>}
        {!editing && dirDeco && !fileDeco && <span className="size-1.5 rounded-full bg-warning/80 shrink-0" />}
        {r.error && <span className="text-[10px] text-danger shrink-0" title={r.error}>!</span>}
      </div>
    </ContextMenu>
  )
}

function InlineEdit({ initial, isDir, onCommit, onCancel }: { initial: string; isDir: boolean; onCommit: (v: string) => void; onCancel: () => void }) {
  const ref = useRef<HTMLInputElement>(null)
  const [v, setV] = useState(initial)
  useEffect(() => { const el = ref.current; if (!el) return; el.focus(); const dot = initial.lastIndexOf('.'); el.setSelectionRange(0, !isDir && dot > 0 ? dot : initial.length) }, [initial, isDir])
  const invalid = v.includes('/') ? 'Name cannot contain "/"' : v.trim() === '' ? null : v === '.' || v === '..' ? 'Invalid name' : null
  return (
    <span className="flex-1 min-w-0 relative">
      <input ref={ref} value={v} onChange={(e) => setV(e.target.value)} spellCheck={false}
        className={cn('w-full h-[18px] px-1 rounded bg-input border text-xs outline-none', invalid ? 'border-danger' : 'border-accent')}
        onBlur={() => (invalid ? onCancel() : onCommit(v))}
        onKeyDown={(e) => { e.stopPropagation(); if (e.key === 'Enter') { e.preventDefault(); if (!invalid) onCommit(v) } else if (e.key === 'Escape') { e.preventDefault(); onCancel() } }} />
      {invalid && <span className="absolute left-0 top-full z-10 mt-0.5 rounded bg-danger px-1.5 py-0.5 text-[10px] text-white whitespace-nowrap">{invalid}</span>}
    </span>
  )
}

export type { DirEntry }

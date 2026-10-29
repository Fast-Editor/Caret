import { ChevronRight, CopyMinus, Eye, EyeOff, FilePlus2, FolderOpen, FolderPlus, Locate, RefreshCw, X } from 'lucide-react'
import { useEffect, useRef } from 'react'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { IconButton, Input, Button, cn } from '@/components/common'
import { registerCommands, runCommand } from '@/lib/commands'
import { useExplorer } from '@/store/explorer'
import { useWorkspace } from '@/store/workspace'
import { FileTree, explorerActions } from './FileTree'
import { OpenEditors } from './OpenEditors'
import { ExplorerBottomSections } from './BottomSections'
export { fileIconFor } from './fileIcons'

export function ExplorerView() {
  const ws = useWorkspace((s) => s.workspace)
  const recent = useWorkspace((s) => s.recent)
  const ex = useExplorer()
  const filterRef = useRef<HTMLInputElement>(null)
  useEffect(() => { if (ex.filterOpen) filterRef.current?.focus() }, [ex.filterOpen])

  if (!ws) {
    return (
      <div className="p-3 text-xs flex flex-col gap-3">
        <div className="text-muted">No folder opened.</div>
        <Button variant="primary" icon={<FolderOpen className="size-3.5" />} onClick={() => void runCommand('workspace.openFolder')}>Open Folder</Button>
        {recent.length > 0 && (
          <div>
            <div className="text-[11px] font-semibold text-faint mb-1">Recent</div>
            {recent.slice(0, 8).map((r) => (
              <button key={r.path} disabled={!r.exists} title={r.path} onClick={() => void runCommand('workspace.openFolder', { path: r.path })} className="block w-full text-left truncate rounded px-1.5 h-6 hover:bg-hover disabled:opacity-40">
                <span className="text-fg">{r.name}</span> <span className="text-faint">{r.path.replace(/^\/Users\/[^/]+/, '~')}</span>
              </button>
            ))}
          </div>
        )}
      </div>
    )
  }
  return (
    <div className="flex flex-col h-full">
      <OpenEditors />
      <div className="flex items-center gap-0.5 h-[26px] px-1 shrink-0 group/head">
        <button onClick={() => ex.set({ treeCollapsed: !ex.treeCollapsed })} className="flex items-center gap-1 text-[11px] font-semibold text-muted hover:text-fg truncate flex-1 min-w-0">
          <ChevronRight className="size-3.5 shrink-0 caret-chevron" data-open={!ex.treeCollapsed} /><span className="truncate" title={ws.root}>{ws.name}</span>
        </button>
        {/* Hover-only actions: `hidden` (not opacity-0) so they reserve no width and the workspace name can use it. */}
        <div className="hidden group-hover/head:flex items-center shrink-0">
          <IconButton size="xs" icon={<FilePlus2 />} label="New File" onClick={() => explorerActions.current.newFile?.()} />
          <IconButton size="xs" icon={<FolderPlus />} label="New Folder" onClick={() => explorerActions.current.newFolder?.()} />
          <IconButton size="xs" icon={<RefreshCw />} label="Refresh" onClick={() => void ex.loadDir(ws.root, true)} />
          <IconButton size="xs" icon={<CopyMinus />} label="Collapse All" onClick={() => ex.collapseAll()} />
          <IconButton size="xs" icon={ex.showHidden ? <Eye /> : <EyeOff />} label={ex.showHidden ? 'Hide hidden files' : 'Show hidden files'} onClick={() => ex.set({ showHidden: !ex.showHidden })} />
          <IconButton size="xs" icon={<Locate />} label="Follow active file" active={ex.followActive} onClick={() => ex.set({ followActive: !ex.followActive })} />
        </div>
      </div>
      {ex.filterOpen && (
        <div className="px-2 pb-1 shrink-0">
          <Input ref={filterRef} value={ex.filter} placeholder="Filter files…" onChange={(e) => ex.set({ filter: e.target.value })} className="text-xs" wrapperClassName="h-6"
            trailing={<button aria-label="Close filter" onClick={() => ex.set({ filterOpen: false, filter: '' })} className="text-muted hover:text-fg"><X className="size-3.5" /></button>}
            onKeyDown={(e) => { if (e.key === 'Escape') { e.preventDefault(); ex.set({ filterOpen: false, filter: '' }) } if (e.key === 'ArrowDown') { (e.currentTarget.closest('[data-explorer]')?.querySelector('[data-keyctx=explorer]') as HTMLElement | null)?.focus() } }} />
        </div>
      )}
      <div data-explorer className={cn('flex-1 min-h-0 flex flex-col', ex.treeCollapsed && 'hidden')}><FileTree /></div>
      <ExplorerBottomSections />
    </div>
  )
}

/** Explorer commands (called from shellCommands). */
export function registerExplorerCommands(): () => void {
  const a = explorerActions
  return registerCommands([
    { id: 'explorer.newFile', title: 'New File', category: 'File', run: (args) => a.current.newFile?.(args) },
    { id: 'explorer.newFolder', title: 'New Folder', category: 'File', run: (args) => a.current.newFolder?.(args) },
    { id: 'explorer.rename', title: 'Rename', category: 'Explorer', hidden: true, run: () => a.current.rename?.() },
    { id: 'explorer.delete', title: 'Delete', category: 'Explorer', hidden: true, run: () => a.current.delete?.() },
    { id: 'explorer.copy', title: 'Copy', category: 'Explorer', hidden: true, run: () => a.current.copy?.() },
    { id: 'explorer.cut', title: 'Cut', category: 'Explorer', hidden: true, run: () => a.current.cut?.() },
    { id: 'explorer.paste', title: 'Paste', category: 'Explorer', hidden: true, run: () => a.current.paste?.() },
    { id: 'explorer.filter', title: 'Filter Explorer', category: 'Explorer', hidden: true, run: () => useExplorer.setState({ filterOpen: true }) },
    { id: 'explorer.reveal', title: 'Reveal Active File in Explorer', category: 'Explorer', run: async (args) => { const p = (args as { path?: string } | undefined)?.path ?? (await runCommand<{ path?: string } | string | undefined>('editor.getActiveFile')); const path = typeof p === 'string' ? p : p?.path; const { useUi } = await import('@/store/ui'); useUi.getState().showSidebar('explorer'); if (path) a.current.reveal?.({ path }) } },
    { id: 'explorer.refresh', title: 'Refresh Explorer', category: 'Explorer', run: () => { const r = useWorkspace.getState().workspace?.root; if (r) void useExplorer.getState().loadDir(r, true) } },
    { id: 'explorer.collapseAll', title: 'Collapse Folders in Explorer', category: 'Explorer', run: () => useExplorer.getState().collapseAll() },
    { id: 'explorer.toggleHidden', title: 'Toggle Hidden Files', category: 'Explorer', run: () => useExplorer.setState((s) => ({ showHidden: !s.showHidden })) },
    { id: 'explorer.openFolderDialog', title: 'Open Folder…', category: 'File', hidden: true, run: async () => { const p = await openDialog({ directory: true, multiple: false }); if (typeof p === 'string') await useWorkspace.getState().open(p) } },
  ])
}

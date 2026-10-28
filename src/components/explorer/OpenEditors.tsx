import { ChevronDown, ChevronRight, X, Circle } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { runCommand, hasCommand } from '@/lib/commands'
import { subscribe } from '@/lib/bus'
import { baseName, relPath } from '@/store/workspace'
import { useExplorer } from '@/store/explorer'
import { fileIconFor } from './fileIcons'
import { cn } from '@/components/common'

export interface OpenFileInfo { path: string; dirty?: boolean; active?: boolean; preview?: boolean; title?: string }

/** "Open Editors" section fed by the editor module's hidden `editor.getOpenFiles` command and the `editor.tabs` bus topic. */
export function OpenEditors() {
  const collapsed = useExplorer((s) => s.openEditorsCollapsed)
  const [files, setFiles] = useState<OpenFileInfo[]>([])
  const refresh = useCallback(async () => {
    if (!hasCommand('editor.getOpenFiles')) { setFiles([]); return }
    try {
      const raw = hasCommand('editor.getOpenEditors') ? await runCommand<OpenFileInfo[]>('editor.getOpenEditors') : await runCommand<(OpenFileInfo | string)[]>('editor.getOpenFiles')
      setFiles((raw ?? []).map((f) => (typeof f === 'string' ? { path: f } : f)).filter((f) => f && f.path))
    } catch { setFiles([]) }
  }, [])
  useEffect(() => { void refresh(); return subscribe('editor.tabs', () => void refresh()) }, [refresh])
  // VS Code hides this section until something is open — no placeholder row, no zero badge.
  if (!hasCommand('editor.getOpenFiles') || files.length === 0) return null
  return (
    <div className="shrink-0 border-b border-border">
      <button onClick={() => useExplorer.setState({ openEditorsCollapsed: !collapsed })} className="w-full flex items-center gap-1 h-[22px] px-1 text-[11px] font-semibold uppercase tracking-wide text-muted hover:text-fg">
        {collapsed ? <ChevronRight className="size-3.5" /> : <ChevronDown className="size-3.5" />}Open Editors
        <span className="ml-auto text-faint font-normal normal-case tracking-normal">{files.length}</span>
      </button>
      {!collapsed && (
        <div className="max-h-[200px] overflow-auto pb-1">
          {files.map((f) => (
            <div key={(f as { tabId?: string }).tabId ?? f.path} className={cn('group flex items-center gap-1.5 h-[22px] pl-4 pr-1 text-xs hover:bg-hover cursor-pointer', f.active && 'bg-selected')} onClick={() => void runCommand('editor.openFile', { path: f.path, focus: true })} title={f.path}>
              <button aria-label="Close" className="size-4 rounded flex items-center justify-center text-muted hover:bg-active opacity-0 group-hover:opacity-100 data-[dirty=true]:opacity-100 shrink-0" data-dirty={!!f.dirty}
                onClick={(e) => { e.stopPropagation(); void runCommand('editor.closeTab', { path: f.path }) }}>
                {f.dirty ? <Circle className="size-2.5 fill-current group-hover:hidden" /> : null}
                <X className={cn('size-3.5', f.dirty && 'hidden group-hover:block')} />
              </button>
              <span className="shrink-0 flex">{fileIconFor(baseName(f.path), false)}</span>
              <span className={cn('shrink-0 max-w-[70%] truncate', f.preview && 'italic')}>{f.title ?? baseName(f.path)}</span>
              <span className="min-w-0 truncate text-faint text-[11px]" dir="rtl">{relPath(f.path).split('/').slice(0, -1).join('/')}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

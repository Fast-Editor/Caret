import { Bot, ChevronLeft, ChevronRight, LayoutPanelLeft, LayoutPanelTop, MessageSquare, PanelRight, Search, Minus, Square, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { IconButton, Kbd, cn } from '@/components/common'
import { runCommand } from '@/lib/commands'
import { subscribe } from '@/lib/bus'
import { useEditor } from '@/store/editor'
import { useUi } from '@/store/ui'
import { useWorkspace } from '@/store/workspace'

function useRunningAgents(): number {
  const [n, setN] = useState(0)
  useEffect(() => subscribe<{ running: number }>('agents.status', (p) => setN(p?.running ?? 0)), [])
  return n
}

/** Compact Cursor-style title bar: navigation + command center in the middle, layout toggles on the right. ≤ 40px at 110% zoom. */
export function TitleBar() {
  const ui = useUi()
  const ws = useWorkspace((s) => s.workspace)
  const running = useRunningAgents()
  const canBack = useEditor((s) => s.navBack.length > 0)
  const canForward = useEditor((s) => s.navForward.length > 0)
  const [tabStatus, setTabStatus] = useState<{ enabled: boolean; busy?: boolean; error?: boolean } | null>(null)
  useEffect(() => subscribe<{ enabled: boolean; busy?: boolean; error?: boolean }>('tab.status', setTabStatus), [])
  const isMac = ui.platform === 'macos'
  const win = () => getCurrentWindow()
  const navCls = 'size-6 rounded-md flex items-center justify-center text-muted hover:bg-hover hover:text-fg disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-muted'
  return (
    <div data-tauri-drag-region className={cn('h-[34px] shrink-0 flex items-center bg-titlebar select-none', isMac ? 'pl-[78px] pr-2' : 'pl-2 pr-0')} onDoubleClick={() => void win().toggleMaximize()}>
      {/* left: status pills only (the window title is native) */}
      <div data-tauri-drag-region className="flex items-center gap-1 w-[150px] shrink-0 min-w-0">
        {tabStatus && <button type="button" onClick={() => void runCommand('ai.tab.toggle')} className={cn('h-5 px-1.5 rounded text-[10px] font-medium border', tabStatus.error ? 'border-danger/50 text-danger' : tabStatus.enabled ? 'border-accent/40 text-accent' : 'border-border text-faint')} title={tabStatus.error ? 'Tab completions: the last request failed' : 'Tab completions'}>{tabStatus.busy ? '…' : tabStatus.error ? 'Tab !' : 'Tab'}</button>}
        {running > 0 && <button type="button" onClick={() => void runCommand('view.agents')} className="flex items-center gap-1 h-5 px-1.5 rounded border border-accent/40 text-accent text-[10px]"><Bot className="size-3 pulse" />{running}</button>}
      </div>
      {/* centre: back / forward + command center */}
      <div data-tauri-drag-region className="flex-1 flex items-center justify-center gap-1 min-w-0">
        <button type="button" className={navCls} disabled={!canBack} onClick={() => void runCommand('go.back')} title="Go Back (⌃-)" aria-label="Go back"><ChevronLeft className="size-4" /></button>
        <button type="button" className={navCls} disabled={!canForward} onClick={() => void runCommand('go.forward')} title="Go Forward (⌃⇧-)" aria-label="Go forward"><ChevronRight className="size-4" /></button>
        <button type="button" onClick={() => ui.openPalette('files')} className="flex items-center gap-2 h-[26px] w-[min(560px,44vw)] rounded-md bg-input hover:bg-active px-2.5 text-[11.5px] text-muted" aria-label="Open command center" title="Search files and commands">
          <Search className="size-3.5 shrink-0" /><span className="truncate flex-1 text-center">{ws ? ws.name : 'Search files and commands'}</span><Kbd keys="mod+p" />
        </button>
      </div>
      {/* right: layout toggles (+ window controls off macOS) */}
      <div className="flex items-center gap-0.5 w-[150px] shrink-0 justify-end">
        <IconButton size="sm" icon={<LayoutPanelLeft />} label="Toggle Primary Sidebar" shortcut="mod+b" active={ui.sidebarVisible} onClick={() => ui.toggleSidebar()} />
        <IconButton size="sm" icon={<LayoutPanelTop className="rotate-180" />} label="Toggle Panel" shortcut="mod+j" active={ui.panelVisible} onClick={() => ui.togglePanel()} />
        <IconButton size="sm" icon={<PanelRight />} label="Toggle AI Pane" shortcut="mod+shift+i" active={ui.chatVisible} onClick={() => ui.toggleChat()} />
        <IconButton size="sm" icon={<MessageSquare />} label={ui.layout === 'agents' ? 'Back to Editor' : 'Agents Layout'} shortcut="mod+alt+a" active={ui.layout === 'agents'} onClick={() => ui.set({ layout: ui.layout === 'agents' ? 'editor' : 'agents' })} />
        {!isMac && (
          <div className="flex items-stretch h-[34px] ml-2">
            <button type="button" aria-label="Minimize" className="w-11 hover:bg-hover flex items-center justify-center" onClick={() => void win().minimize()}><Minus className="size-3.5" /></button>
            <button type="button" aria-label="Maximize" className="w-11 hover:bg-hover flex items-center justify-center" onClick={() => void win().toggleMaximize()}><Square className="size-3" /></button>
            <button type="button" aria-label="Close" className="w-11 hover:bg-danger hover:text-white flex items-center justify-center" onClick={() => void win().close()}><X className="size-4" /></button>
          </div>
        )}
      </div>
    </div>
  )
}

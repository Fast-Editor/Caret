import { Bot, LayoutPanelLeft, LayoutPanelTop, MessageSquare, PanelRight, Search, Minus, Square, X, Copy } from 'lucide-react'
import { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { IconButton, Kbd, cn } from '@/components/common'
import { runCommand } from '@/lib/commands'
import { subscribe } from '@/lib/bus'
import { useUi } from '@/store/ui'
import { useWorkspace } from '@/store/workspace'

function useRunningAgents(): number {
  const [n, setN] = useState(0)
  useEffect(() => subscribe<{ running: number }>('agents.status', (p) => setN(p?.running ?? 0)), [])
  return n
}

export function TitleBar() {
  const ui = useUi()
  const ws = useWorkspace((s) => s.workspace)
  const running = useRunningAgents()
  const [tabStatus, setTabStatus] = useState<{ enabled: boolean; busy?: boolean } | null>(null)
  useEffect(() => subscribe<{ enabled: boolean; busy?: boolean }>('tab.status', setTabStatus), [])
  const isMac = ui.platform === 'macos'
  const win = () => getCurrentWindow()
  return (
    <div data-tauri-drag-region className={cn('h-[38px] shrink-0 flex items-center bg-titlebar border-b border-border select-none', isMac ? 'pl-[78px] pr-2' : 'pl-3 pr-0')} onDoubleClick={() => void win().toggleMaximize()}>
      <div data-tauri-drag-region className="flex items-center gap-2 min-w-0 w-[220px]">
        <span className="text-xs font-semibold text-fg">Caret</span>
        {ws && <span className="text-xs text-muted truncate" title={ws.root}>{ws.name}</span>}
      </div>
      <div data-tauri-drag-region className="flex-1 flex justify-center min-w-0">
        <button onClick={() => ui.openPalette('files')} className="flex items-center gap-2 h-6 w-[min(520px,46vw)] rounded-md border border-border bg-input/70 hover:bg-input px-2 text-[11px] text-muted" aria-label="Open command center">
          <Search className="size-3.5" /><span className="truncate flex-1 text-left">{ws ? `Search ${ws.name}` : 'Search files and commands'}</span><Kbd keys="mod+p" />
        </button>
      </div>
      <div className="flex items-center gap-0.5 w-[220px] justify-end">
        {tabStatus && <button onClick={() => void runCommand('ai.tab.toggle')} className={cn('h-5 px-1.5 rounded text-[10px] font-medium border', tabStatus.enabled ? 'border-accent/40 text-accent' : 'border-border text-faint')} title="Tab completions">{tabStatus.busy ? '…' : 'Tab'}</button>}
        {running > 0 && <button onClick={() => void runCommand('view.agents')} className="flex items-center gap-1 h-5 px-1.5 rounded border border-accent/40 text-accent text-[10px]"><Bot className="size-3 pulse" />{running}</button>}
        <IconButton size="sm" icon={<LayoutPanelLeft />} label="Toggle Primary Sidebar" shortcut="mod+b" active={ui.sidebarVisible} onClick={() => ui.toggleSidebar()} />
        <IconButton size="sm" icon={<LayoutPanelTop className="rotate-180" />} label="Toggle Panel" shortcut="mod+j" active={ui.panelVisible} onClick={() => ui.togglePanel()} />
        <IconButton size="sm" icon={<PanelRight />} label="Toggle AI Pane" shortcut="mod+shift+i" active={ui.chatVisible} onClick={() => ui.toggleChat()} />
        <IconButton size="sm" icon={<MessageSquare />} label={ui.layout === 'agents' ? 'Back to Editor' : 'Agents Layout'} shortcut="mod+alt+a" active={ui.layout === 'agents'} onClick={() => ui.set({ layout: ui.layout === 'agents' ? 'editor' : 'agents' })} />
        {!isMac && (
          <div className="flex items-stretch h-[38px] ml-2">
            <button aria-label="Minimize" className="w-11 hover:bg-hover flex items-center justify-center" onClick={() => void win().minimize()}><Minus className="size-3.5" /></button>
            <button aria-label="Maximize" className="w-11 hover:bg-hover flex items-center justify-center" onClick={() => void win().toggleMaximize()}><Square className="size-3" /><Copy className="hidden" /></button>
            <button aria-label="Close" className="w-11 hover:bg-danger hover:text-white flex items-center justify-center" onClick={() => void win().close()}><X className="size-4" /></button>
          </div>
        )}
      </div>
    </div>
  )
}

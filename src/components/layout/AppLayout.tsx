import { ArrowLeft, X } from 'lucide-react'
import { useCallback } from 'react'
import { ResizeHandle, cn } from '@/components/common'
import { useUi } from '@/store/ui'
import { useWorkspace } from '@/store/workspace'
import { useSettings } from '@/store/settings'
import { EditorArea } from '@/components/editor'
import { DebugToolbar } from '@/components/debug'
import { ChatPane } from '@/components/chat'
import { AgentsLayout } from '@/components/agents'
import { SettingsPage } from '@/components/settings'
import { StatusBar } from '@/components/statusbar'
import { TitleBar } from './TitleBar'
import { ActivityBar } from './ActivityBar'
import { Sidebar } from './Sidebar'
import { PanelArea } from './Panel'
import { WelcomeView } from './WelcomeView'

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v))

export function AppLayout() {
  const ui = useUi()
  const ws = useWorkspace((s) => s.workspace)
  const settings = useSettings((s) => s.settings)
  const sidebarRight = settings?.ui.sidebarPosition === 'right'
  const chatLeft = settings?.ui.chatPosition === 'left'
  const onSidebar = useCallback((d: number) => useUi.setState((s) => ({ sidebarWidth: clamp(s.sidebarWidth + d, 180, 600) })), [])
  const onChat = useCallback((d: number) => useUi.setState((s) => ({ chatWidth: clamp(s.chatWidth + d, 300, 900) })), [])
  const onPanel = useCallback((d: number) => useUi.setState((s) => ({ panelHeight: clamp(s.panelHeight - d, 100, window.innerHeight - 200), panelMaximized: false })), [])
  const zen = ui.zenMode

  const sidebar = ui.sidebarVisible && !zen && (
    <>
      {sidebarRight && <ResizeHandle direction="horizontal" onResize={(d) => onSidebar(-d)} className="bg-border" />}
      <div style={{ width: ui.sidebarWidth }} className="shrink-0 min-w-0 h-full"><Sidebar /></div>
      {!sidebarRight && <ResizeHandle direction="horizontal" onResize={onSidebar} className="bg-border" />}
    </>
  )
  const chat = ui.chatVisible && !zen && (
    <>
      {!chatLeft && <ResizeHandle direction="horizontal" onResize={(d) => onChat(-d)} className="bg-border" />}
      <div style={{ width: ui.chatWidth }} className="shrink-0 min-w-0 h-full bg-sidebar" data-keyctx="chat"><ChatPane /></div>
      {chatLeft && <ResizeHandle direction="horizontal" onResize={onChat} className="bg-border" />}
    </>
  )
  const main = (
    <div className="flex-1 min-w-0 h-full flex flex-col bg-editor relative">
      <div className={cn('min-h-0 relative', ui.panelVisible && ui.panelMaximized ? 'h-0 overflow-hidden' : 'flex-1')}>
        {ws ? <EditorArea /> : <WelcomeView />}
        <DebugToolbar />
        {ui.settingsOpen && (
          <div className="absolute inset-0 z-30 bg-editor flex flex-col">
            <div className="h-9 shrink-0 flex items-center gap-2 px-3 border-b border-border text-xs">
              <button onClick={() => ui.closeSettings()} className="flex items-center gap-1 text-muted hover:text-fg"><ArrowLeft className="size-3.5" />Back</button>
              <span className="font-medium">Settings</span>
              <button aria-label="Close settings" onClick={() => ui.closeSettings()} className="ml-auto text-muted hover:text-fg"><X className="size-4" /></button>
            </div>
            <div className="flex-1 min-h-0 overflow-auto"><SettingsPage page={ui.settingsPage} query={ui.settingsQuery} /></div>
          </div>
        )}
      </div>
      {ui.panelVisible && !zen && (
        <>
          <ResizeHandle direction="vertical" onResize={onPanel} onDoubleClick={() => ui.set({ panelMaximized: !ui.panelMaximized })} className="bg-border" />
          <div style={{ height: ui.panelMaximized ? undefined : ui.panelHeight }} className={cn('shrink-0 min-h-0', ui.panelMaximized && 'flex-1')}><PanelArea /></div>
        </>
      )}
    </div>
  )

  return (
    <div className="h-full w-full flex flex-col bg-app text-fg overflow-hidden">
      {!zen && <TitleBar />}
      {ui.layout === 'agents' ? (
        <div className="flex-1 min-h-0 relative">
          <AgentsLayout />
          <button onClick={() => ui.set({ layout: 'editor' })} className="absolute top-2 left-2 z-40 flex items-center gap-1 rounded-md border border-border bg-panel px-2 h-6 text-xs text-muted hover:text-fg shadow-[var(--shadow)]"><ArrowLeft className="size-3.5" />Editor</button>
        </div>
      ) : (
        <div className="flex-1 min-h-0 flex">
          {!zen && !sidebarRight && <ActivityBar />}
          {!sidebarRight && sidebar}
          {chatLeft && chat}
          {main}
          {!chatLeft && chat}
          {sidebarRight && sidebar}
          {!zen && sidebarRight && <ActivityBar />}
        </div>
      )}
      {!zen && <StatusBar />}
    </div>
  )
}

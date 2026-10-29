import { ArrowLeft, X } from 'lucide-react'
import { useCallback, useEffect } from 'react'
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
import { Pane } from './Pane'
import './layout.css'

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
  useScrollReveal()

  const sidebarOpen = ui.sidebarVisible && !zen
  const chatOpen = ui.chatVisible && !zen
  const panelOpen = ui.panelVisible && !zen
  const sidebar = (
    <>
      {sidebarOpen && sidebarRight && <ResizeHandle direction="horizontal" onResize={(d) => onSidebar(-d)} />}
      <Pane open={sidebarOpen} size={ui.sidebarWidth} axis="x" edge={sidebarRight ? 'end' : 'start'} className="h-full"><Sidebar /></Pane>
      {sidebarOpen && !sidebarRight && <ResizeHandle direction="horizontal" onResize={onSidebar} />}
    </>
  )
  const chat = (
    <>
      {chatOpen && !chatLeft && <ResizeHandle direction="horizontal" onResize={(d) => onChat(-d)} />}
      <Pane open={chatOpen} size={ui.chatWidth} axis="x" edge={chatLeft ? 'start' : 'end'} className="h-full bg-sidebar" data-keyctx="chat"><ChatPane /></Pane>
      {chatOpen && chatLeft && <ResizeHandle direction="horizontal" onResize={onChat} />}
    </>
  )
  const main = (
    <div className="flex-1 min-w-0 h-full flex flex-col bg-editor relative">
      <div className={cn('min-h-0 relative', ui.panelVisible && ui.panelMaximized ? 'h-0 overflow-hidden' : 'flex-1')}>
        {ws ? <EditorArea /> : <WelcomeView />}
        <DebugToolbar />
        {ui.settingsOpen && (
          <div className="absolute inset-0 z-30 bg-editor flex flex-col animate-fade">
            <div className="h-9 shrink-0 flex items-center gap-2 px-3 border-b border-border text-xs">
              <button onClick={() => ui.closeSettings()} className="flex items-center gap-1 text-muted hover:text-fg"><ArrowLeft className="size-3.5" />Back</button>
              <span className="font-medium">Settings</span>
              <button aria-label="Close settings" onClick={() => ui.closeSettings()} className="ml-auto text-muted hover:text-fg"><X className="size-4" /></button>
            </div>
            <div className="flex-1 min-h-0 overflow-auto"><SettingsPage page={ui.settingsPage} query={ui.settingsQuery} /></div>
          </div>
        )}
      </div>
      {panelOpen && <ResizeHandle direction="vertical" onResize={onPanel} onDoubleClick={() => ui.set({ panelMaximized: !ui.panelMaximized })} />}
      <Pane open={panelOpen} size={ui.panelMaximized ? undefined : ui.panelHeight} axis="y" className={cn(ui.panelMaximized && 'flex-1')}><PanelArea /></Pane>
    </div>
  )

  return (
    <div className="h-full w-full flex flex-col bg-app text-fg overflow-hidden">
      <Pane open={!zen} size={34} axis="y" edge="start"><TitleBar /></Pane>
      {ui.layout === 'agents' ? (
        <div key="agents" className="flex-1 min-h-0 relative layout-swap">
          <AgentsLayout />
          <button onClick={() => ui.set({ layout: 'editor' })} className="absolute top-2 left-2 z-40 flex items-center gap-1 rounded-md surface-float px-2 h-6 text-xs text-muted hover:text-fg"><ArrowLeft className="size-3.5" />Editor</button>
        </div>
      ) : (
        <div key="editor" className="flex-1 min-h-0 flex layout-swap">
          {!zen && !sidebarRight && <ActivityBar />}
          {!sidebarRight && sidebar}
          {chatLeft && chat}
          {main}
          {!chatLeft && chat}
          {sidebarRight && sidebar}
          {!zen && sidebarRight && <ActivityBar />}
        </div>
      )}
      <Pane open={!zen} size={22} axis="y" edge="end"><StatusBar /></Pane>
    </div>
  )
}

/** Mark any element as `.is-scrolling` while it scrolls so its overlay scrollbar shows, then let it fade. */
function useScrollReveal() {
  useEffect(() => {
    const timers = new WeakMap<Element, number>()
    const onScroll = (e: Event) => {
      const el = e.target as Element | null
      if (!el || !(el instanceof Element)) return
      el.classList.add('is-scrolling')
      const prev = timers.get(el); if (prev) window.clearTimeout(prev)
      timers.set(el, window.setTimeout(() => el.classList.remove('is-scrolling'), 700))
    }
    document.addEventListener('scroll', onScroll, true)
    return () => document.removeEventListener('scroll', onScroll, true)
  }, [])
}

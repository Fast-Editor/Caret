import { useEditor, type EditorGroup as Group } from '@/store/editor'
import { TabBar } from './TabBar'
import { Breadcrumbs } from './Breadcrumbs'
import { MonacoEditor } from './MonacoEditor'
import { DiffEditor } from './DiffEditor'
import { EmptyState } from './EmptyState'
import { WelcomeTab } from './tabs/WelcomeTab'
import { SettingsTab } from './tabs/SettingsTab'
import { PlanTab } from './tabs/PlanTab'
import { ImageTab } from './tabs/ImageTab'
import { MarkdownPreviewTab } from './tabs/MarkdownPreviewTab'
import { ReviewTab } from './tabs/ReviewTab'
import { BrowserTab } from './tabs/BrowserTab'
import { useSettings } from '@/store/settings'
import { clsx } from 'clsx'
import { useEffect, useRef } from 'react'

export function EditorGroupView({ group }: { group: Group }) {
  const active = useEditor((s) => s.activeGroupId === group.id)
  const tab = group.tabs.find((t) => t.id === group.activeTabId)
  const breadcrumbs = useSettings((s) => s.settings?.ui.breadcrumbs ?? true)
  // Crossfade the content wrapper when the active tab changes (restart the one-shot animation; the editor instance is untouched).
  const swapRef = useRef<HTMLDivElement>(null)
  const lastTab = useRef<string | undefined>(tab?.id)
  useEffect(() => {
    const el = swapRef.current
    if (!el || lastTab.current === tab?.id) return
    lastTab.current = tab?.id
    el.classList.remove('tab-swap'); void el.offsetWidth; el.classList.add('tab-swap')
  }, [tab?.id])
  return (
    <div className={clsx('h-full w-full flex flex-col min-w-0 min-h-0 bg-editor', !active && 'opacity-[0.98]')} onMouseDownCapture={() => useEditor.getState().setActiveGroup(group.id)}
      onDragOver={(e) => { if (e.dataTransfer.types.includes('text/caret-tab') || e.dataTransfer.types.includes('text/caret-path')) e.preventDefault() }}
      onDrop={(e) => { const raw = e.dataTransfer.getData('text/caret-tab'); if (raw) { const { group: g, tabId } = JSON.parse(raw) as { group: string; tabId: string }; if (g !== group.id) useEditor.getState().moveTab(g, tabId, group.id) } const p = e.dataTransfer.getData('text/caret-path'); if (p) void useEditor.getState().openFile(p, { group: group.id, preview: false }) }}>
      <TabBar group={group} active={active} />
      {!tab ? <EmptyState /> : (
        <>
          {breadcrumbs && (tab.kind === 'file') && <Breadcrumbs tab={tab} groupId={group.id} />}
          <div ref={swapRef} className="flex-1 min-h-0 relative">
            {tab.kind === 'file' || tab.kind === 'untitled' ? <MonacoEditor groupId={group.id} tab={tab} active={active} />
              : tab.kind === 'diff' && tab.diffMeta ? <DiffEditor meta={tab.diffMeta} onDecision={() => void useEditor.getState().closeTab(group.id, tab.id, { force: true })} />
              : tab.kind === 'welcome' ? <WelcomeTab />
              : tab.kind === 'settings' ? <SettingsTab page={tab.settingsPage} query={tab.settingsQuery} />
              : tab.kind === 'keybindings' ? <SettingsTab page="keybindings" />
              : tab.kind === 'plan' ? <PlanTab path={tab.path!} />
              : tab.kind === 'image' ? <ImageTab path={tab.path!} />
              : tab.kind === 'markdown-preview' ? <MarkdownPreviewTab path={tab.path!} />
              : tab.kind === 'review' ? <ReviewTab />
              : tab.kind === 'browser' ? <BrowserTab url={tab.url} tabId={tab.browserTabId} />
              : <EmptyState />}
          </div>
        </>
      )}
    </div>
  )
}

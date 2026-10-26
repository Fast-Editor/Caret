import { ChevronsDownUp, ChevronsUpDown, X } from 'lucide-react'
import { lazy, Suspense } from 'react'
import { IconButton, Tabs, cn } from '@/components/common'
import { useUi, type PanelView } from '@/store/ui'
import { useOutput } from '@/store/notifications'
import { OutputPanel } from './OutputPanel'
import { ProblemsPanel } from '@/components/editor'
import { ReviewPanel } from '@/components/review'

const TerminalPanel = lazy(() => import('@/components/terminal').then((m) => ({ default: m.TerminalPanel })))

export function PanelArea() {
  const ui = useUi()
  const errors = useOutput((s) => s.lines.filter((l) => l.level === 'error').length)
  const items: { id: PanelView; label: string; badge?: number }[] = [
    { id: 'terminal', label: 'Terminal' }, { id: 'problems', label: 'Problems' }, { id: 'output', label: 'Output', badge: errors || undefined }, { id: 'review', label: 'Review' },
  ]
  return (
    <div className="h-full flex flex-col bg-panel" data-keyctx="panel">
      <div className="h-8 shrink-0 flex items-center px-2 border-b border-border">
        <Tabs items={items} value={ui.panelView} onChange={(v) => ui.set({ panelView: v })} size="xs" />
        <div className="ml-auto flex items-center">
          <IconButton size="xs" icon={ui.panelMaximized ? <ChevronsDownUp /> : <ChevronsUpDown />} label={ui.panelMaximized ? 'Restore Panel Size' : 'Maximize Panel'} onClick={() => ui.set({ panelMaximized: !ui.panelMaximized })} />
          <IconButton size="xs" icon={<X />} label="Close Panel" onClick={() => ui.set({ panelVisible: false, panelMaximized: false })} />
        </div>
      </div>
      <div className="flex-1 min-h-0">
        <div className={cn('h-full', ui.panelView !== 'terminal' && 'hidden')}><Suspense fallback={<div className="p-3 text-xs text-muted">Loading terminal…</div>}><TerminalPanel /></Suspense></div>
        {ui.panelView === 'problems' && <div className="h-full overflow-auto"><ProblemsPanel /></div>}
        {ui.panelView === 'output' && <OutputPanel />}
        {ui.panelView === 'review' && <div className="h-full overflow-auto"><ReviewPanel /></div>}
        {ui.panelView === 'ports' && <div className="p-3 text-xs text-muted">No forwarded ports.</div>}
      </div>
    </div>
  )
}

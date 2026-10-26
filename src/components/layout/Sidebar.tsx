import { useUi } from '@/store/ui'
import { ExplorerView } from '@/components/explorer'
import { SearchView } from '@/components/search'
import { ScmView } from '@/components/scm'
import { AgentsSidebar } from '@/components/chat'
import { OutlineView } from '@/components/editor'
import { RulesView } from './RulesView'

export function Sidebar() {
  const view = useUi((s) => s.sidebarView)
  return (
    <div className="h-full bg-sidebar flex flex-col min-w-0" data-keyctx="sidebar">
      {view === 'explorer' && <ExplorerView />}
      {view === 'search' && <SearchView />}
      {view === 'scm' && <ScmView />}
      {view === 'agents' && <AgentsSidebar />}
      {view === 'outline' && <OutlineView />}
      {view === 'rules' && <RulesView />}
    </div>
  )
}

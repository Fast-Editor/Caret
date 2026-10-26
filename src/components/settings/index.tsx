/**
 * Settings UI — Cursor-style settings page with left navigation, search, and user/workspace scope.
 * Rendered by the shell inside an editor tab: <SettingsPage page="models" query="font" />
 */
import { useEffect, useMemo, useState } from 'react'
import { Search, Settings as SettingsIcon, Brain, Zap, Bot, FileText, Database, Layers, Server, Webhook, Palette, FlaskConical, Keyboard, Info, ExternalLink } from 'lucide-react'
import { SETTINGS_PAGES, searchFields, type SettingsPageId } from '@/lib/settingsSchema'
import { initSettingsStore, useSettings } from '@/store/settings'
import { initWorkspaceStore, useWorkspace } from '@/store/workspace'
import { runCommand } from '@/lib/commands'
import { api } from '@/lib/api'
import { useSettingsScope } from './useField'
import { FieldControl } from './FieldControl'
import { Section, cn, notify, errorMessage } from './primitives'
import { GeneralPage } from './pages/General'
import { ModelsPage } from './pages/Models'
import { TabPage } from './pages/Tab'
import { AgentPage } from './pages/Agent'
import { RulesPage } from './pages/Rules'
import { MemoriesPage } from './pages/Memories'
import { IndexingPage } from './pages/Indexing'
import { McpPage } from './pages/Mcp'
import { AgentsPage } from './pages/Agents'
import { HooksPage } from './pages/Hooks'
import { AppearancePage } from './pages/Appearance'
import { BetaPage } from './pages/Beta'
import { KeybindingsPage } from './pages/Keybindings'
import { AboutPage } from './pages/About'

const ICONS: Record<string, React.ComponentType<{ size?: number; className?: string }>> = {
  settings: SettingsIcon, brain: Brain, zap: Zap, bot: Bot, 'file-text': FileText, database: Database, layers: Layers, server: Server, webhook: Webhook, palette: Palette, flask: FlaskConical, keyboard: Keyboard, info: Info,
}

const PAGE_ALIASES: Record<string, SettingsPageId> = { model: 'models', providers: 'models', apikeys: 'models', autocomplete: 'tab', agents: 'agent', mcp: 'mcp', index: 'indexing', docs: 'indexing', theme: 'appearance', editor: 'appearance', shortcuts: 'keybindings', keyboard: 'keybindings' }

export function SettingsPage({ page, query }: { page?: string; query?: string }) {
  const [active, setActive] = useState<SettingsPageId>(() => resolvePage(page))
  const [q, setQ] = useState(query ?? '')
  const scope = useSettingsScope((s) => s.scope)
  const setScope = useSettingsScope((s) => s.setScope)
  const hasWorkspace = useWorkspace((s) => !!s.workspace)
  const loaded = useSettings((s) => s.loaded)

  useEffect(() => { initSettingsStore(); initWorkspaceStore() }, [])
  useEffect(() => { if (page) setActive(resolvePage(page)) }, [page])
  useEffect(() => { if (query !== undefined) setQ(query) }, [query])
  useEffect(() => { if (!hasWorkspace && scope === 'workspace') setScope('user') }, [hasWorkspace, scope, setScope])

  const results = useMemo(() => (q.trim() ? searchFields(q) : null), [q])

  const openJson = async () => {
    try { const p = await api.settings.path(scope === 'workspace' ? 'workspace' : 'user'); await runCommand('editor.openFile', { path: p }) } catch (e) { notify('error', 'Cannot open settings file', errorMessage(e)) }
  }

  return (
    <div className="h-full w-full flex bg-editor text-fg selectable">
      {/* nav */}
      <nav className="w-[220px] shrink-0 border-r border-border bg-sidebar flex flex-col">
        <div className="p-3 pb-2">
          <div className="relative">
            <Search size={13} className="absolute left-2 top-1/2 -translate-y-1/2 text-muted" />
            <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search settings" aria-label="Search settings"
              className="h-7 w-full pl-7 pr-2 rounded-[var(--radius)] border border-border bg-input text-xs placeholder:text-faint focus:border-accent outline-none" />
          </div>
        </div>
        <ul className="flex-1 overflow-auto px-2 pb-2">
          {SETTINGS_PAGES.map((p) => {
            const Icon = ICONS[p.icon] ?? SettingsIcon
            const selected = !results && active === p.id
            return (
              <li key={p.id}>
                <button onClick={() => { setQ(''); setActive(p.id) }} className={cn('w-full flex items-center gap-2 h-7 px-2 rounded text-xs text-left', selected ? 'bg-selected text-fg' : 'text-muted hover:bg-hover hover:text-fg')}>
                  <Icon size={14} className="shrink-0" /><span className="truncate-1">{p.title}</span>
                </button>
              </li>
            )
          })}
        </ul>
        <div className="p-2 border-t border-border text-[10px] text-faint">Changes save automatically</div>
      </nav>

      {/* content */}
      <div className="flex-1 min-w-0 flex flex-col">
        <header className="h-[44px] shrink-0 flex items-center justify-between px-5 border-b border-border">
          <div className="flex items-center gap-3 min-w-0">
            <h2 className="text-sm font-semibold truncate-1">{results ? `Search results for “${q}”` : SETTINGS_PAGES.find((p) => p.id === active)?.title}</h2>
            {!results && <span className="text-[11px] text-muted truncate-1 hidden md:inline">{SETTINGS_PAGES.find((p) => p.id === active)?.description}</span>}
          </div>
          <div className="flex items-center gap-2 shrink-0">
            <div role="tablist" aria-label="Settings scope" className="inline-flex rounded-[var(--radius)] border border-border overflow-hidden text-[11px]">
              <button role="tab" aria-selected={scope === 'user'} onClick={() => setScope('user')} className={cn('h-6 px-2.5', scope === 'user' ? 'bg-active text-fg' : 'text-muted hover:bg-hover')}>User</button>
              <button role="tab" aria-selected={scope === 'workspace'} disabled={!hasWorkspace} title={hasWorkspace ? 'Workspace settings (.caret/settings.json)' : 'Open a folder to edit workspace settings'} onClick={() => setScope('workspace')} className={cn('h-6 px-2.5 border-l border-border disabled:opacity-40', scope === 'workspace' ? 'bg-active text-fg' : 'text-muted hover:bg-hover')}>Workspace</button>
            </div>
            <button onClick={openJson} className="inline-flex items-center gap-1 h-6 px-2 rounded text-[11px] text-muted hover:text-fg hover:bg-hover" title="Open settings.json"><ExternalLink size={12} />JSON</button>
          </div>
        </header>
        <div className="flex-1 overflow-auto">
          <div className="max-w-[920px] px-5 py-4">
            {!loaded && <div className="text-muted text-xs">Loading settings…</div>}
            {loaded && results && <SearchResults results={results} onJump={(p) => { setQ(''); setActive(p) }} />}
            {loaded && !results && <PageBody page={active} />}
          </div>
        </div>
      </div>
    </div>
  )
}

function resolvePage(p?: string): SettingsPageId {
  if (!p) return 'general'
  const key = p.toLowerCase()
  if (SETTINGS_PAGES.some((x) => x.id === key)) return key as SettingsPageId
  return PAGE_ALIASES[key] ?? 'general'
}

function SearchResults({ results, onJump }: { results: ReturnType<typeof searchFields>; onJump: (p: SettingsPageId) => void }) {
  const byPage = useMemo(() => {
    const m = new Map<SettingsPageId, typeof results>()
    for (const f of results) { if (!m.has(f.page)) m.set(f.page, []); m.get(f.page)!.push(f) }
    return [...m.entries()]
  }, [results])
  if (!results.length) return <div className="text-muted text-xs py-8 text-center">No settings match. Try different words — e.g. “font”, “auto run”, “theme”.</div>
  return (
    <>
      {byPage.map(([page, list]) => (
        <Section key={page} title={<button className="hover:underline" onClick={() => onJump(page)}>{SETTINGS_PAGES.find((p) => p.id === page)?.title}</button>} description={`${list.length} match${list.length === 1 ? '' : 'es'}`}>
          {list.map((f) => <FieldControl key={f.id} field={f} />)}
        </Section>
      ))}
    </>
  )
}

function PageBody({ page }: { page: SettingsPageId }) {
  switch (page) {
    case 'general': return <GeneralPage />
    case 'models': return <ModelsPage />
    case 'tab': return <TabPage />
    case 'agent': return <AgentPage />
    case 'rules': return <RulesPage />
    case 'memories': return <MemoriesPage />
    case 'indexing': return <IndexingPage />
    case 'agents': return <AgentsPage />
    case 'mcp': return <McpPage />
    case 'hooks': return <HooksPage />
    case 'appearance': return <AppearancePage />
    case 'beta': return <BetaPage />
    case 'keybindings': return <KeybindingsPage />
    case 'about': return <AboutPage />
  }
}

export default SettingsPage

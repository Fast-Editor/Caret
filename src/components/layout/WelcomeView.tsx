import { FolderOpen, Clock, Sparkles, BookOpen, Keyboard, Settings } from 'lucide-react'
import { Button, Kbd } from '@/components/common'
import { runCommand } from '@/lib/commands'
import { useWorkspace } from '@/store/workspace'

const ACTIONS: { icon: typeof Sparkles; label: string; run: () => void }[] = [
  { icon: Sparkles, label: 'Start a new agent chat', run: () => void runCommand('chat.new') },
  { icon: Settings, label: 'Configure models & API keys', run: () => void runCommand('settings.open', { page: 'models' }) },
  { icon: Keyboard, label: 'Keyboard shortcuts', run: () => void runCommand('help.shortcuts') },
  { icon: BookOpen, label: 'Documentation', run: () => void runCommand('help.docs') },
]
const SHORTCUTS: [string, string][] = [['Command palette', 'mod+shift+p'], ['Go to file', 'mod+p'], ['Chat / add selection', 'mod+l'], ['New agent', 'mod+i'], ['Inline edit', 'mod+k'], ['Terminal', 'ctrl+`']]

/** Shown in the main area when no folder is open: one centred column (title, primary action, links, shortcuts, recent). */
export function WelcomeView() {
  const recent = useWorkspace((s) => s.recent)
  return (
    <div className="h-full w-full overflow-auto flex items-center justify-center px-8 py-10">
      <div className="w-full max-w-md flex flex-col items-center text-center">
        <h1 className="text-3xl font-semibold tracking-tight">Caret</h1>
        <p className="text-muted mt-1.5 text-sm">A memory-efficient, Rust-powered AI code editor.</p>
        <Button variant="primary" size="md" className="mt-7" icon={<FolderOpen className="size-4" />} onClick={() => void runCommand('workspace.openFolder')}>Open Folder…</Button>
        <div className="mt-5 flex flex-col items-center gap-1.5">
          {ACTIONS.map(({ icon: Icon, label, run }) => (
            <button key={label} type="button" onClick={run} className="text-accent text-sm hover:underline inline-flex items-center gap-2"><Icon className="size-4" />{label}</button>
          ))}
        </div>
        <div className="mt-8 w-full max-w-xs grid grid-cols-[1fr_auto] gap-x-6 gap-y-1.5 text-xs text-muted">
          {SHORTCUTS.map(([label, keys]) => (<div key={label} className="contents"><span className="text-left">{label}</span><span className="justify-self-end"><Kbd keys={keys} /></span></div>))}
        </div>
        <div className="mt-8 w-full max-w-xs">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-faint mb-2 flex items-center justify-center gap-1.5"><Clock className="size-3.5" />Recent</h2>
          {recent.length === 0 && <div className="text-xs text-faint">No recent folders.</div>}
          <div className="flex flex-col">
            {recent.slice(0, 8).map((r) => (
              <button key={r.path} type="button" disabled={!r.exists} onClick={() => void runCommand('workspace.openFolder', { path: r.path })} className="text-xs h-7 rounded px-2 hover:bg-hover disabled:opacity-40 truncate" title={r.path}>
                <span className="text-fg">{r.name}</span> <span className="text-faint">{r.path.replace(/^\/Users\/[^/]+/, '~').replace(new RegExp(`/${r.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`), '')}</span>
              </button>
            ))}
          </div>
        </div>
      </div>
    </div>
  )
}

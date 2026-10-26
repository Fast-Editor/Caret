import { FolderOpen, Clock, Sparkles, BookOpen, Keyboard, Settings } from 'lucide-react'
import { Button, Kbd } from '@/components/common'
import { runCommand } from '@/lib/commands'
import { useWorkspace } from '@/store/workspace'

/** Shown in the main area when no folder is open. */
export function WelcomeView() {
  const recent = useWorkspace((s) => s.recent)
  return (
    <div className="h-full w-full overflow-auto flex items-start justify-center pt-[10vh] px-8">
      <div className="w-full max-w-3xl grid grid-cols-[1.2fr_1fr] gap-10">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Caret</h1>
          <p className="text-muted mt-1 text-sm">A memory-efficient, Rust-powered AI code editor.</p>
          <div className="mt-6 flex flex-col gap-2 items-start">
            <Button variant="primary" size="md" icon={<FolderOpen className="size-4" />} onClick={() => void runCommand('workspace.openFolder')}>Open Folder…</Button>
            <button onClick={() => void runCommand('chat.new')} className="text-accent text-sm hover:underline flex items-center gap-2"><Sparkles className="size-4" />Start a new agent chat</button>
            <button onClick={() => void runCommand('settings.open', { page: 'models' })} className="text-accent text-sm hover:underline flex items-center gap-2"><Settings className="size-4" />Configure models & API keys</button>
            <button onClick={() => void runCommand('help.shortcuts')} className="text-accent text-sm hover:underline flex items-center gap-2"><Keyboard className="size-4" />Keyboard shortcuts</button>
            <button onClick={() => void runCommand('help.docs')} className="text-accent text-sm hover:underline flex items-center gap-2"><BookOpen className="size-4" />Documentation</button>
          </div>
          <div className="mt-8 grid grid-cols-2 gap-x-6 gap-y-1.5 text-xs text-muted">
            <span>Command palette</span><Kbd keys="mod+shift+p" />
            <span>Go to file</span><Kbd keys="mod+p" />
            <span>Chat / add selection</span><Kbd keys="mod+l" />
            <span>New agent</span><Kbd keys="mod+i" />
            <span>Inline edit</span><Kbd keys="mod+k" />
            <span>Terminal</span><Kbd keys="ctrl+`" />
          </div>
        </div>
        <div>
          <h2 className="text-sm font-semibold mb-2 flex items-center gap-2"><Clock className="size-4 text-muted" />Recent</h2>
          {recent.length === 0 && <div className="text-xs text-faint">No recent folders.</div>}
          <div className="flex flex-col">
            {recent.slice(0, 10).map((r) => (
              <button key={r.path} disabled={!r.exists} onClick={() => void runCommand('workspace.openFolder', { path: r.path })} className="text-left text-xs h-7 rounded px-2 hover:bg-hover disabled:opacity-40 truncate" title={r.path}>
                <span className="text-accent">{r.name}</span> <span className="text-faint">{r.path.replace(/^\/Users\/[^/]+/, '~').replace(new RegExp(`/${r.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`), '')}</span>
              </button>
            ))}
          </div>
        </div>
      </div>
    </div>
  )
}

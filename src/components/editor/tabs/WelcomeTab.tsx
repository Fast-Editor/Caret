import { FolderOpen, Clock, Sparkles, Keyboard, BookOpen, Settings2, AppWindow, ExternalLink } from 'lucide-react'
import { IconButton } from '@/components/common'
import { useWorkspace } from '@/store/workspace'
import { useSettings } from '@/store/settings'
import { runCommand } from '@/lib/commands'
import { displayPath, opensInNewWindow } from '@/lib/windowing'

export function WelcomeTab() {
  const recent = useWorkspace((s) => s.recent)
  const ws = useWorkspace((s) => s.workspace)
  const openFoldersIn = useSettings((s) => s.settings?.window.openFoldersIn)
  // A folder is open here, so a recent folder follows `window.openFoldersIn`; Alt-click does the opposite.
  const recentHint = opensInNewWindow(!!ws, openFoldersIn) ? 'Opens in a new window · Alt-click opens in this window' : 'Opens in this window · Alt-click opens in a new window'
  return (
    <div className="h-full overflow-auto bg-editor">
      <div className="max-w-3xl mx-auto px-10 py-14 text-fg">
        <div className="flex items-end gap-3 mb-1"><span className="text-4xl font-semibold tracking-tight">Caret</span><span className="text-muted mb-1">Rust-powered AI code editor</span></div>
        <div className="text-muted text-sm mb-10">Memory-efficient. Agentic. Yours.</div>
        <div className="grid grid-cols-2 gap-10">
          <div>
            <h3 className="text-sm font-medium mb-3">Start</h3>
            <div className="space-y-1.5 text-sm">
              <button className="flex items-center gap-2 text-accent hover:underline" onClick={() => void runCommand('workspace.openFolder')}><FolderOpen size={15} />Open Folder…</button>
              <button className="flex items-center gap-2 text-accent hover:underline" onClick={() => void runCommand('file.new')}><Sparkles size={15} />New File</button>
              <button className="flex items-center gap-2 text-accent hover:underline" onClick={() => void runCommand('file.newWindow')}><AppWindow size={15} />New Window</button>
              <button className="flex items-center gap-2 text-accent hover:underline" onClick={() => void runCommand('chat.new')}><Sparkles size={15} />New Agent Chat</button>
              <button className="flex items-center gap-2 text-accent hover:underline" onClick={() => void runCommand('settings.open', { page: 'models' })}><Settings2 size={15} />Configure models & API keys</button>
            </div>
            <h3 className="text-sm font-medium mt-8 mb-3 flex items-center gap-2"><Clock size={14} />Recent</h3>
            <div className="space-y-1 text-sm">
              {recent.filter((r) => r.path !== ws?.root).slice(0, 8).map((r) => (
                <div key={r.path} className="group flex items-center gap-1">
                  <button disabled={!r.exists} className="flex items-baseline gap-2 flex-1 min-w-0 text-left disabled:opacity-40" title={`${r.path}\n${recentHint}`} onClick={(e) => void runCommand('workspace.openFolder', { path: r.path, invert: e.altKey })}>
                    <span className="text-accent hover:underline">{r.name}</span><span className="text-faint text-xs truncate">{displayPath(r.path)}</span>
                  </button>
                  <IconButton size="xs" icon={<ExternalLink />} label="Open in New Window" disabled={!r.exists} className="invisible group-hover:visible group-focus-within:visible" onClick={() => void runCommand('workspace.openFolder', { path: r.path, newWindow: true })} />
                </div>
              ))}
              {!recent.length && <div className="text-muted text-xs">No recent folders</div>}
            </div>
          </div>
          <div>
            <h3 className="text-sm font-medium mb-3 flex items-center gap-2"><Keyboard size={14} />Shortcuts</h3>
            <div className="grid grid-cols-[1fr_auto] gap-y-1.5 gap-x-4 text-xs text-muted">
              {([['Command palette', '⌘⇧P'], ['Go to file', '⌘P'], ['Chat / add selection', '⌘L'], ['New agent', '⌘I'], ['Inline edit', '⌘K'], ['Accept Tab suggestion', 'Tab'], ['Accept next word', '⌘→'], ['Find in files', '⌘⇧F'], ['New window', '⌘⇧N'], ['Toggle terminal', '⌃`'], ['Settings', '⌘,']] as const).map(([l, k]) => <div key={l} className="contents"><span>{l}</span><kbd className="px-1 rounded bg-input border border-border text-fg">{k}</kbd></div>)}
            </div>
            <h3 className="text-sm font-medium mt-8 mb-3 flex items-center gap-2"><BookOpen size={14} />Tips</h3>
            <ul className="text-xs text-muted space-y-1.5 list-disc pl-4">
              <li>Type <code className="mono">@</code> in chat to attach files, folders, docs, git diffs, terminal output or lint errors.</li>
              <li>Project rules live in <code className="mono">.caret/rules/*.mdc</code> (Cursor's <code className="mono">.cursor/rules</code> is read too).</li>
              <li>Plan mode writes a plan you can review and build; Ask mode never edits files.</li>
              <li>Press <kbd>Tab</kbd> on a next-edit hint to jump to the predicted change.</li>
              <li>Right-click a folder in the Explorer → Open in New Window; Window: Switch Window… in the palette moves between windows.</li>
            </ul>
          </div>
        </div>
      </div>
    </div>
  )
}

import { useState } from 'react'
import { FlaskConical, FolderInput } from 'lucide-react'
import { useChat } from '@/store/chat'
import { runCommand } from '@/lib/commands'

/** Banner shown above a scratch thread: where it lives and a one-click "make it a project" flow. */
export function ScratchBanner({ sessionId }: { sessionId: string }) {
  const root = useChat((s) => s.loaded[sessionId]?.workspaceRoot ?? '')
  const promote = useChat((s) => s.promoteScratch)
  const [busy, setBusy] = useState(false)
  const shortRoot = root.replace(/^\/Users\/[^/]+|^\/home\/[^/]+/, '~')
  const pick = async () => {
    if (busy) return
    setBusy(true)
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const picked = await open({ directory: true, multiple: false, title: 'Choose a folder for this project (new or empty)' })
      const dir = Array.isArray(picked) ? picked[0] : picked
      if (!dir) return
      await promote(sessionId, dir)
      void runCommand('app.notify', { level: 'success', title: 'Scratch thread is now a project', message: dir })
    } catch (e) {
      void runCommand('app.notify', { level: 'error', title: 'Could not make this thread a project', message: e instanceof Error ? e.message : String(e) })
    } finally { setBusy(false) }
  }
  return (
    <div className="flex items-center gap-2 px-2 h-7 shrink-0 border-b border-border bg-panel/60 text-[11px] text-muted" data-testid="scratch-banner">
      <FlaskConical size={12} className="text-accent shrink-0" />
      <span className="truncate-1 flex-1" title={root}>Scratch thread in <span className="mono">{shortRoot}</span></span>
      <button type="button" onClick={() => void pick()} disabled={busy} className="flex items-center gap-1 h-5 px-1.5 rounded border border-border hover:bg-hover text-fg text-[11px] press disabled:opacity-50"><FolderInput size={11} />{busy ? 'Moving…' : 'Make it a project…'}</button>
    </div>
  )
}

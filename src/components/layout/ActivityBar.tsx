import { Bot, Files, GitBranch, Search, Settings, BookMarked, ListTree, Bug } from 'lucide-react'
import { useEffect, useState } from 'react'
import { Tooltip, cn } from '@/components/common'
import { subscribe } from '@/lib/bus'
import { runCommand } from '@/lib/commands'
import { useGit } from '@/store/git'
import { useUi, type SidebarView } from '@/store/ui'

const ITEMS: { id: SidebarView; icon: typeof Files; label: string; shortcut: string }[] = [
  { id: 'explorer', icon: Files, label: 'Explorer', shortcut: 'mod+shift+e' },
  { id: 'search', icon: Search, label: 'Search', shortcut: 'mod+shift+f' },
  { id: 'scm', icon: GitBranch, label: 'Source Control', shortcut: 'ctrl+shift+g' },
  { id: 'agents', icon: Bot, label: 'Agents', shortcut: 'mod+shift+a' },
  { id: 'debug', icon: Bug, label: 'Run and Debug', shortcut: 'mod+shift+d' },
  { id: 'outline', icon: ListTree, label: 'Outline', shortcut: '' },
  { id: 'rules', icon: BookMarked, label: 'Rules & Context', shortcut: '' },
]

export function ActivityBar() {
  const ui = useUi()
  const git = useGit((s) => s.status)
  const [running, setRunning] = useState(0)
  useEffect(() => subscribe<{ running: number }>('agents.status', (p) => setRunning(p?.running ?? 0)), [])
  const changed = git ? git.staged.length + git.unstaged.length + git.untracked.length + git.conflicted.length : 0
  return (
    <div className="w-12 shrink-0 bg-app flex flex-col items-center py-1">
      {ITEMS.map((it) => {
        const active = ui.sidebarVisible && ui.sidebarView === it.id
        const badge = it.id === 'scm' ? changed : it.id === 'agents' ? running : 0
        return (
          <Tooltip key={it.id} label={it.label} shortcut={it.shortcut || undefined} side="right">
            <button aria-label={it.label} aria-pressed={active} onClick={() => ui.toggleSidebar(it.id)}
              className={cn('relative size-12 flex items-center justify-center text-muted hover:text-fg', active && 'text-fg before:absolute before:left-0 before:top-2 before:bottom-2 before:w-0.5 before:bg-accent')}>
              <it.icon className="size-6" strokeWidth={1.75} />
              {badge > 0 && <span className={cn('absolute bottom-2 right-2 min-w-[16px] h-4 px-1 rounded-full text-[10px] leading-4 text-center', it.id === 'agents' ? 'bg-accent text-accent-fg' : 'bg-border-strong text-fg')}>{badge > 99 ? '99+' : badge}</span>}
            </button>
          </Tooltip>
        )
      })}
      <div className="flex-1" />
      <Tooltip label="Settings" shortcut="mod+," side="right">
        <button aria-label="Settings" onClick={() => void runCommand('settings.open')} className="size-12 flex items-center justify-center text-muted hover:text-fg"><Settings className="size-6" strokeWidth={1.75} /></button>
      </Tooltip>
    </div>
  )
}

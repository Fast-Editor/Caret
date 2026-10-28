import { useEffect, useState } from 'react'
import { ChevronDown, ChevronRight, GitCommitHorizontal, Loader2 } from 'lucide-react'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import { cn } from '@/components/common'
import { relativeTime } from '@/components/settings/primitives'
import { useExplorer } from '@/store/explorer'
import { useEditor, activeTab } from '@/store/editor'
import { useWorkspace, relPath } from '@/store/workspace'
import { useGit } from '@/store/git'
import { OutlineView } from '@/components/editor/OutlineView'
import type { GitCommit } from '@/types'

/** VS Code-style collapsible sections at the bottom of the explorer: Outline (active editor symbols) and Timeline (git history of the active file). */
export function ExplorerBottomSections() {
  const outlineCollapsed = useExplorer((s) => s.outlineCollapsed)
  const timelineCollapsed = useExplorer((s) => s.timelineCollapsed)
  const set = useExplorer((s) => s.set)
  return (
    <div className="shrink-0 border-t border-border">
      <Section title="Outline" collapsed={outlineCollapsed} onToggle={() => set({ outlineCollapsed: !outlineCollapsed })}>
        <div className="h-[32vh]"><OutlineView /></div>
      </Section>
      <Section title="Timeline" collapsed={timelineCollapsed} onToggle={() => set({ timelineCollapsed: !timelineCollapsed })}>
        <Timeline />
      </Section>
    </div>
  )
}

function Section({ title, collapsed, onToggle, children }: { title: string; collapsed: boolean; onToggle: () => void; children: React.ReactNode }) {
  return (
    <div className="border-t border-border first:border-t-0">
      <button type="button" onClick={onToggle} className="w-full flex items-center gap-1 h-[22px] px-1 text-[11px] font-semibold uppercase tracking-wide text-muted hover:text-fg">
        {collapsed ? <ChevronRight className="size-3.5" /> : <ChevronDown className="size-3.5" />}{title}
      </button>
      {!collapsed && children}
    </div>
  )
}

function Timeline() {
  const path = useEditor((s) => activeTab(s)?.path)
  const root = useWorkspace((s) => s.workspace?.root)
  const headSha = useGit((s) => s.status?.headSha)
  const [commits, setCommits] = useState<GitCommit[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let alive = true
    setError(null)
    if (!path || !root || !path.startsWith(root)) { setCommits(null); return }
    setCommits(null)
    api.git.log({ path: relPath(path), limit: 60 }).then((c) => { if (alive) setCommits(c) }).catch((e) => { if (alive) { setCommits([]); setError(String((e as Error).message ?? e)) } })
    return () => { alive = false }
  }, [path, root, headSha])
  const openAt = async (c: GitCommit) => {
    if (!path) return
    const rel = relPath(path)
    let original = ''; let modified = ''
    const parent = c.parents[0]
    if (parent) { try { original = await api.git.showFile(rel, parent) } catch { original = '' } }
    try { modified = await api.git.showFile(rel, c.sha) } catch { modified = '' }
    await runCommand('editor.openDiff', { path, original, modified, title: `${rel.split('/').pop()} (${c.shortSha})` })
  }
  if (!path) return <div className="px-3 py-2 text-[11px] text-faint">Open a file to see its history.</div>
  if (commits === null && !error) return <div className="px-3 py-2 text-[11px] text-faint flex items-center gap-1.5"><Loader2 className="size-3 spin" />Loading history…</div>
  if (error) return <div className="px-3 py-2 text-[11px] text-faint">No git history available.</div>
  if (!commits?.length) return <div className="px-3 py-2 text-[11px] text-faint">No commits touch this file yet.</div>
  return (
    <div className="max-h-[32vh] overflow-auto py-0.5">
      {commits.map((c) => (
        <button key={c.sha} type="button" onClick={() => void openAt(c)} title={`${c.sha}\n${c.author}${c.body ? `\n\n${c.body}` : ''}`}
          className={cn('w-full flex items-center gap-1.5 h-[22px] pl-2 pr-2 text-xs hover:bg-hover text-left')}>
          <GitCommitHorizontal className="size-3.5 shrink-0 text-muted" strokeWidth={1.75} />
          <span className="truncate flex-1">{c.summary}</span>
          <span className="shrink-0 text-[10.5px] text-faint">{c.author.split(' ')[0]} · {relativeTime(c.time * 1000)}</span>
        </button>
      ))}
    </div>
  )
}

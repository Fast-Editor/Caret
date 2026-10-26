/**
 * Source Control sidebar view + git.* command registrations.
 * Exports: <ScmView/> (sidebar), initScmModule() (registers commands; idempotent, also runs on import).
 */
import { useEffect, useState } from 'react'
import { ArrowDown, ArrowUp, ChevronDown, Cloud, CloudDownload, CloudUpload, GitBranch, MoreHorizontal, RefreshCw, FolderGit2, Archive, ArchiveRestore, Plus } from 'lucide-react'
import { initGitStore, useGit } from '@/store/git'
import { initWorkspaceStore, useWorkspace } from '@/store/workspace'
import { registerCommands, hasCommand, runCommand, Cmd } from '@/lib/commands'
import { BranchPicker } from './BranchPicker'
import { CommitBox } from './CommitBox'
import { ChangesList, openChangeDiff } from './ChangesList'
import { History } from './History'
import { Worktrees } from './Worktrees'
import { countChanges, syncLabel, isValidBranchName } from './helpers'
import { Button, IconButton, Menu, Tooltip, Empty, confirmDialog, promptDialog, notify, errorMessage, cn } from '@/components/settings/primitives'

let branchPickerOpener: ((open: boolean) => void) | null = null

export function ScmView() {
  const { status, refreshing, syncing, error, lastRemoteOutput } = useGit()
  const ws = useWorkspace((s) => s.workspace)
  const [pickerOpen, setPickerOpen] = useState(false)
  useEffect(() => { initScmModule(); initWorkspaceStore(); branchPickerOpener = setPickerOpen; return () => { if (branchPickerOpener === setPickerOpen) branchPickerOpener = null } }, [])

  const counts = countChanges(status)
  const sync = syncLabel(status)
  const doSync = async () => {
    const g = useGit.getState()
    try {
      let r
      if (sync.action === 'publish') r = await g.push({ setUpstream: true })
      else if (sync.action === 'pull') r = await g.pull()
      else if (sync.action === 'push') r = await g.push()
      else if (sync.action === 'sync') { r = await g.pull(); if (r?.ok) r = await g.push() }
      else r = await g.fetch()
      if (r && !r.ok) notify('error', `${sync.title} failed`, r.output)
      else if (r && sync.action !== 'fetch') notify('success', sync.title === 'Publish branch to remote' ? 'Branch published' : `${sync.title} complete`)
    } catch (e) { notify('error', `${sync.title} failed`, errorMessage(e)) }
  }

  if (!ws) return <Empty icon={<GitBranch size={22} />} title="No folder open" description="Open a folder to see its source control state." />
  if (status && !status.isRepo) return (
    <Empty icon={<FolderGit2 size={22} />} title="This folder is not a git repository" description="Initialize a repository to track changes, commit and let agents use checkpoints and worktrees."
      action={<Button variant="primary" onClick={async () => { try { await useGit.getState().init(); notify('success', 'Repository initialized') } catch (e) { notify('error', 'git init failed', errorMessage(e)) } }}>Initialize Repository</Button>} />
  )

  return (
    <div className="h-full flex flex-col text-fg">
      <div className="flex items-center gap-1 h-[30px] px-2 border-b border-border">
        <button onClick={() => setPickerOpen(true)} className="flex items-center gap-1 h-6 pl-1.5 pr-1 rounded hover:bg-hover text-xs max-w-[55%]" title="Switch branch" aria-label="Current branch">
          <GitBranch size={12} className="text-muted shrink-0" />
          <span className="mono truncate-1 font-medium">{status?.isDetached ? `(${status.headSha?.slice(0, 7) ?? 'detached'})` : status?.branch ?? '…'}</span>
          <ChevronDown size={11} className="text-muted shrink-0" />
        </button>
        {status?.state && status.state !== 'clean' && status.state !== 'normal' && <span className="text-[9px] uppercase px-1 rounded bg-warning/20 text-warning">{status.state}</span>}
        <div className="flex-1" />
        {status && (status.ahead > 0 || status.behind > 0) && <span className="text-[10px] text-muted mono flex items-center gap-0.5">{status.behind > 0 && <><ArrowDown size={10} />{status.behind}</>}{status.ahead > 0 && <><ArrowUp size={10} />{status.ahead}</>}</span>}
        <Tooltip label={sync.title}><button onClick={doSync} disabled={syncing} aria-label={sync.title} className={cn('h-6 px-1.5 rounded inline-flex items-center gap-1 text-[11px] text-muted hover:text-fg hover:bg-hover disabled:opacity-50', syncing && 'pulse')}>{sync.action === 'publish' ? <CloudUpload size={13} /> : sync.action === 'pull' ? <CloudDownload size={13} /> : sync.action === 'push' ? <CloudUpload size={13} /> : sync.action === 'sync' ? <RefreshCw size={13} /> : <Cloud size={13} />}{sync.action === 'publish' && <span>Publish</span>}</button></Tooltip>
        <IconButton title="Refresh" onClick={() => void useGit.getState().refresh()} className={refreshing ? 'spin' : ''}><RefreshCw size={12} /></IconButton>
        <Menu trigger={<button aria-label="More actions" className="h-6 w-6 rounded inline-flex items-center justify-center text-muted hover:text-fg hover:bg-hover"><MoreHorizontal size={13} /></button>} items={[
          { id: 'fetch', label: 'Fetch', icon: <Cloud size={12} />, onSelect: () => void runCommand(Cmd.gitFetch) },
          { id: 'pull', label: 'Pull', icon: <CloudDownload size={12} />, onSelect: () => void runCommand(Cmd.gitPull) },
          { id: 'pull-rebase', label: 'Pull (rebase)', onSelect: () => void safe(() => useGit.getState().pull(true).then(reportRemote('Pull'))) },
          { id: 'push', label: 'Push', icon: <CloudUpload size={12} />, onSelect: () => void runCommand(Cmd.gitPush) },
          { id: 'push-force', label: 'Push (force)…', danger: true, onSelect: async () => { if (await confirmDialog({ title: 'Force push?', message: `This overwrites ${status?.upstream ?? 'the remote branch'}.`, danger: true, confirmLabel: 'Force push' })) void safe(() => useGit.getState().push({ force: true }).then(reportRemote('Force push'))) } },
          'sep',
          { id: 'branch', label: 'Switch branch…', icon: <GitBranch size={12} />, onSelect: () => setPickerOpen(true) },
          { id: 'new-branch', label: 'Create branch…', icon: <Plus size={12} />, onSelect: () => void runCommand(Cmd.gitNewBranch) },
          'sep',
          { id: 'stash', label: 'Stash changes…', icon: <Archive size={12} />, disabled: counts.total === 0, onSelect: async () => { const m = await promptDialog({ title: 'Stash message', placeholder: 'WIP', confirmLabel: 'Stash' }); if (m !== null) void safe(() => useGit.getState().stash(m || undefined)) } },
          { id: 'stash-pop', label: `Pop latest stash${status?.stashCount ? ` (${status.stashCount})` : ''}`, icon: <ArchiveRestore size={12} />, disabled: !status?.stashCount, onSelect: () => void safe(() => useGit.getState().stashPop()) },
          'sep',
          { id: 'open-worktrees', label: 'Worktrees…', icon: <FolderGit2 size={12} />, onSelect: () => void useGit.getState().loadWorktrees() },
          { id: 'open-repo-settings', label: 'Open .gitignore', onSelect: () => void runCommand('editor.openFile', { path: `${ws.root}/.gitignore` }) },
        ]} />
      </div>

      <CommitBox />
      <div className="flex-1 overflow-auto min-h-0">
        {error && <div className="px-3 py-2 text-[11px] text-danger">{error}</div>}
        <ChangesList />
        <Worktrees />
        <History />
        {lastRemoteOutput && <details className="border-t border-border px-2 py-1 text-[10px] text-muted"><summary className="cursor-default">Last remote output</summary><pre className="mono whitespace-pre-wrap max-h-32 overflow-auto selectable">{lastRemoteOutput}</pre></details>}
      </div>
      <BranchPicker open={pickerOpen} onOpenChange={setPickerOpen} />
    </div>
  )
}

async function safe(f: () => Promise<unknown>): Promise<void> { try { await f() } catch (e) { notify('error', 'Git operation failed', errorMessage(e)) } }
const reportRemote = (label: string) => (r: { ok: boolean; output: string } | null) => { if (r && !r.ok) notify('error', `${label} failed`, r.output); else if (r) notify('success', `${label} complete`) }

let registered = false
/** Register git.* commands and subscribe the store. Safe to call many times. */
export function initScmModule(): void {
  if (registered) return
  registered = true
  initGitStore()
  const g = () => useGit.getState()
  registerCommands([
    { id: Cmd.gitRefresh, title: 'Git: Refresh', category: 'Git', run: () => g().refresh() },
    { id: Cmd.gitCommit, title: 'Git: Commit', category: 'Git', run: async () => { if (!g().commitSummary.trim()) { void runCommand(Cmd.showScm); return } await safe(() => g().commit()) } },
    { id: Cmd.gitStageAll, title: 'Git: Stage All Changes', category: 'Git', run: () => safe(() => g().stageAll()) },
    { id: Cmd.gitStage, title: 'Git: Stage Files', category: 'Git', hidden: true, run: (a) => safe(() => g().stage(pathsArg(a))) },
    { id: Cmd.gitUnstage, title: 'Git: Unstage Files', category: 'Git', hidden: true, run: (a) => safe(() => g().unstage(pathsArg(a))) },
    { id: Cmd.gitDiscard, title: 'Git: Discard Changes', category: 'Git', hidden: true, run: async (a) => { const paths = pathsArg(a); if (!paths.length) return; if (await confirmDialog({ title: `Discard changes in ${paths.length} file${paths.length === 1 ? '' : 's'}?`, danger: true, confirmLabel: 'Discard' })) await safe(() => g().discard(paths)) } },
    { id: Cmd.gitPush, title: 'Git: Push', category: 'Git', run: () => safe(() => g().push().then(reportRemote('Push'))) },
    { id: Cmd.gitPull, title: 'Git: Pull', category: 'Git', run: () => safe(() => g().pull().then(reportRemote('Pull'))) },
    { id: Cmd.gitFetch, title: 'Git: Fetch', category: 'Git', run: () => safe(() => g().fetch().then((r) => { if (r && !r.ok) notify('error', 'Fetch failed', r.output) })) },
    { id: Cmd.gitCheckout, title: 'Git: Checkout Branch…', category: 'Git', run: async (a) => { const ref = (a as { ref?: string } | undefined)?.ref; if (ref) await safe(() => g().checkout(ref)); else if (branchPickerOpener) branchPickerOpener(true); else void runCommand(Cmd.showScm) } },
    { id: Cmd.gitNewBranch, title: 'Git: Create Branch…', category: 'Git', run: async () => { const name = await promptDialog({ title: 'Create branch', message: 'Branch from current HEAD and check it out.', placeholder: 'feature/name', validate: (v) => (isValidBranchName(v) ? null : 'Invalid branch name') }); if (name) await safe(() => g().createBranch(name, true).then(() => notify('success', `Switched to new branch ${name}`))) } },
    { id: Cmd.gitOpenDiff, title: 'Git: Open Changes', category: 'Git', hidden: true, run: (a) => { const { path, staged, base } = (a ?? {}) as { path: string; staged?: boolean; base?: string }; return openChangeDiff(path, !!staged, base) } },
    { id: 'git.setCommitMessage', title: 'Git: Set Commit Message', category: 'Git', hidden: true, run: (a) => { const { summary, body } = (a ?? {}) as { summary: string; body?: string }; g().setCommitMessage(summary ?? '', body ?? ''); void runCommand(Cmd.showScm) } },
    { id: 'git.stash', title: 'Git: Stash Changes', category: 'Git', run: () => safe(() => g().stash()) },
    { id: 'git.stashPop', title: 'Git: Pop Stash', category: 'Git', run: () => safe(() => g().stashPop()) },
    { id: 'git.init', title: 'Git: Initialize Repository', category: 'Git', when: () => !useGit.getState().status?.isRepo, run: () => safe(() => g().init()) },
  ])
  // AI commit message convenience (the chat module may also register ai.commitMessage; don't override if present).
  if (!hasCommand(Cmd.generateCommitMessage)) {
    registerCommands([{ id: Cmd.generateCommitMessage, title: 'AI: Generate Commit Message', category: 'Git', run: async () => {
      void runCommand(Cmd.showScm)
      useGit.setState({ generating: true })
      try { const { api } = await import('@/lib/api'); const r = await api.ai.commitMessage({ style: 'conventional', includeBody: true }); g().setCommitMessage(r.summary, r.body) } catch (e) { notify('error', 'Could not generate commit message', errorMessage(e)) } finally { useGit.setState({ generating: false }) }
    } }])
  }
}
function pathsArg(a: unknown): string[] { const x = a as { paths?: string[]; path?: string } | string[] | undefined; if (Array.isArray(x)) return x; return x?.paths ?? (x?.path ? [x.path] : []) }

// Self-register on import as a fallback for shells that don't call initScmModule().
initScmModule()

export default ScmView

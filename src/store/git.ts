/**
 * Git store — status, branches, history, worktrees and the commit draft.
 * `useGit` keeps `status` + `refresh()` (read by the status bar / editor gutter); the SCM view uses the rest.
 */
import { create } from 'zustand'
import type { GitStatus, GitBranch, GitCommit, GitWorktree, GitRemoteOpResult } from '@/types'
import { api } from '@/lib/api'
import { Events, on } from '@/lib/tauri'

interface GitStore {
  status: GitStatus | null
  branches: GitBranch[]
  log: GitCommit[]
  logHasMore: boolean
  worktrees: GitWorktree[]
  remotes: { name: string; url: string }[]
  defaultBranch: string | null
  commitSummary: string
  commitBody: string
  amend: boolean
  signoff: boolean
  refreshing: boolean
  committing: boolean
  syncing: boolean
  generating: boolean
  loadingLog: boolean
  error: string | null
  lastRemoteOutput: string | null
  refresh: () => Promise<void>
  loadBranches: () => Promise<void>
  loadLog: (reset?: boolean) => Promise<void>
  loadWorktrees: () => Promise<void>
  loadRemotes: () => Promise<void>
  setCommitMessage: (summary: string, body?: string) => void
  setAmend: (v: boolean) => void
  setSignoff: (v: boolean) => void
  stage: (paths: string[]) => Promise<void>
  unstage: (paths: string[]) => Promise<void>
  stageAll: () => Promise<void>
  unstageAll: () => Promise<void>
  discard: (paths: string[]) => Promise<void>
  commit: (opts?: { push?: boolean; all?: boolean }) => Promise<string | null>
  push: (opts?: { force?: boolean; setUpstream?: boolean }) => Promise<GitRemoteOpResult | null>
  pull: (rebase?: boolean) => Promise<GitRemoteOpResult | null>
  fetch: () => Promise<GitRemoteOpResult | null>
  checkout: (ref: string, create?: boolean) => Promise<void>
  createBranch: (name: string, checkout?: boolean) => Promise<void>
  deleteBranch: (name: string, force?: boolean) => Promise<void>
  stash: (message?: string) => Promise<void>
  stashPop: () => Promise<void>
  worktreeAdd: (path: string, branch: string, createBranch?: boolean) => Promise<void>
  worktreeRemove: (path: string, force?: boolean) => Promise<void>
  init: () => Promise<void>
}

const LOG_PAGE = 50
let refreshTimer: number | null = null
let refreshPromise: Promise<void> | null = null

function message(e: unknown): string { return (e as { message?: string })?.message ?? String(e) }

export const useGit = create<GitStore>((set, get) => ({
  status: null, branches: [], log: [], logHasMore: true, worktrees: [], remotes: [], defaultBranch: null,
  commitSummary: '', commitBody: '', amend: false, signoff: false,
  refreshing: false, committing: false, syncing: false, generating: false, loadingLog: false, error: null, lastRemoteOutput: null,

  refresh: () => {
    // Debounced + coalesced: many fs/git events in a burst produce one status call.
    if (refreshPromise) return refreshPromise
    refreshPromise = new Promise<void>((resolve) => {
      if (refreshTimer) window.clearTimeout(refreshTimer)
      refreshTimer = window.setTimeout(async () => {
        refreshTimer = null
        set({ refreshing: true })
        try {
          const status = await api.git.status()
          set({ status, error: null })
          if (status.isRepo) {
            get().loadBranches().catch(() => {})
          } else {
            set({ branches: [], log: [], worktrees: [] })
          }
        } catch (e) {
          set({ status: null, error: message(e) })
        } finally {
          set({ refreshing: false })
          refreshPromise = null
          resolve()
        }
      }, 120)
    })
    return refreshPromise
  },
  loadBranches: async () => {
    try {
      const [branches, defaultBranch] = await Promise.all([api.git.branches(), api.git.defaultBranch().catch(() => null)])
      set({ branches, defaultBranch })
    } catch { /* not a repo */ }
  },
  loadLog: async (reset = false) => {
    if (get().loadingLog) return
    set({ loadingLog: true })
    try {
      const skip = reset ? 0 : get().log.length
      const page = await api.git.log({ limit: LOG_PAGE, skip })
      set({ log: reset ? page : [...get().log, ...page], logHasMore: page.length === LOG_PAGE })
    } catch { set({ logHasMore: false }) } finally { set({ loadingLog: false }) }
  },
  loadWorktrees: async () => { try { set({ worktrees: await api.git.worktrees() }) } catch { set({ worktrees: [] }) } },
  loadRemotes: async () => { try { set({ remotes: await api.git.remotes() }) } catch { set({ remotes: [] }) } },

  setCommitMessage: (summary, body = '') => set({ commitSummary: summary, commitBody: body }),
  setAmend: (amend) => set({ amend }),
  setSignoff: (signoff) => set({ signoff }),

  stage: async (paths) => { await api.git.stage(paths); await get().refresh() },
  unstage: async (paths) => { await api.git.unstage(paths); await get().refresh() },
  stageAll: async () => { await api.git.stageAll(); await get().refresh() },
  unstageAll: async () => { await api.git.unstageAll(); await get().refresh() },
  discard: async (paths) => { await api.git.discard(paths); await get().refresh() },

  commit: async (opts) => {
    const { commitSummary, commitBody, amend, signoff } = get()
    const msg = commitBody.trim() ? `${commitSummary.trim()}\n\n${commitBody.trim()}` : commitSummary.trim()
    if (!msg && !amend) return null
    set({ committing: true })
    try {
      if (opts?.all) await api.git.stageAll()
      const sha = await api.git.commit(msg, { amend, signoff })
      set({ commitSummary: '', commitBody: '', amend: false })
      await get().refresh()
      get().loadLog(true).catch(() => {})
      if (opts?.push) await get().push()
      return sha
    } finally { set({ committing: false }) }
  },
  push: async (opts) => {
    set({ syncing: true })
    try {
      const st = get().status
      const r = await api.git.push({ force: opts?.force, setUpstream: opts?.setUpstream ?? !st?.upstream })
      set({ lastRemoteOutput: r.output })
      await get().refresh()
      return r
    } finally { set({ syncing: false }) }
  },
  pull: async (rebase = false) => {
    set({ syncing: true })
    try { const r = await api.git.pull(rebase); set({ lastRemoteOutput: r.output }); await get().refresh(); get().loadLog(true).catch(() => {}); return r } finally { set({ syncing: false }) }
  },
  fetch: async () => {
    set({ syncing: true })
    try { const r = await api.git.fetch(); set({ lastRemoteOutput: r.output }); await get().refresh(); return r } finally { set({ syncing: false }) }
  },
  checkout: async (ref, create = false) => { await api.git.checkout(ref, create); await get().refresh(); await get().loadBranches(); get().loadLog(true).catch(() => {}) },
  createBranch: async (name, checkout = true) => { await api.git.createBranch(name, checkout); await get().refresh(); await get().loadBranches() },
  deleteBranch: async (name, force = false) => { await api.git.deleteBranch(name, force); await get().loadBranches() },
  stash: async (msg) => { await api.git.stash(msg); await get().refresh() },
  stashPop: async () => { await api.git.stashPop(); await get().refresh() },
  worktreeAdd: async (path, branch, createBranch = true) => { await api.git.worktreeAdd(path, branch, createBranch); await get().loadWorktrees(); await get().loadBranches() },
  worktreeRemove: async (path, force = false) => { await api.git.worktreeRemove(path, force); await get().loadWorktrees() },
  init: async () => { await api.git.init(); await get().refresh() },
}))

let subscribed = false
/** Subscribe to backend events once (idempotent). Called by the SCM module and safe to call from anywhere. */
export function initGitStore(): void {
  if (subscribed) return
  subscribed = true
  useGit.getState().refresh().catch(() => {})
  on(Events.gitChanged, () => { const g = useGit.getState(); g.refresh().catch(() => {}) })
  on(Events.workspaceChanged, () => { useGit.setState({ log: [], logHasMore: true, worktrees: [], commitSummary: '', commitBody: '' }); const g = useGit.getState(); g.refresh().catch(() => {}); g.loadWorktrees().catch(() => {}); g.loadRemotes().catch(() => {}) })
  on(Events.fsChanged, () => { useGit.getState().refresh().catch(() => {}) })
}

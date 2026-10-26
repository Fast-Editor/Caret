import type { GitFileChange, GitFileStatus, GitStatus } from '@/types'

export const STATUS_LETTER: Record<GitFileStatus, string> = { modified: 'M', added: 'A', deleted: 'D', renamed: 'R', copied: 'C', untracked: 'U', ignored: 'I', conflicted: '!', typechange: 'T' }
export const STATUS_CLASS: Record<GitFileStatus, string> = {
  modified: 'text-warning', added: 'text-success', deleted: 'text-danger', renamed: 'text-info', copied: 'text-info', untracked: 'text-success', ignored: 'text-faint', conflicted: 'text-danger', typechange: 'text-warning',
}
export const STATUS_TITLE: Record<GitFileStatus, string> = { modified: 'Modified', added: 'Added', deleted: 'Deleted', renamed: 'Renamed', copied: 'Copied', untracked: 'Untracked', ignored: 'Ignored', conflicted: 'Conflict', typechange: 'Type changed' }

export function splitPath(path: string): { dir: string; base: string } {
  const i = path.lastIndexOf('/')
  return i === -1 ? { dir: '', base: path } : { dir: path.slice(0, i), base: path.slice(i + 1) }
}

export type SectionId = 'conflicted' | 'staged' | 'unstaged' | 'untracked'
export interface ChangeSection { id: SectionId; title: string; items: GitFileChange[] }

/** Group a GitStatus into the four SCM sections (empty sections omitted, conflicts first). */
export function groupChanges(status: GitStatus | null): ChangeSection[] {
  if (!status || !status.isRepo) return []
  const sections: ChangeSection[] = [
    { id: 'conflicted', title: 'Merge Changes', items: status.conflicted },
    { id: 'staged', title: 'Staged Changes', items: status.staged },
    { id: 'unstaged', title: 'Changes', items: status.unstaged },
    { id: 'untracked', title: 'Untracked', items: status.untracked },
  ]
  return sections.filter((s) => s.items.length > 0).map((s) => ({ ...s, items: [...s.items].sort((a, b) => a.path.localeCompare(b.path)) }))
}

export function countChanges(status: GitStatus | null): { total: number; staged: number; unstaged: number; untracked: number; conflicted: number } {
  if (!status || !status.isRepo) return { total: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0 }
  const staged = status.staged.length, unstaged = status.unstaged.length, untracked = status.untracked.length, conflicted = status.conflicted.length
  return { total: staged + unstaged + untracked + conflicted, staged, unstaged, untracked, conflicted }
}

export function canCommit(status: GitStatus | null, summary: string, amend: boolean, commitAll: boolean): boolean {
  if (!status?.isRepo) return false
  if (!summary.trim() && !amend) return false
  const c = countChanges(status)
  if (c.conflicted > 0) return false
  if (amend) return true
  return commitAll ? c.total > 0 : c.staged > 0
}

export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) return '?'
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase()
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase()
}

/** Deterministic hue for avatars. */
export function hueFor(s: string): number { let h = 0; for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0; return h % 360 }

/** Sort branches: current first, then local by recency/name, then remotes. */
export function sortBranches<T extends { name: string; isCurrent: boolean; isRemote: boolean; lastCommitTime?: number }>(branches: T[]): T[] {
  return [...branches].sort((a, b) => {
    if (a.isCurrent !== b.isCurrent) return a.isCurrent ? -1 : 1
    if (a.isRemote !== b.isRemote) return a.isRemote ? 1 : -1
    const ta = a.lastCommitTime ?? 0, tb = b.lastCommitTime ?? 0
    if (ta !== tb) return tb - ta
    return a.name.localeCompare(b.name)
  })
}

export function isValidBranchName(name: string): boolean {
  if (!name || name.length > 200) return false
  if (/[\s~^:?*[\\]|\.\.|@\{|^\/|\/$|\.lock$|^\.|\/\.|\/\/|^-/.test(name)) return false
  return name !== '@' && !name.endsWith('.')
}

export function syncLabel(status: GitStatus | null): { label: string; title: string; action: 'publish' | 'pull' | 'push' | 'sync' | 'fetch' } {
  if (!status?.isRepo) return { label: '', title: '', action: 'fetch' }
  if (!status.upstream) return { label: 'Publish', title: 'Publish branch to remote', action: 'publish' }
  if (status.ahead && status.behind) return { label: `${status.behind}↓ ${status.ahead}↑`, title: 'Pull then push', action: 'sync' }
  if (status.behind) return { label: `${status.behind}↓`, title: 'Pull', action: 'pull' }
  if (status.ahead) return { label: `${status.ahead}↑`, title: 'Push', action: 'push' }
  return { label: 'Fetch', title: 'Fetch from remote', action: 'fetch' }
}

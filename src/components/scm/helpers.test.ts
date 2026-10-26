import { describe, it, expect } from 'vitest'
import type { GitStatus, GitFileChange } from '@/types'
import { groupChanges, countChanges, canCommit, splitPath, sortBranches, isValidBranchName, syncLabel, initials } from './helpers'

const ch = (path: string, status: GitFileChange['status'], staged = false): GitFileChange => ({ path, status, staged })
const base = (over: Partial<GitStatus> = {}): GitStatus => ({ isRepo: true, branch: 'main', upstream: 'origin/main', ahead: 0, behind: 0, staged: [], unstaged: [], untracked: [], conflicted: [], isDetached: false, headSha: 'abc', state: 'clean', stashCount: 0, ...over })

describe('groupChanges', () => {
  it('omits empty sections, orders conflicts first and sorts paths', () => {
    const s = base({ staged: [ch('b.ts', 'modified', true), ch('a.ts', 'added', true)], untracked: [ch('z.md', 'untracked')], conflicted: [ch('c.rs', 'conflicted')] })
    const g = groupChanges(s)
    expect(g.map((x) => x.id)).toEqual(['conflicted', 'staged', 'untracked'])
    expect(g[1].items.map((i) => i.path)).toEqual(['a.ts', 'b.ts'])
  })
  it('returns nothing for non-repos or null', () => {
    expect(groupChanges(null)).toEqual([])
    expect(groupChanges(base({ isRepo: false }))).toEqual([])
  })
})

describe('countChanges / canCommit', () => {
  it('counts per section and total', () => {
    const c = countChanges(base({ staged: [ch('a', 'modified', true)], unstaged: [ch('b', 'modified'), ch('c', 'deleted')], untracked: [ch('d', 'untracked')] }))
    expect(c).toEqual({ total: 4, staged: 1, unstaged: 2, untracked: 1, conflicted: 0 })
  })
  it('requires a message and staged files unless committing all or amending', () => {
    const staged = base({ staged: [ch('a', 'modified', true)] })
    const unstagedOnly = base({ unstaged: [ch('a', 'modified')] })
    expect(canCommit(staged, 'msg', false, false)).toBe(true)
    expect(canCommit(staged, '  ', false, false)).toBe(false)
    expect(canCommit(unstagedOnly, 'msg', false, false)).toBe(false)
    expect(canCommit(unstagedOnly, 'msg', false, true)).toBe(true)
    expect(canCommit(base(), '', true, false)).toBe(true)
    expect(canCommit(base({ conflicted: [ch('x', 'conflicted')], staged: [ch('a', 'modified', true)] }), 'msg', false, false)).toBe(false)
    expect(canCommit(null, 'msg', false, false)).toBe(false)
  })
})

describe('path and name helpers', () => {
  it('splits paths', () => {
    expect(splitPath('src/a/b.ts')).toEqual({ dir: 'src/a', base: 'b.ts' })
    expect(splitPath('README.md')).toEqual({ dir: '', base: 'README.md' })
  })
  it('derives initials', () => {
    expect(initials('Ada Lovelace')).toBe('AL')
    expect(initials('torvalds')).toBe('TO')
    expect(initials('')).toBe('?')
  })
  it('validates branch names', () => {
    expect(isValidBranchName('feature/x-1')).toBe(true)
    expect(isValidBranchName('bad name')).toBe(false)
    expect(isValidBranchName('a..b')).toBe(false)
    expect(isValidBranchName('-lead')).toBe(false)
    expect(isValidBranchName('x.lock')).toBe(false)
    expect(isValidBranchName('')).toBe(false)
  })
})

describe('sortBranches / syncLabel', () => {
  it('puts current first, locals before remotes, newest first', () => {
    const r = sortBranches([
      { name: 'origin/main', isCurrent: false, isRemote: true, lastCommitTime: 9 },
      { name: 'old', isCurrent: false, isRemote: false, lastCommitTime: 1 },
      { name: 'main', isCurrent: true, isRemote: false, lastCommitTime: 5 },
      { name: 'new', isCurrent: false, isRemote: false, lastCommitTime: 8 },
    ])
    expect(r.map((b) => b.name)).toEqual(['main', 'new', 'old', 'origin/main'])
  })
  it('chooses the sync action from upstream/ahead/behind', () => {
    expect(syncLabel(base({ upstream: null })).action).toBe('publish')
    expect(syncLabel(base({ behind: 2 })).action).toBe('pull')
    expect(syncLabel(base({ ahead: 1 })).action).toBe('push')
    expect(syncLabel(base({ ahead: 1, behind: 1 })).action).toBe('sync')
    expect(syncLabel(base()).action).toBe('fetch')
    expect(syncLabel(null).label).toBe('')
  })
})

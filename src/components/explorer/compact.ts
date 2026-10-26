import type { DirEntry } from '@/types'

export interface CompactChain { label: string; path: string; chain: string[] }

/**
 * VS Code "compact folders": a directory whose only child is another directory is rendered as `a/b/c`.
 * Only collapses through directories whose children are already loaded.
 */
export function compactChain(entry: DirEntry, children: Record<string, DirEntry[] | undefined>, enabled: boolean): CompactChain {
  const chain = [entry.path]
  let label = entry.name
  let cur = entry
  if (!enabled) return { label, path: entry.path, chain }
  for (let guard = 0; guard < 32; guard++) {
    const kids = children[cur.path]
    if (!kids || kids.length !== 1 || kids[0].kind !== 'dir') break
    cur = kids[0]
    chain.push(cur.path)
    label += `/${cur.name}`
  }
  return { label, path: cur.path, chain }
}

export interface TreeRow { entry: DirEntry; depth: number; expanded: boolean; loading: boolean; label: string; nodePath: string; chain: string[]; error?: string }

export interface FlattenInput {
  root: string
  children: Record<string, DirEntry[] | undefined>
  expanded: Record<string, true | undefined>
  loading: Record<string, boolean | undefined>
  errors?: Record<string, string | undefined>
  showHidden: boolean
  filter: string
  compact: boolean
  globalIgnore?: string[]
}

function matchesFilter(name: string, filter: string): boolean { return filter === '' || name.toLowerCase().includes(filter.toLowerCase()) }

/** Flatten the loaded tree into visible rows. With a filter, keeps matching entries and the dirs that contain them (loaded subtrees only). */
export function flattenTree(input: FlattenInput): TreeRow[] {
  const rows: TreeRow[] = []
  const filter = input.filter.trim()
  const ignored = new Set(input.globalIgnore ?? [])
  const subtreeMatches = new Map<string, boolean>()
  const anyMatch = (dir: string): boolean => {
    if (subtreeMatches.has(dir)) return subtreeMatches.get(dir)!
    const kids = input.children[dir] ?? []
    let hit = false
    for (const k of kids) {
      if (!input.showHidden && k.isHidden) continue
      if (matchesFilter(k.name, filter) || (k.kind === 'dir' && anyMatch(k.path))) { hit = true; break }
    }
    subtreeMatches.set(dir, hit)
    return hit
  }
  const walk = (dir: string, depth: number) => {
    const kids = input.children[dir]
    if (!kids) return
    for (const k of kids) {
      if (!input.showHidden && k.isHidden) continue
      if (ignored.has(k.name)) continue
      const isDir = k.kind === 'dir'
      if (filter && !matchesFilter(k.name, filter) && !(isDir && anyMatch(k.path))) continue
      const c = isDir ? compactChain(k, input.children, input.compact && !filter) : { label: k.name, path: k.path, chain: [k.path] }
      const expanded = isDir && !!input.expanded[c.path] || (!!filter && isDir)
      rows.push({ entry: k, depth, expanded, loading: !!input.loading[c.path], label: c.label, nodePath: c.path, chain: c.chain, error: input.errors?.[c.path] })
      if (isDir && expanded) walk(c.path, depth + 1)
    }
  }
  walk(input.root, 0)
  return rows
}

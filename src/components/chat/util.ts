import type { ChatSessionSummary, ContextItem, ContextItemKind, ToolCallRecord, Usage } from '@/types'

export function baseName(p: string): string { return p.split('/').filter(Boolean).pop() ?? p }

export function relToRoot(p: string, root?: string | null): string {
  if (root && p.startsWith(root)) return p.slice(root.length).replace(/^\//, '') || '.'
  return p
}

export function fmtTokens(n: number | undefined): string {
  if (!n) return '0'
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1000) return `${(n / 1000).toFixed(n >= 10_000 ? 0 : 1)}k`
  return String(n)
}
export function fmtCost(usd: number | undefined): string {
  if (usd === undefined || usd === null) return ''
  if (usd < 0.01) return `$${usd.toFixed(4)}`
  return `$${usd.toFixed(2)}`
}
export function fmtDuration(ms: number | undefined): string {
  if (!ms && ms !== 0) return ''
  if (ms < 1000) return `${ms}ms`
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 1 : 0)}s`
  const m = Math.floor(ms / 60_000); const s = Math.round((ms % 60_000) / 1000)
  return `${m}m ${s}s`
}
export function fmtRelative(ts: number): string {
  const d = Date.now() - ts
  if (d < 60_000) return 'just now'
  if (d < 3_600_000) return `${Math.floor(d / 60_000)}m ago`
  if (d < 86_400_000) return `${Math.floor(d / 3_600_000)}h ago`
  if (d < 7 * 86_400_000) return `${Math.floor(d / 86_400_000)}d ago`
  return new Date(ts).toLocaleDateString()
}
export function dayGroup(ts: number, nowMs = Date.now()): 'Today' | 'Yesterday' | 'This week' | 'Earlier' {
  const now = new Date(nowMs)
  const startToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  if (ts >= startToday) return 'Today'
  if (ts >= startToday - 86_400_000) return 'Yesterday'
  if (ts >= startToday - 6 * 86_400_000) return 'This week'
  return 'Earlier'
}

export function emptyUsage(): Usage { return { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 } }

/** Human label for a context pill. */
export function contextLabel(item: ContextItem, root?: string | null): string {
  switch (item.kind) {
    case 'file': case 'active-file': { const name = baseName(item.path ?? item.label); return item.range ? `${name}:${item.range.startLine}-${item.range.endLine}` : name }
    case 'selection': case 'code': case 'symbol': { const name = baseName(item.path ?? item.label); return item.range ? `${name}:${item.range.startLine}${item.range.endLine !== item.range.startLine ? `-${item.range.endLine}` : ''}` : name }
    case 'folder': return (relToRoot(item.path ?? item.label, root) || '.') + '/'
    case 'codebase': return item.query ? `Codebase: ${item.query.slice(0, 30)}` : 'Codebase'
    case 'git-diff': return 'Git: working changes'
    case 'git-staged': return 'Git: staged'
    case 'git-commit': return `Commit ${item.sha?.slice(0, 7) ?? ''}`
    case 'git-branch-diff': return `Diff vs ${item.base ?? 'main'}`
    case 'web': return item.query ? `Web: ${item.query.slice(0, 30)}` : 'Web'
    case 'docs': return `Docs: ${item.label}`
    case 'terminal': return `Terminal: ${item.label}`
    case 'lint': return 'Lint errors'
    case 'recent-changes': return 'Recent changes'
    case 'past-chat': return `Chat: ${item.label}`
    case 'rule': return `Rule: ${item.label}`
    case 'link': return item.url ? item.url.replace(/^https?:\/\//, '').slice(0, 40) : 'Link'
    case 'image': return item.label || 'Image'
    case 'note': return `Note: ${item.label}`
    case 'mcp-resource': return `${item.label}`
    case 'open-files': return 'Open files'
    case 'plan': return `Plan: ${item.label}`
    case 'skill': return `Skill: ${item.label}`
    default: return item.label
  }
}

export const KIND_ORDER: ContextItemKind[] = ['selection', 'active-file', 'file', 'folder', 'code', 'symbol', 'codebase', 'git-diff', 'git-staged', 'git-branch-diff', 'git-commit', 'docs', 'web', 'link', 'terminal', 'lint', 'recent-changes', 'past-chat', 'rule', 'skill', 'note', 'mcp-resource', 'plan', 'open-files', 'image']

export function sortContext(items: ContextItem[]): ContextItem[] {
  return [...items].sort((a, b) => KIND_ORDER.indexOf(a.kind) - KIND_ORDER.indexOf(b.kind))
}

export function toolTitle(call: ToolCallRecord): string {
  const i = (call.input ?? {}) as Record<string, unknown>
  const str = (k: string) => (typeof i[k] === 'string' ? (i[k] as string) : undefined)
  const file = str('path') ?? str('target_file') ?? str('file') ?? str('file_path')
  switch (call.name) {
    case 'read_file': return file ? `Read ${baseName(file)}${i.start_line ? ` (${i.start_line}-${i.end_line ?? ''})` : ''}` : 'Read file'
    case 'list_dir': return `Listed ${str('path') ?? str('relative_workspace_path') ?? '.'}`
    case 'grep_search': return `Grep "${str('query') ?? str('pattern') ?? ''}"`
    case 'file_search': case 'glob_file_search': return `Searched files "${str('query') ?? str('glob_pattern') ?? ''}"`
    case 'codebase_search': return `Searched codebase "${(str('query') ?? '').slice(0, 60)}"`
    case 'edit_file': case 'search_replace': case 'write_file': case 'apply_patch': return `${call.name === 'write_file' ? 'Wrote' : 'Edited'} ${file ? baseName(file) : 'file'}`
    case 'delete_file': return `Deleted ${file ? baseName(file) : 'file'}`
    case 'run_terminal_cmd': case 'run_command': case 'bash': return `Ran \`${(call.command ?? str('command') ?? '').slice(0, 80)}\``
    case 'read_lints': return 'Read lints'
    case 'web_search': return `Searched web "${(str('query') ?? '').slice(0, 50)}"`
    case 'fetch_url': case 'web_fetch': return `Fetched ${(str('url') ?? '').replace(/^https?:\/\//, '').slice(0, 50)}`
    case 'todo_write': return 'Updated todos'
    case 'update_memory': return 'Saved memory'
    case 'fetch_rules': return 'Fetched rules'
    case 'create_plan': case 'edit_plan': return call.name === 'create_plan' ? 'Created plan' : 'Edited plan'
    case 'ask_question': return 'Asked a question'
    case 'task': return `Subagent: ${(str('description') ?? str('prompt') ?? '').slice(0, 50)}`
    case 'skill': return `Loaded skill ${str('name') ?? ''}`
    case 'search_docs': return `Searched docs "${(str('query') ?? '').slice(0, 50)}"`
    case 'read_terminal': return 'Read terminal'
    case 'browser_navigate': return `Browser → ${(str('url') ?? '').slice(0, 50)}`
    case 'browser_snapshot': return 'Browser snapshot'
    case 'browser_click': return `Browser click ${str('ref') ?? ''}`
    case 'browser_type': return `Browser type`
    default: return call.mcpServerId ? `${call.displayName || call.name} (MCP)` : call.displayName || call.name
  }
}

export type ToolGroup = 'read' | 'search' | 'edit' | 'shell' | 'web' | 'plan' | 'memory' | 'mcp' | 'browser' | 'agent' | 'other'
export function toolGroup(call: ToolCallRecord): ToolGroup {
  if (call.mcpServerId || call.name.startsWith('mcp_')) return 'mcp'
  if (/^(read_file|list_dir|read_lints|read_terminal)$/.test(call.name)) return 'read'
  if (/search|glob/.test(call.name)) return 'search'
  if (/^(edit_file|search_replace|write_file|delete_file|apply_patch|reapply)$/.test(call.name)) return 'edit'
  if (/terminal|command|bash|shell/.test(call.name)) return 'shell'
  if (/web|fetch_url/.test(call.name)) return 'web'
  if (/plan|todo/.test(call.name)) return 'plan'
  if (/memory|rules|skill/.test(call.name)) return 'memory'
  if (/^browser_/.test(call.name)) return 'browser'
  if (/^(task|ask_question)$/.test(call.name)) return 'agent'
  return 'other'
}

export function isMac(): boolean { return typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform) }
export const MOD = isMac() ? '⌘' : 'Ctrl+'

export function uid(prefix = 'c'): string { return `${prefix}_${Math.random().toString(36).slice(2, 10)}${Date.now().toString(36)}` }

export function severityRank(s: string): number { return ({ critical: 0, high: 1, medium: 2, low: 3, info: 4 } as Record<string, number>)[s] ?? 5 }

// ───────────── thread lifecycle grouping (docs/THREADS.md) ─────────────
export type ThreadSectionId = 'working' | 'pinned' | 'today' | 'yesterday' | 'week' | 'earlier' | 'snoozed' | 'settled' | 'archived'
export interface ThreadSection {
  id: ThreadSectionId
  title: string
  sessions: ChatSessionSummary[]
  /** Snoozed and Settled start collapsed. */
  defaultCollapsed: boolean
  /** Effective fold state after applying `GroupOpts.folds`. */
  collapsed: boolean
}
export interface GroupOpts {
  /** Include the Archived section (the list's "archived" toggle). */
  showArchived?: boolean
  includeSubagents?: boolean
  /** Clock for day buckets and snooze expiry (tests). */
  now?: number
  /** Persisted fold state keyed by section id or `project:<root>` (ui store). */
  folds?: Record<string, boolean>
  /** Busy predicate for the Working section (`isSessionBusy` from the chat store). */
  isBusy?: (status: string | undefined) => boolean
  /** Case-insensitive filter over title and last message preview. */
  query?: string
}
type Cmp = (a: ChatSessionSummary, b: ChatSessionSummary) => number
const SECTION_ORDER: ThreadSectionId[] = ['working', 'pinned', 'today', 'yesterday', 'week', 'earlier', 'snoozed', 'settled', 'archived']
const SECTION_TITLE: Record<ThreadSectionId, string> = { working: 'Working', pinned: 'Pinned', today: 'Today', yesterday: 'Yesterday', week: 'This week', earlier: 'Earlier', snoozed: 'Snoozed', settled: 'Settled', archived: 'Archived' }
const DAY_SECTION: Record<ReturnType<typeof dayGroup>, ThreadSectionId> = { Today: 'today', Yesterday: 'yesterday', 'This week': 'week', Earlier: 'earlier' }
const COLLAPSED_BY_DEFAULT = new Set<ThreadSectionId>(['snoozed', 'settled'])

export const defaultIsBusy = (status: string | undefined): boolean => status === 'running' || status === 'awaiting-approval' || status === 'awaiting-input' || status === 'summarizing'

export function isSnoozed(s: Pick<ChatSessionSummary, 'snoozedUntil'>, now = Date.now()): boolean { return typeof s.snoozedUntil === 'number' && s.snoozedUntil > now }

/** Lifecycle section of one thread. Precedence: archived, working, snoozed, settled, pinned, then the day bucket. */
export function threadSection(s: ChatSessionSummary, opts: GroupOpts = {}): ThreadSectionId {
  const now = opts.now ?? Date.now()
  const busy = opts.isBusy ?? defaultIsBusy
  if (s.archived) return 'archived'
  if (busy(s.status)) return 'working'
  if (isSnoozed(s, now)) return 'snoozed'
  if (s.settled) return 'settled'
  if (s.pinned) return 'pinned'
  return DAY_SECTION[dayGroup(s.updatedAt, now)]
}

const matchesQuery = (s: ChatSessionSummary, q: string): boolean => !q || s.title.toLowerCase().includes(q) || (s.lastMessagePreview ?? '').toLowerCase().includes(q)
const workingRank = (s: ChatSessionSummary): number => (s.status === 'awaiting-approval' || s.status === 'awaiting-input' ? 0 : 1)
const byUpdated: Cmp = (a, b) => b.updatedAt - a.updatedAt
const SORTERS: Partial<Record<ThreadSectionId, Cmp>> = {
  working: (a, b) => workingRank(a) - workingRank(b) || byUpdated(a, b),
  pinned: (a, b) => (b.pinnedAt ?? 0) - (a.pinnedAt ?? 0) || byUpdated(a, b),
  snoozed: (a, b) => (a.snoozedUntil ?? 0) - (b.snoozedUntil ?? 0) || byUpdated(a, b),
  archived: (a, b) => (b.archivedAt ?? b.updatedAt) - (a.archivedAt ?? a.updatedAt) || byUpdated(a, b),
}

/** Ordered, foldable sections for the history popover and the Agents sidebar. Empty sections are omitted. */
export function groupSessions(sessions: ChatSessionSummary[], opts: GroupOpts = {}): ThreadSection[] {
  const q = (opts.query ?? '').trim().toLowerCase()
  const buckets = new Map<ThreadSectionId, ChatSessionSummary[]>()
  for (const s of sessions) {
    if (s.isSubagent && !opts.includeSubagents) continue
    if (!matchesQuery(s, q)) continue
    const id = threadSection(s, opts)
    if (id === 'archived' && !opts.showArchived) continue
    const list = buckets.get(id) ?? []
    list.push(s)
    buckets.set(id, list)
  }
  return SECTION_ORDER.filter((id) => buckets.has(id)).map((id) => {
    const defaultCollapsed = COLLAPSED_BY_DEFAULT.has(id)
    return { id, title: SECTION_TITLE[id], sessions: [...(buckets.get(id) ?? [])].sort(SORTERS[id] ?? byUpdated), defaultCollapsed, collapsed: opts.folds?.[id] ?? defaultCollapsed }
  })
}

export interface ProjectGroup {
  /** Workspace root ('' for sessions without a folder). Fold key: `project:<root>`. */
  root: string
  title: string
  current: boolean
  count: number
  collapsed: boolean
  sections: ThreadSection[]
}
export const projectFoldKey = (root: string): string => `project:${root}`

/** One group per workspace root (current workspace first, then most recent activity), each with its lifecycle sections. */
export function groupSessionsByProject(sessions: ChatSessionSummary[], currentRoot: string | null | undefined, opts: GroupOpts = {}): ProjectGroup[] {
  const byRoot = new Map<string, ChatSessionSummary[]>()
  for (const s of sessions) { const r = s.workspaceRoot ?? ''; const l = byRoot.get(r) ?? []; l.push(s); byRoot.set(r, l) }
  const groups: ProjectGroup[] = []
  for (const [root, list] of byRoot) {
    const sections = groupSessions(list, opts)
    const count = sections.reduce((n, sec) => n + sec.sessions.length, 0)
    if (!count) continue
    groups.push({ root, title: root ? baseName(root) : 'No folder', current: !!currentRoot && root === currentRoot, count, collapsed: opts.folds?.[projectFoldKey(root)] ?? false, sections })
  }
  const latest = (g: ProjectGroup): number => Math.max(...g.sections.flatMap((sec) => sec.sessions.map((s) => s.updatedAt)))
  return groups.sort((a, b) => Number(b.current) - Number(a.current) || latest(b) - latest(a) || a.title.localeCompare(b.title))
}

/** Thread ids in display order, skipping collapsed sections and groups (next/previous thread commands). */
export function flattenThreadOrder(input: ThreadSection[] | ProjectGroup[]): string[] {
  const out: string[] = []
  for (const item of input) {
    if (item.collapsed) continue
    if ('sections' in item) { for (const sec of item.sections) if (!sec.collapsed) out.push(...sec.sessions.map((s) => s.id)) }
    else out.push(...item.sessions.map((s) => s.id))
  }
  return out
}

/** Snooze presets: 1 hour, tonight 18:00 (tomorrow 18:00 once that has passed), tomorrow 09:00, next Monday 09:00. */
export function snoozePresets(now = Date.now()): { id: string; label: string; until: number }[] {
  const d = new Date(now)
  const at = (dayOffset: number, hour: number): number => new Date(d.getFullYear(), d.getMonth(), d.getDate() + dayOffset, hour, 0, 0, 0).getTime()
  const tonightIsToday = at(0, 18) > now
  const daysToMonday = (8 - d.getDay()) % 7 || 7
  return [
    { id: 'hour', label: '1 hour', until: now + 3_600_000 },
    { id: 'tonight', label: tonightIsToday ? 'Tonight 18:00' : 'Tomorrow 18:00', until: tonightIsToday ? at(0, 18) : at(1, 18) },
    { id: 'tomorrow', label: 'Tomorrow 09:00', until: at(1, 9) },
    { id: 'week', label: 'Next week (Mon 09:00)', until: at(daysToMonday, 9) },
  ]
}

/** Wake-time label for snoozed rows: "in 45m", "Today 18:00", "Tomorrow 09:00", "Mon 09:00", else a date. */
export function fmtWake(until: number, now = Date.now()): string {
  const diff = until - now
  if (diff <= 0) return 'now'
  if (diff < 3_600_000) return `in ${Math.max(1, Math.round(diff / 60_000))}m`
  const t = new Date(until)
  const n = new Date(now)
  const hm = `${String(t.getHours()).padStart(2, '0')}:${String(t.getMinutes()).padStart(2, '0')}`
  const startToday = new Date(n.getFullYear(), n.getMonth(), n.getDate()).getTime()
  const dayIdx = Math.round((new Date(t.getFullYear(), t.getMonth(), t.getDate()).getTime() - startToday) / 86_400_000)
  if (dayIdx === 0) return `Today ${hm}`
  if (dayIdx === 1) return `Tomorrow ${hm}`
  if (dayIdx < 7) return `${['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'][t.getDay()]} ${hm}`
  return `${t.toLocaleDateString()} ${hm}`
}

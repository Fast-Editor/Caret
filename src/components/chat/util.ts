import type { ContextItem, ContextItemKind, ToolCallRecord, Usage } from '@/types'

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
export function dayGroup(ts: number): 'Today' | 'Yesterday' | 'This week' | 'Earlier' {
  const now = new Date(); const d = new Date(ts)
  const startToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  if (ts >= startToday) return 'Today'
  if (ts >= startToday - 86_400_000) return 'Yesterday'
  if (ts >= startToday - 6 * 86_400_000) return 'This week'
  void d
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

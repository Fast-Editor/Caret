/** Group pending changes by the intent that produced them (tool call / turn), with ancillary file kinds split out. */
import type { ChatMessage, FileChangeSummary, ReviewGroup, ToolCallRecord } from '@/types'
import { fileKind, COLLAPSED_KINDS, KIND_LABEL, KIND_RANK, type FileKind } from './fileKind'
import { compareByKindThenPath } from './ordering'

export interface GroupInput { pendingChanges: FileChangeSummary[]; messages: ChatMessage[] }

function toolCalls(messages: ChatMessage[]): Map<string, { call: ToolCallRecord; messageIndex: number; partIndex: number; turnId?: string }> {
  const m = new Map<string, { call: ToolCallRecord; messageIndex: number; partIndex: number; turnId?: string }>()
  messages.forEach((msg, mi) => msg.parts.forEach((p, pi) => { if (p.type === 'tool') m.set(p.call.id, { call: p.call, messageIndex: mi, partIndex: pi, turnId: msg.turnId }) }))
  return m
}

function firstSentence(text: string): string {
  const t = text.replace(/^\*\[[^\]]+\][^\n]*\n*/g, '').replace(/[#*`_>]/g, '').trim()
  const s = t.split(/(?<=[.!?])\s|\n/)[0] ?? ''
  return s.length > 90 ? s.slice(0, 87).trimEnd() + '…' : s
}

/** Title for an intent group: the tool call's summary, else the assistant sentence preceding it, else the tool label. */
export function intentTitle(call: ToolCallRecord | undefined, msg: ChatMessage | undefined, partIndex: number): string {
  if (call?.summary) return call.summary
  if (msg) {
    for (let i = partIndex - 1; i >= 0; i--) { const p = msg.parts[i]; if (p.type === 'text' && p.text.trim()) { const s = firstSentence(p.text); if (s) return s } }
  }
  if (call) {
    const input = call.input as { path?: string; explanation?: string } | undefined
    if (input?.explanation) return input.explanation
    if (input?.path) return `${call.displayName || call.name}: ${input.path.split('/').pop()}`
    return call.displayName || call.name
  }
  return 'Changes'
}

export function computeGroups(input: GroupInput): ReviewGroup[] {
  const pending = input.pendingChanges.filter((c) => c.status === 'pending')
  if (pending.length === 0) return []
  const calls = toolCalls(input.messages)
  const intent = new Map<string, { title: string; order: number; ids: string[]; turnId?: string; toolCallId?: string }>()
  const kindBuckets = new Map<FileKind, string[]>()
  for (const c of pending) {
    const k = fileKind(c.path)
    if (COLLAPSED_KINDS.has(k)) { (kindBuckets.get(k) ?? kindBuckets.set(k, []).get(k)!).push(c.changeId); continue }
    const key = c.toolCallId ?? (c.turnId ? `turn:${c.turnId}` : 'other')
    let g = intent.get(key)
    if (!g) {
      const tc = c.toolCallId ? calls.get(c.toolCallId) : undefined
      const msg = tc ? input.messages[tc.messageIndex] : input.messages.find((m) => m.turnId === c.turnId && m.role === 'assistant')
      const title = c.toolCallId ? intentTitle(tc?.call, msg, tc?.partIndex ?? 0) : c.turnId ? (msg ? firstSentence(msg.parts.filter((p) => p.type === 'text').map((p) => (p as { text: string }).text).join(' ')) || 'Changes' : 'Changes') : 'Changes'
      const order = tc ? tc.messageIndex * 1000 + tc.partIndex : input.messages.findIndex((m) => m.turnId === c.turnId) * 1000 + 999
      g = { title, order: order < 0 ? Number.MAX_SAFE_INTEGER : order, ids: [], turnId: c.turnId ?? tc?.turnId, toolCallId: c.toolCallId }
      intent.set(key, g)
    }
    g.ids.push(c.changeId)
  }
  // merge intent groups that share a title (same step across several tool calls)
  const byTitle = new Map<string, { title: string; order: number; ids: string[]; turnId?: string; toolCallId?: string }>()
  for (const g of intent.values()) {
    const ex = byTitle.get(g.title)
    if (ex) { ex.ids.push(...g.ids); ex.order = Math.min(ex.order, g.order) } else byTitle.set(g.title, { ...g, ids: [...g.ids] })
  }
  const pathOf = new Map(pending.map((c) => [c.changeId, c.path] as const))
  const sortIds = (ids: string[]) => [...ids].sort((a, b) => compareByKindThenPath(pathOf.get(a) ?? '', pathOf.get(b) ?? ''))
  const groups: ReviewGroup[] = [...byTitle.values()].sort((a, b) => a.order - b.order).map((g, i) => ({ id: `intent:${g.toolCallId ?? g.turnId ?? i}`, title: g.title, kind: g.toolCallId ? 'tool-call' : g.turnId ? 'intent' : 'other', turnId: g.turnId, toolCallId: g.toolCallId, changeIds: sortIds(g.ids), collapsedByDefault: false, order: i }))
  const kinds = [...kindBuckets.entries()].sort((a, b) => KIND_RANK[a[0]] - KIND_RANK[b[0]])
  for (const [k, ids] of kinds) groups.push({ id: `kind:${k}`, title: KIND_LABEL[k], kind: 'file-kind', changeIds: sortIds(ids), collapsedByDefault: true, order: groups.length })
  return groups
}

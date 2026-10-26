import { useEffect, useMemo, useRef, useState } from 'react'
import { Command } from 'cmdk'
import { ChevronLeft, Plus } from 'lucide-react'
import type { ContextItem, ContextItemKind } from '@/types'
import { api } from '@/lib/api'
import { useChat } from '@/store/chat'
import { useWorkspace } from '@/store/workspace'
import { kindIcon } from './ContextPills'
import { baseName, relToRoot, uid } from '../util'
import { runCommand, hasCommand } from '@/lib/commands'

interface Category { id: string; label: string; kind: ContextItemKind; hint?: string; immediate?: (q: string) => ContextItem | null }

const CATEGORIES: Category[] = [
  { id: 'files', label: 'Files & Folders', kind: 'file' },
  { id: 'code', label: 'Code', kind: 'symbol', hint: 'symbols in open files' },
  { id: 'codebase', label: 'Codebase', kind: 'codebase', hint: 'semantic search over the index', immediate: (q) => ({ id: uid('cb'), kind: 'codebase', label: 'Codebase', query: q || undefined }) },
  { id: 'git', label: 'Git', kind: 'git-diff' },
  { id: 'web', label: 'Web', kind: 'web', hint: 'search the web', immediate: (q) => ({ id: uid('web'), kind: 'web', label: 'Web', query: q || undefined }) },
  { id: 'docs', label: 'Docs', kind: 'docs' },
  { id: 'terminals', label: 'Terminals', kind: 'terminal' },
  { id: 'lint', label: 'Lint errors', kind: 'lint', immediate: () => ({ id: uid('lint'), kind: 'lint', label: 'Lint errors' }) },
  { id: 'recent', label: 'Recent changes', kind: 'recent-changes', immediate: () => ({ id: uid('rc'), kind: 'recent-changes', label: 'Recent changes' }) },
  { id: 'chats', label: 'Past chats', kind: 'past-chat' },
  { id: 'rules', label: 'Rules', kind: 'rule' },
  { id: 'skills', label: 'Skills', kind: 'skill' },
  { id: 'link', label: 'Link', kind: 'link', hint: 'paste a URL' },
  { id: 'notes', label: 'Notes', kind: 'note', hint: 'free-form note' },
  { id: 'mcp', label: 'MCP resources', kind: 'mcp-resource' },
  { id: 'openfiles', label: 'Open files', kind: 'open-files', immediate: () => ({ id: uid('of'), kind: 'open-files', label: 'Open files' }) },
]

interface Props { query: string; onPick: (item: ContextItem) => void; onClose: () => void; sessionId: string }

function useDebounced<T>(v: T, ms: number): T { const [d, setD] = useState(v); useEffect(() => { const t = setTimeout(() => setD(v), ms); return () => clearTimeout(t) }, [v, ms]); return d }

export function MentionPicker({ query, onPick, onClose, sessionId }: Props) {
  const [cat, setCat] = useState<Category | null>(null)
  const [items, setItems] = useState<ContextItem[]>([])
  const [loading, setLoading] = useState(false)
  const root = useWorkspace((s) => s.workspace?.root)
  const sessions = useChat((s) => s.sessions)
  const q = useDebounced(query, 90)
  const listRef = useRef<HTMLDivElement>(null)
  void sessionId

  // Reset category when the query is cleared by the user.
  useEffect(() => { if (query === '' && cat && !cat.immediate) { /* keep */ } }, [query, cat])

  useEffect(() => {
    let alive = true
    const run = async () => {
      setLoading(true)
      try {
        const out: ContextItem[] = []
        const effective = cat?.id ?? (q ? 'all' : 'none')
        if (effective === 'none') { if (alive) setItems([]); return }
        if (effective === 'files' || effective === 'all') {
          const [files, folders] = await Promise.all([
            api.fs.quickOpen(q || '', effective === 'all' ? 8 : 30).catch(() => []),
            q ? api.chat.mentionCandidates('folder', q, effective === 'all' ? 4 : 15).catch(() => [] as ContextItem[]) : Promise.resolve([] as ContextItem[]),
          ])
          out.push(...files.map((f) => ({ id: uid('file'), kind: 'file' as const, label: f.relPath, path: f.path })))
          out.push(...folders)
        }
        if (effective === 'code' || (effective === 'all' && q.length > 1)) {
          if (hasCommand('editor.getDocumentSymbols')) {
            const syms = ((await runCommand<{ name: string; path: string; startLine: number; endLine: number; kind?: string }[]>('editor.getDocumentSymbols', { query: q })) ?? []).slice(0, 20)
            out.push(...syms.map((s) => ({ id: uid('sym'), kind: 'symbol' as const, label: s.name, path: s.path, range: { startLine: s.startLine, endLine: s.endLine }, data: s.kind })))
          } else if (effective === 'code') {
            out.push(...(await api.chat.mentionCandidates('symbol', q, 20).catch(() => [] as ContextItem[])))
          }
        }
        if (effective === 'git') {
          out.push({ id: uid('g'), kind: 'git-diff', label: 'Diff of working state' }, { id: uid('g'), kind: 'git-staged', label: 'Staged changes' })
          const def = await api.git.defaultBranch().catch(() => null)
          out.push({ id: uid('g'), kind: 'git-branch-diff', label: `Diff with ${def ?? 'main'}`, base: def ?? 'main' })
          const log = await api.git.log({ limit: 15 }).catch(() => [])
          out.push(...log.filter((c) => !q || c.summary.toLowerCase().includes(q.toLowerCase()) || c.shortSha.startsWith(q)).map((c) => ({ id: uid('c'), kind: 'git-commit' as const, label: `${c.shortSha} ${c.summary}`, sha: c.sha })))
        }
        if (effective === 'docs' || effective === 'all') {
          const docs = await api.docs.list().catch(() => [])
          out.push(...docs.filter((d) => !q || d.name.toLowerCase().includes(q.toLowerCase())).map((d) => ({ id: uid('d'), kind: 'docs' as const, label: d.name, docId: d.id, url: d.url })))
        }
        if (effective === 'terminals') { const t = await api.pty.list().catch(() => []); out.push(...t.map((p) => ({ id: uid('t'), kind: 'terminal' as const, label: p.title || p.shell, terminalId: p.id }))) }
        if (effective === 'chats' || effective === 'all') out.push(...sessions.filter((s) => !s.isSubagent && (!q || s.title.toLowerCase().includes(q.toLowerCase()))).slice(0, effective === 'all' ? 3 : 20).map((s) => ({ id: uid('pc'), kind: 'past-chat' as const, label: s.title, sessionId: s.id })))
        if (effective === 'rules' || effective === 'all') { const rules = await api.rules.list().catch(() => []); out.push(...rules.filter((r) => !q || r.name.toLowerCase().includes(q.toLowerCase())).slice(0, effective === 'all' ? 3 : 40).map((r) => ({ id: uid('r'), kind: 'rule' as const, label: r.name, ruleId: r.id, path: r.path }))) }
        if (effective === 'skills') { const sk = await api.skills.list().catch(() => []); out.push(...sk.filter((s) => !q || s.name.toLowerCase().includes(q.toLowerCase())).map((s) => ({ id: uid('sk'), kind: 'skill' as const, label: s.name, path: s.path, data: s.id }))) }
        if (effective === 'mcp') { const res = await api.mcp.resources().catch(() => []); out.push(...res.filter((r) => !q || r.name.toLowerCase().includes(q.toLowerCase()) || r.uri.includes(q)).slice(0, 50).map((r) => ({ id: uid('m'), kind: 'mcp-resource' as const, label: r.name || r.uri, uri: r.uri, serverId: r.serverId, mime: r.mimeType }))) }
        if (alive) setItems(out)
      } finally { if (alive) setLoading(false) }
    }
    void run()
    return () => { alive = false }
  }, [q, cat, sessions])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.stopPropagation(); if (cat) setCat(null); else onClose() } }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [cat, onClose])

  const showCategories = !cat && !q
  const catMatches = useMemo(() => (q && !cat ? CATEGORIES.filter((c) => c.label.toLowerCase().includes(q.toLowerCase())) : []), [q, cat])

  const pickCategory = (c: Category) => {
    if (c.immediate) { const it = c.immediate(q); if (it) onPick(it); return }
    if (c.id === 'link') { const url = window.prompt('Link URL'); if (url) onPick({ id: uid('l'), kind: 'link', label: url, url }); return }
    if (c.id === 'notes') { const note = window.prompt('Note text'); if (note) onPick({ id: uid('n'), kind: 'note', label: note.slice(0, 30), data: note }); return }
    setCat(c)
  }
  const addDoc = async () => { const url = window.prompt('Documentation URL to index'); if (!url) return; try { const d = await api.docs.add(url); onPick({ id: uid('d'), kind: 'docs', label: d.name, docId: d.id, url: d.url }) } catch (e) { void runCommand('app.notify', { level: 'error', title: 'Add docs failed', message: String(e) }) } }

  return (
    <div className="absolute bottom-full left-0 right-0 mb-1 z-30 rounded-md border border-border bg-tooltip shadow-[var(--shadow)] overflow-hidden text-[12px]">
      <Command shouldFilter={false} loop label="Add context">
        <div className="flex items-center gap-1.5 px-2 h-7 border-b border-border text-[11px] text-muted">
          {cat ? <button type="button" onClick={() => setCat(null)} className="flex items-center gap-0.5 hover:text-fg"><ChevronLeft size={12} /> {cat.label}</button> : <span>Add context</span>}
          {cat?.hint && <span className="text-faint">· {cat.hint}</span>}
          <span className="flex-1" />
          {loading && <span className="pulse">loading…</span>}
          <span className="text-faint">↑↓ navigate · ⏎ add · esc close</span>
        </div>
        <Command.Input value={query} readOnly className="sr-only" aria-hidden />
        <Command.List ref={listRef} className="max-h-72 overflow-y-auto p-1">
          <Command.Empty className="px-2 py-3 text-faint text-center">{loading ? 'Searching…' : 'No matches'}</Command.Empty>
          {showCategories && CATEGORIES.map((c) => { const Icon = kindIcon(c.kind); return (
            <Command.Item key={c.id} value={`cat:${c.id}`} onSelect={() => pickCategory(c)} className="flex items-center gap-2 px-2 h-7 rounded data-[selected=true]:bg-selected cursor-default"><Icon size={13} className="text-muted" /><span>{c.label}</span>{c.hint && <span className="text-faint">{c.hint}</span>}{!c.immediate && c.id !== 'link' && c.id !== 'notes' && <span className="ml-auto text-faint">›</span>}</Command.Item>
          ) })}
          {catMatches.length > 0 && <Command.Group heading="Categories" className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1 [&_[cmdk-group-heading]]:text-[10.5px] [&_[cmdk-group-heading]]:text-faint">{catMatches.map((c) => { const Icon = kindIcon(c.kind); return <Command.Item key={c.id} value={`cat:${c.id}`} onSelect={() => pickCategory(c)} className="flex items-center gap-2 px-2 h-7 rounded data-[selected=true]:bg-selected cursor-default"><Icon size={13} className="text-muted" />{c.label}</Command.Item> })}</Command.Group>}
          {(cat?.id === 'codebase' || cat?.id === 'web') && <Command.Item value="immediate" onSelect={() => pickCategory(cat)} className="flex items-center gap-2 px-2 h-7 rounded data-[selected=true]:bg-selected"><Plus size={12} /> Add {cat.label}{q ? ` for "${q}"` : ''}</Command.Item>}
          {cat?.id === 'docs' && <Command.Item value="adddoc" onSelect={() => void addDoc()} className="flex items-center gap-2 px-2 h-7 rounded data-[selected=true]:bg-selected"><Plus size={12} /> Add new doc…</Command.Item>}
          {items.map((it) => { const Icon = kindIcon(it.kind); const sub = it.path ? relToRoot(it.path, root) : it.url ?? it.uri ?? ''; return (
            <Command.Item key={it.id} value={it.id} onSelect={() => onPick(it)} className="flex items-center gap-2 px-2 h-7 rounded data-[selected=true]:bg-selected cursor-default">
              <Icon size={13} className="text-muted shrink-0" />
              <span className="truncate-1">{it.kind === 'file' || it.kind === 'folder' ? baseName(it.path ?? it.label) : it.label}</span>
              {sub && <span className="truncate-1 text-faint text-[11px] ml-1">{sub}</span>}
              {it.range && <span className="text-faint ml-auto">:{it.range.startLine}</span>}
            </Command.Item>
          ) })}
        </Command.List>
      </Command>
    </div>
  )
}

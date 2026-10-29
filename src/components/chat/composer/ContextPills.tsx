import { memo } from 'react'
import { AtSign, BookOpen, Brain, CircleDot, Clock, Code, FileText, Folder, GitBranch, Globe, Image, Link, ListChecks, MessageSquare, Pin, PinOff, Server, Sparkles, SquareTerminal, StickyNote, TriangleAlert, X, Files } from 'lucide-react'
import type { ContextItem, ContextItemKind } from '@/types'
import { contextLabel } from '../util'
import { useWorkspace } from '@/store/workspace'
import { runCommand, hasCommand } from '@/lib/commands'

const ICON: Record<ContextItemKind, typeof FileText> = {
  file: FileText, 'active-file': FileText, folder: Folder, code: Code, selection: Code, symbol: Code, codebase: Sparkles, 'git-diff': GitBranch, 'git-staged': GitBranch, 'git-commit': CircleDot, 'git-branch-diff': GitBranch,
  web: Globe, docs: BookOpen, terminal: SquareTerminal, lint: TriangleAlert, 'recent-changes': Clock, 'past-chat': MessageSquare, rule: Brain, link: Link, image: Image, note: StickyNote, 'mcp-resource': Server, 'open-files': Files, plan: ListChecks, skill: Sparkles,
}
export function kindIcon(kind: ContextItemKind) { return ICON[kind] ?? AtSign }

export const ContextPill = memo(function ContextPill({ item, onRemove, onTogglePin, dim, small, className = '' }: { item: ContextItem; onRemove?: () => void; onTogglePin?: () => void; dim?: boolean; small?: boolean; className?: string }) {
  const root = useWorkspace((s) => s.workspace?.root)
  const Icon = kindIcon(item.kind)
  const label = contextLabel(item, root)
  const clickable = !!item.path && hasCommand('editor.openFile')
  const open = () => { if (clickable) void runCommand('editor.openFile', { path: item.path!, line: item.range?.startLine }) }
  return (
    <span className={`group/pill inline-flex items-center gap-1 ${small ? 'h-[18px] text-[10.5px] px-1' : 'h-5 text-[11px] px-1.5'} rounded border ${dim ? 'border-dashed border-border text-faint' : 'border-border bg-input text-fg'} max-w-[220px] ${className}`} title={item.path ?? item.url ?? item.query ?? label}>
      <Icon size={small ? 10 : 11} className="shrink-0 text-muted" />
      <button type="button" onClick={open} className={`truncate-1 ${clickable ? 'hover:underline' : 'cursor-pointer'}`}>{label}</button>
      {item.pinned && !onTogglePin && <Pin size={9} className="text-accent shrink-0" />}
      {onTogglePin && <button type="button" onClick={onTogglePin} title={item.pinned ? 'Unpin (remove after sending)' : 'Pin (keep for future messages)'} className={`shrink-0 ${item.pinned ? 'text-accent' : 'opacity-0 group-hover/pill:opacity-100 text-faint hover:text-fg'}`}>{item.pinned ? <Pin size={9} /> : <PinOff size={9} />}</button>}
      {onRemove && <button type="button" onClick={onRemove} title="Remove" className="shrink-0 text-faint hover:text-fg press"><X size={10} /></button>}
    </span>
  )
})

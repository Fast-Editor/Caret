import { useEffect, useRef } from 'react'
import type { monaco } from '@/lib/monaco/setup'
import { runCommand } from '@/lib/commands'
import { useProblems } from '@/store/problems'
import { relPath } from '@/store/workspace'
import type { EditorTab } from '@/store/editor'
import { Sparkles, MessageSquarePlus, Wand2, Bug, ArrowRight, Users, Pencil, AlignLeft, Copy, FolderOpen, FileCode2 } from 'lucide-react'

export interface MenuPos { x: number; y: number; position?: monaco.Position }
interface Item { label: string; icon?: React.ReactNode; shortcut?: string; run: () => void; sep?: boolean; disabled?: boolean }

export function EditorContextMenu({ pos, editor, tab, hasProblems, onClose }: { pos: MenuPos; editor: monaco.editor.IStandaloneCodeEditor; tab: EditorTab; hasProblems: boolean; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const down = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node)) onClose() }
    const key = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    window.addEventListener('mousedown', down, true); window.addEventListener('keydown', key, true)
    return () => { window.removeEventListener('mousedown', down, true); window.removeEventListener('keydown', key, true) }
  }, [onClose])
  const model = editor.getModel()
  const sel = editor.getSelection()
  const hasSel = !!sel && !sel.isEmpty()
  const line = pos.position?.lineNumber ?? sel?.startLineNumber ?? 1
  const diagAtLine = tab.path ? useProblems.getState().forPath(tab.path).filter((d) => d.range.start.line + 1 <= line && d.range.end.line + 1 >= line) : []
  const act = (id: string) => () => { onClose(); void editor.getAction(id)?.run() }
  const items: Item[] = [
    { label: 'Add to Chat', icon: <MessageSquarePlus size={14} />, shortcut: '⌘L', run: () => { onClose(); void runCommand('chat.addSelection') } },
    { label: hasSel ? 'Edit Selection' : 'Edit (⌘K)', icon: <Wand2 size={14} />, shortcut: '⌘K', run: () => { onClose(); void runCommand('ai.inlineEdit') } },
    { label: 'Explain', icon: <Sparkles size={14} />, run: () => { onClose(); void runCommand('ai.explain') } },
    ...(diagAtLine.length ? [{ label: 'Fix with AI', icon: <Bug size={14} />, run: () => { onClose(); const d = diagAtLine[0]; void runCommand('ai.fixInChat', { path: tab.path!, line, message: d.message, code: model?.getLineContent(line) }) } }] : []),
    { label: 'Go to Definition', icon: <ArrowRight size={14} />, shortcut: 'F12', run: act('editor.action.revealDefinition'), sep: true },
    { label: 'Go to References', icon: <Users size={14} />, shortcut: '⇧F12', run: act('editor.action.goToReferences') },
    { label: 'Peek Definition', run: act('editor.action.peekDefinition') },
    { label: 'Rename Symbol', icon: <Pencil size={14} />, shortcut: 'F2', run: act('editor.action.rename') },
    { label: 'Change All Occurrences', shortcut: '⌘F2', run: act('editor.action.changeAll') },
    { label: 'Format Document', icon: <AlignLeft size={14} />, shortcut: '⇧⌥F', run: act('editor.action.formatDocument'), sep: true },
    ...(hasSel ? [{ label: 'Format Selection', run: act('editor.action.formatSelection') }] : []),
    { label: 'Quick Fix…', shortcut: '⌘.', run: act('editor.action.quickFix'), disabled: !hasProblems },
    { label: 'Cut', shortcut: '⌘X', run: act('editor.action.clipboardCutAction'), sep: true },
    { label: 'Copy', shortcut: '⌘C', run: act('editor.action.clipboardCopyAction') },
    { label: 'Paste', shortcut: '⌘V', run: act('editor.action.clipboardPasteAction') },
    { label: 'Command Palette…', shortcut: '⌘⇧P', run: () => { onClose(); void runCommand('palette.commands') }, sep: true },
    ...(tab.path ? [
      { label: 'Copy Path', icon: <Copy size={14} />, run: () => { onClose(); void navigator.clipboard.writeText(tab.path!) }, sep: true },
      { label: 'Copy Relative Path', icon: <FileCode2 size={14} />, run: () => { onClose(); void navigator.clipboard.writeText(relPath(tab.path!)) } },
      { label: 'Reveal in Explorer', icon: <FolderOpen size={14} />, run: () => { onClose(); void runCommand('view.explorer', { reveal: tab.path }) } },
    ] : []),
  ]
  const maxH = window.innerHeight - 12
  const top = Math.min(pos.y, Math.max(8, maxH - Math.min(items.length * 26 + 16, maxH)))
  const left = Math.min(pos.x, window.innerWidth - 260)
  return (
    <div ref={ref} className="fixed z-[1000] min-w-[230px] py-1 rounded-md border border-border bg-tooltip text-fg text-xs shadow-[var(--shadow)]" style={{ top, left }} onContextMenu={(e) => e.preventDefault()}>
      {items.map((it, i) => (
        <div key={i}>
          {it.sep && i > 0 && <div className="my-1 border-t border-border" />}
          <button disabled={it.disabled} className="w-full flex items-center gap-2 px-3 h-[26px] text-left hover:bg-hover disabled:opacity-40" onClick={it.run}>
            <span className="w-4 text-muted flex items-center">{it.icon}</span>
            <span className="flex-1">{it.label}</span>
            {it.shortcut && <span className="text-faint">{it.shortcut}</span>}
          </button>
        </div>
      ))}
    </div>
  )
}

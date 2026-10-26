/** Inline git blame for the current line (Cursor "cursorBlame"-style) + optional gutter toggle. */
import { monaco } from './setup'
import { api } from '@/lib/api'
import { useWorkspace, relPath } from '@/store/workspace'
import { useEditor } from '@/store/editor'
import { subscribe as busSubscribe } from '@/lib/bus'
import type { GitBlameLine } from '@/types'

const blameCache = new Map<string, { lines: GitBlameLine[]; at: number }>()
export function timeAgo(ts: number): string {
  const s = Math.max(0, Date.now() / 1000 - ts)
  if (s < 60) return 'just now'; const m = s / 60; if (m < 60) return `${Math.floor(m)}m ago`; const h = m / 60; if (h < 24) return `${Math.floor(h)}h ago`; const d = h / 24; if (d < 30) return `${Math.floor(d)}d ago`; const mo = d / 30; if (mo < 12) return `${Math.floor(mo)}mo ago`; return `${Math.floor(mo / 12)}y ago`
}

export function attachBlame(editor: monaco.editor.ICodeEditor): monaco.IDisposable {
  const coll = editor.createDecorationsCollection()
  let gen = 0
  async function update() {
    const model = editor.getModel(); const root = useWorkspace.getState().workspace?.root; const pos = editor.getPosition()
    if (!useEditor.getState().blameEnabled || !model || !pos || model.uri.scheme !== 'file' || !root || !model.uri.path.startsWith(root)) { coll.clear(); return }
    const path = model.uri.path; const my = ++gen
    let entry = blameCache.get(path)
    if (!entry || Date.now() - entry.at > 30_000) {
      try { entry = { lines: await api.git.blame(relPath(path)), at: Date.now() } } catch { entry = { lines: [], at: Date.now() } }
      blameCache.set(path, entry)
    }
    if (my !== gen || model.isDisposed()) return
    const dirty = useEditor.getState().groups.some((g) => g.tabs.some((t) => t.path === path && t.dirty))
    const b = entry.lines.find((l) => l.line === pos.lineNumber)
    if (!b) { coll.clear(); return }
    const text = dirty ? 'You · uncommitted changes' : b.isUncommitted ? 'Uncommitted changes' : `${b.author}, ${timeAgo(b.time)} · ${b.summary}`
    coll.set([{ range: new monaco.Range(pos.lineNumber, model.getLineMaxColumn(pos.lineNumber), pos.lineNumber, model.getLineMaxColumn(pos.lineNumber)), options: { after: { content: `   ${text}`, inlineClassName: 'caret-blame-inline' }, isWholeLine: false } }])
  }
  let timer: ReturnType<typeof setTimeout> | null = null
  const schedule = () => { if (timer) clearTimeout(timer); timer = setTimeout(() => { timer = null; void update() }, 120) }
  const d: monaco.IDisposable[] = [
    editor.onDidChangeCursorPosition(schedule), editor.onDidChangeModel(() => { coll.clear(); schedule() }),
    { dispose: useEditor.subscribe((s, p) => { if (s.blameEnabled !== p.blameEnabled) { blameCache.clear(); schedule() } }) },
    { dispose: busSubscribe<{ path: string }>('editor:saved', ({ path }) => { blameCache.delete(path); schedule() }) },
  ]
  return { dispose: () => { if (timer) clearTimeout(timer); coll.clear(); d.forEach((x) => x.dispose()) } }
}

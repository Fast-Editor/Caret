/** Git gutter decorations (added/modified/deleted vs HEAD) for the model shown in an editor. */
import { monaco } from './setup'
import { api } from '@/lib/api'
import { Events, on } from '@/lib/tauri'
import { subscribe as busSubscribe } from '@/lib/bus'
import { useWorkspace, relPath } from '@/store/workspace'
import { lineDiff } from './lineDiff'

const headCache = new Map<string, { text: string | null; at: number }>()
const MAX_BYTES = 1024 * 1024

export function attachGitGutter(editor: monaco.editor.ICodeEditor): monaco.IDisposable {
  const coll = editor.createDecorationsCollection()
  let timer: ReturnType<typeof setTimeout> | null = null
  let gen = 0
  const schedule = (delay = 600) => { if (timer) clearTimeout(timer); timer = setTimeout(() => { timer = null; void refresh() }, delay) }
  async function refresh() {
    const model = editor.getModel(); const root = useWorkspace.getState().workspace?.root
    if (!model || model.uri.scheme !== 'file' || !root || !model.uri.path.startsWith(root) || model.getValueLength() > MAX_BYTES) { coll.clear(); return }
    const path = model.uri.path; const my = ++gen
    let head = headCache.get(path)
    if (!head || Date.now() - head.at > 15_000) {
      try { const text = await api.git.showFile(relPath(path), 'HEAD'); head = { text, at: Date.now() } } catch { head = { text: null, at: Date.now() } }
      headCache.set(path, head)
      if (headCache.size > 60) { const oldest = [...headCache.entries()].sort((a, b) => a[1].at - b[1].at)[0]; if (oldest) headCache.delete(oldest[0]) }
    }
    if (my !== gen || model.isDisposed()) return
    if (head.text === null) { coll.clear(); return }
    const hunks = lineDiff(head.text.split(/\r?\n/), model.getLinesContent())
    coll.set(hunks.map((h) => ({ range: new monaco.Range(h.start, 1, h.end, 1), options: { isWholeLine: false, linesDecorationsClassName: `caret-gutter-${h.kind}`, overviewRuler: { color: h.kind === 'added' ? '#6fcf97' : h.kind === 'modified' ? '#6b9dff' : '#f2655f', position: monaco.editor.OverviewRulerLane.Left } } })))
  }
  const d: monaco.IDisposable[] = [
    editor.onDidChangeModel(() => { coll.clear(); schedule(50) }),
    editor.onDidChangeModelContent(() => schedule(800)),
    { dispose: busSubscribe<{ path: string }>('editor:saved', ({ path }) => { headCache.delete(path); if (editor.getModel()?.uri.path === path) schedule(100) }) },
    { dispose: on(Events.gitChanged, () => { headCache.clear(); schedule(300) }) },
  ]
  schedule(50)
  return { dispose: () => { if (timer) clearTimeout(timer); coll.clear(); d.forEach((x) => x.dispose()) } }
}

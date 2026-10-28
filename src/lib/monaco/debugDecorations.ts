// Breakpoint glyphs, the current-execution-line highlight, and F-key debugger shortcuts inside Monaco editors.
import { monaco } from '@/lib/monaco/setup'
import { runCommand } from '@/lib/commands'
import { useDebug } from '@/store/debug'

const CSS = `
.caret-bp-glyph::after { content: ''; display: block; width: 9px; height: 9px; margin: 5px auto 0; border-radius: 50%; background: var(--danger); }
.caret-bp-glyph-unverified::after { content: ''; display: block; width: 9px; height: 9px; margin: 5px auto 0; border-radius: 50%; border: 1.5px solid var(--fg-faint); box-sizing: border-box; }
.caret-bp-glyph-hover::after { content: ''; display: block; width: 9px; height: 9px; margin: 5px auto 0; border-radius: 50%; background: var(--danger); opacity: .35; }
.caret-debug-current-line { background: color-mix(in srgb, var(--warning) 18%, transparent); }
.caret-debug-current-glyph::after { content: '▶'; display: block; color: var(--warning); font-size: 10px; line-height: 19px; text-align: center; }
.caret-debug-current-glyph.caret-bp-glyph::after { content: '▶'; background: none; width: auto; height: auto; margin: 0; border-radius: 0; }
`
let cssInjected = false
function injectCss() { if (cssInjected || typeof document === 'undefined') return; cssInjected = true; const el = document.createElement('style'); el.setAttribute('data-caret', 'debug'); el.textContent = CSS; document.head.appendChild(el) }

function modelPath(editor: monaco.editor.IStandaloneCodeEditor): string | null { const m = editor.getModel(); return m && m.uri.scheme === 'file' ? m.uri.path : null }

/** Install breakpoint gutter + current line decorations on an editor; returns a disposable. */
export function installDebugDecorations(editor: monaco.editor.IStandaloneCodeEditor): monaco.IDisposable {
  injectCss()
  editor.updateOptions({ glyphMargin: true })
  const collection = editor.createDecorationsCollection()
  let hoverLine: number | null = null

  const render = () => {
    const path = modelPath(editor); const model = editor.getModel()
    if (!path || !model) { collection.clear(); return }
    const st = useDebug.getState()
    const lines = st.breakpoints[path] ?? []
    const verified = new Map((st.verified[path] ?? []).filter((b) => b.line != null).map((b) => [b.line as number, b.verified]))
    const sessionLive = st.state !== 'idle' && st.state !== 'terminated'
    const decos: monaco.editor.IModelDeltaDecoration[] = lines.filter((l) => l >= 1 && l <= model.getLineCount()).map((l) => ({
      range: new monaco.Range(l, 1, l, 1),
      options: { glyphMarginClassName: sessionLive && verified.get(l) === false ? 'caret-bp-glyph-unverified' : 'caret-bp-glyph', glyphMarginHoverMessage: { value: verified.get(l) === false && sessionLive ? 'Breakpoint (not verified by the adapter)' : 'Breakpoint' }, stickiness: monaco.editor.TrackedRangeStickiness.NeverGrowsWhenTypingAtEdges },
    }))
    if (hoverLine && !lines.includes(hoverLine) && hoverLine <= model.getLineCount()) decos.push({ range: new monaco.Range(hoverLine, 1, hoverLine, 1), options: { glyphMarginClassName: 'caret-bp-glyph-hover' } })
    const loc = st.currentLocation
    if (loc && loc.path === path && loc.line >= 1 && loc.line <= model.getLineCount()) {
      decos.push({ range: new monaco.Range(loc.line, 1, loc.line, model.getLineMaxColumn(loc.line)), options: { isWholeLine: true, className: 'caret-debug-current-line', glyphMarginClassName: 'caret-debug-current-glyph', overviewRuler: { color: 'rgba(226,179,91,0.9)', position: monaco.editor.OverviewRulerLane.Full } } })
    }
    collection.set(decos)
  }

  const unsub = useDebug.subscribe((s, p) => { if (s.breakpoints !== p.breakpoints || s.verified !== p.verified || s.currentLocation !== p.currentLocation || s.state !== p.state) render() })
  const d1 = editor.onDidChangeModel(() => { hoverLine = null; render() })
  const d2 = editor.onMouseDown((e) => {
    if (e.target.type !== monaco.editor.MouseTargetType.GUTTER_GLYPH_MARGIN) return
    const path = modelPath(editor); const line = e.target.position?.lineNumber; if (!path || !line) return
    e.event.preventDefault(); void useDebug.getState().toggleBreakpoint(path, line)
  })
  const d3 = editor.onMouseMove((e) => {
    const l = e.target.type === monaco.editor.MouseTargetType.GUTTER_GLYPH_MARGIN ? e.target.position?.lineNumber ?? null : null
    if (l !== hoverLine) { hoverLine = l; render() }
  })
  const d4 = editor.onMouseLeave(() => { if (hoverLine !== null) { hoverLine = null; render() } })
  // F-keys while the editor has focus (the shell binds them too; these make sure Monaco does not swallow them)
  const cmds: [number, string][] = [[monaco.KeyCode.F5, 'debug.startOrContinue'], [monaco.KeyMod.Shift | monaco.KeyCode.F5, 'debug.stop'], [monaco.KeyCode.F9, 'debug.toggleBreakpoint'], [monaco.KeyCode.F10, 'debug.stepOver'], [monaco.KeyCode.F11, 'debug.stepInto'], [monaco.KeyMod.Shift | monaco.KeyCode.F11, 'debug.stepOut']]
  for (const [keys, cmd] of cmds) editor.addCommand(keys, () => { void runCommand(cmd) })
  render()
  return { dispose: () => { unsub(); d1.dispose(); d2.dispose(); d3.dispose(); d4.dispose(); collection.clear() } }
}

/** Toggle a breakpoint at the cursor of the given editor (used by the F9 command). */
export function toggleBreakpointAtCursor(editor: monaco.editor.IStandaloneCodeEditor | null): boolean {
  const path = editor ? modelPath(editor) : null; const line = editor?.getPosition()?.lineNumber
  if (!path || !line) return false
  void useDebug.getState().toggleBreakpoint(path, line); return true
}

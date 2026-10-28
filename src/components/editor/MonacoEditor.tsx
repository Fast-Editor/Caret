import { useEffect, useRef, useState } from 'react'
import { monaco, initMonaco, editorOptionsFromSettings } from '@/lib/monaco/setup'
import { useSettings } from '@/store/settings'
import { useEditor, registerEditor, saveViewState, loadViewState, getModel, primeLineCache, type EditorTab } from '@/store/editor'
import { attachGitGutter } from '@/lib/monaco/gitGutter'
import { attachBlame } from '@/lib/monaco/blame'
import { inlineEditFor } from '@/lib/monaco/inlineEdit'
import { installDebugDecorations } from '@/lib/monaco/debugDecorations'
import { runCommand } from '@/lib/commands'
import { useProblems } from '@/store/problems'
import { EditorContextMenu, type MenuPos } from './EditorContextMenu'

interface Props { groupId: string; tab: EditorTab; active: boolean }

/** One Monaco instance per group; models are swapped in as tabs change. */
export function MonacoEditor({ groupId, tab, active }: Props) {
  const hostRef = useRef<HTMLDivElement>(null)
  const editorRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null)
  const ui = useSettings((s) => s.settings?.ui)
  const [menu, setMenu] = useState<MenuPos | null>(null)
  const prevTabId = useRef<string | null>(null)

  // create editor once per group
  useEffect(() => {
    initMonaco()
    const el = hostRef.current!
    const editor = monaco.editor.create(el, { ...editorOptionsFromSettings(useSettings.getState().settings?.ui), model: null })
    editorRef.current = editor
    const unregister = registerEditor(groupId, editor)
    const disposables: monaco.IDisposable[] = [attachGitGutter(editor), attachBlame(editor), installDebugDecorations(editor)]
    inlineEditFor(editor)
    // keybindings → commands (shell skips global handling when defaultPrevented)
    const bind = (keys: number, cmd: string, args?: unknown) => { disposables.push({ dispose: () => {} }); editor.addCommand(keys, () => { void runCommand(cmd, args) }) }
    bind(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyK, 'ai.inlineEdit')
    bind(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyL, 'chat.addSelection')
    bind(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyI, 'chat.open')
    bind(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, 'file.save')
    bind(monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyO, 'editor.goToSymbol')
    bind(monaco.KeyMod.WinCtrl | monaco.KeyCode.KeyG, 'editor.goToLine')
    bind(monaco.KeyMod.Alt | monaco.KeyCode.Backslash, 'ai.tab.trigger')
    bind(monaco.KeyMod.CtrlCmd | monaco.KeyCode.F12, 'editor.goToDefinition')
    bind(monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyL, 'ai.explain')
    editor.addAction({ id: 'caret.addToChat', label: 'Add to Chat', contextMenuGroupId: 'ai', contextMenuOrder: 1, run: () => { void runCommand('chat.addSelection') } })
    disposables.push(editor.onDidChangeCursorPosition((e) => {
      const model = editor.getModel(); if (!model) return
      if (useEditor.getState().activeGroupId !== groupId) return
      const sel = editor.getSelection()
      const selected = sel ? model.getValueLengthInRange(sel) : 0
      useEditor.setState({ cursor: { line: e.position.lineNumber, column: e.position.column, selected } })
      if (model.uri.scheme === 'file') primeLineCache(model.uri.path, e.position.lineNumber)
      if (e.source === 'mouse' || e.reason === monaco.editor.CursorChangeReason.Explicit) { if (model.uri.scheme === 'file' && e.source !== 'keyboard') useEditor.getState().pushNav({ path: model.uri.path, line: e.position.lineNumber, column: e.position.column }) }
    }))
    disposables.push(editor.onDidFocusEditorText(() => useEditor.getState().setActiveGroup(groupId)))
    disposables.push(editor.onContextMenu((e) => { e.event.preventDefault(); setMenu({ x: e.event.posx, y: e.event.posy, position: e.target.position ?? undefined }) }))
    disposables.push(editor.onMouseDown((e) => { if (e.event.middleButton) return }))
    // ctrl/cmd+click → go to definition is handled by Monaco's gotoDefinition contribution with the mouse modifier
    const unsubSettings = useSettings.subscribe((s, prev) => { if (s.settings?.ui !== prev.settings?.ui) editor.updateOptions(editorOptionsFromSettings(s.settings?.ui)) })
    const ro = new ResizeObserver(() => editor.layout())
    ro.observe(el)
    return () => { ro.disconnect(); unsubSettings(); unregister(); for (const d of disposables) d.dispose(); if (prevTabId.current) saveViewState(prevTabId.current, editor.saveViewState()); editor.dispose() }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [groupId])

  // swap model when the tab changes
  useEffect(() => {
    const editor = editorRef.current; if (!editor) return
    if (prevTabId.current && prevTabId.current !== tab.id) saveViewState(prevTabId.current, editor.saveViewState())
    const key = tab.kind === 'untitled' ? `untitled:${tab.id}` : tab.path ?? ''
    const model = getModel(key) ?? null
    if (editor.getModel() !== model) {
      editor.setModel(model)
      const vs = loadViewState(tab.id); if (vs) editor.restoreViewState(vs)
    }
    editor.updateOptions({ readOnly: !!tab.readonly, readOnlyMessage: tab.readonly ? { value: tab.notice ?? 'Read-only' } : undefined })
    prevTabId.current = tab.id
    if (active) requestAnimationFrame(() => { if (useEditor.getState().activeGroupId === groupId) editor.focus() })
  }, [tab.id, tab.path, tab.readonly, tab.kind, active, groupId])

  // keep options in sync when ui settings object identity changes (cheap)
  useEffect(() => { editorRef.current?.updateOptions(editorOptionsFromSettings(ui)) }, [ui])

  const problems = useProblems((s) => (tab.path ? s.byPath[tab.path] : undefined))
  const problemCount = problems ? Object.values(problems).flat().length : 0

  return (
    <div className="relative h-full w-full min-w-0 min-h-0 bg-editor">
      {tab.notice && <div className="caret-editor-notice text-xs px-3 py-1 flex items-center justify-between border-b border-border">
        <span>{tab.notice}</span>
        {tab.conflict && tab.path && <span className="flex gap-2"><button className="underline" onClick={() => void useEditor.getState().revert(tab.path!)}>Revert</button><button className="underline" onClick={() => void useEditor.getState().save(tab.path!)}>Overwrite</button></span>}
      </div>}
      <div ref={hostRef} className="absolute inset-0" style={{ top: tab.notice ? 24 : 0 }} />
      {menu && <EditorContextMenu pos={menu} editor={editorRef.current!} tab={tab} hasProblems={problemCount > 0} onClose={() => setMenu(null)} />}
    </div>
  )
}

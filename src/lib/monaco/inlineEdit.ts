/**
 * Cmd+K inline edit controller: one per editor. Hosts a React widget in a view zone above the
 * selection, streams the result, previews it as an inline diff (green new lines + red removed view zone),
 * and offers Accept (⌘↩) / Reject (⌘⌫) / follow-up.
 */
import { monaco } from './setup'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import { suspendTab } from './inlineCompletions'
import type { ContextItem, InlineEditRequest } from '@/types'

export type InlineEditMode = 'edit' | 'question' | 'generate'
export type InlineEditPhase = 'input' | 'streaming' | 'preview' | 'answer' | 'error'
export interface InlineEditState {
  phase: InlineEditPhase; mode: InlineEditMode; instruction: string; streamed: string; answer: string; error?: string
  startLine: number; endLine: number; history: { instruction: string; result: string }[]; context: ContextItem[]
}
export type Listener = (s: InlineEditState) => void

interface Host { mount(container: HTMLElement, ctrl: InlineEditController): () => void }
let host: Host | null = null
/** The React layer registers how to render the widget (keeps this module free of React). */
export function setInlineEditHost(h: Host): void { host = h }

const controllers = new WeakMap<monaco.editor.IStandaloneCodeEditor, InlineEditController>()
export function inlineEditFor(editor: monaco.editor.IStandaloneCodeEditor): InlineEditController {
  let c = controllers.get(editor); if (!c) { c = new InlineEditController(editor); controllers.set(editor, c) }
  return c
}

let seq = 0

export class InlineEditController {
  state: InlineEditState = { phase: 'input', mode: 'edit', instruction: '', streamed: '', answer: '', startLine: 1, endLine: 1, history: [], context: [] }
  private listeners = new Set<Listener>()
  private zoneId: string | null = null
  private removedZoneId: string | null = null
  private zoneNode: HTMLElement | null = null
  private unmount: (() => void) | null = null
  private decorations: monaco.editor.IEditorDecorationsCollection | null = null
  private original: string | null = null
  private originalRange: monaco.Range | null = null
  private previewRange: monaco.Range | null = null
  private requestId: string | null = null
  private ctxOpen: monaco.editor.IContextKey<boolean>
  private ctxPreview: monaco.editor.IContextKey<boolean>
  private disposables: monaco.IDisposable[] = []

  constructor(private editor: monaco.editor.IStandaloneCodeEditor) {
    this.ctxOpen = editor.createContextKey('caretInlineEditOpen', false)
    this.ctxPreview = editor.createContextKey('caretInlineEditPreview', false)
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => this.accept(), 'caretInlineEditPreview')
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Backspace, () => this.reject(), 'caretInlineEditPreview')
    editor.addCommand(monaco.KeyCode.Escape, () => this.close(), 'caretInlineEditOpen && !suggestWidgetVisible')
    this.disposables.push(editor.onDidDispose(() => this.dispose()))
    this.disposables.push(editor.onDidChangeModel(() => this.close()))
  }

  get isOpen(): boolean { return this.zoneId !== null }
  subscribe(l: Listener): () => void { this.listeners.add(l); l(this.state); return () => { this.listeners.delete(l) } }
  private set(patch: Partial<InlineEditState>) { this.state = { ...this.state, ...patch }; for (const l of this.listeners) l(this.state) }

  open(opts: { instruction?: string; mode?: InlineEditMode; context?: ContextItem[] } = {}): void {
    const model = this.editor.getModel(); if (!model) return
    if (this.isOpen) { if (this.state.phase === 'preview') return; this.close() }
    const sel = this.editor.getSelection() ?? new monaco.Selection(1, 1, 1, 1)
    let startLine = sel.startLineNumber, endLine = sel.endLineNumber
    if (sel.isEmpty()) { startLine = endLine = sel.startLineNumber }
    else if (sel.endColumn === 1 && endLine > startLine) endLine--
    const mode: InlineEditMode = opts.mode ?? (sel.isEmpty() && model.getLineContent(startLine).trim() === '' ? 'generate' : 'edit')
    this.set({ phase: 'input', mode, instruction: opts.instruction ?? '', streamed: '', answer: '', error: undefined, startLine, endLine, context: opts.context ?? [] })
    this.decorations = this.editor.createDecorationsCollection(sel.isEmpty() ? [] : [{ range: new monaco.Range(startLine, 1, endLine, model.getLineMaxColumn(endLine)), options: { isWholeLine: true, className: 'caret-inline-target' } }])
    this.zoneNode = document.createElement('div')
    this.zoneNode.className = 'caret-inline-edit-zone'
    this.editor.changeViewZones((acc) => { this.zoneId = acc.addZone({ afterLineNumber: Math.max(0, startLine - 1), heightInPx: 96, domNode: this.zoneNode!, suppressMouseDown: false }) })
    this.unmount = host?.mount(this.zoneNode, this) ?? null
    this.ctxOpen.set(true)
    suspendTab(60_000)
    if (opts.instruction) void this.submit(opts.instruction)
  }

  /** Called by the widget when its height changes so the view zone fits. */
  setHeight(px: number): void {
    if (!this.zoneId) return
    const id = this.zoneId
    this.editor.changeViewZones((acc) => { acc.removeZone(id); this.zoneId = acc.addZone({ afterLineNumber: Math.max(0, this.state.startLine - 1), heightInPx: Math.max(60, px), domNode: this.zoneNode!, suppressMouseDown: false }) })
  }
  setMode(mode: InlineEditMode): void { this.set({ mode }) }
  setContext(context: ContextItem[]): void { this.set({ context }) }

  async submit(instruction: string): Promise<void> {
    const model = this.editor.getModel(); if (!model || !instruction.trim()) return
    if (this.state.phase === 'preview') this.accept(false)
    const { startLine, endLine, mode } = this.state
    const range = new monaco.Range(startLine, 1, endLine, model.getLineMaxColumn(endLine))
    const selectedText = mode === 'generate' ? '' : model.getValueInRange(range)
    this.set({ phase: 'streaming', instruction, streamed: '', answer: '', error: undefined })
    const requestId = `ie-${++seq}-${Date.now().toString(36)}`
    this.requestId = requestId
    const req: InlineEditRequest = { requestId, path: model.uri.path, language: model.getLanguageId(), fullText: model.getValue(), startLine, endLine, selectedText, instruction, context: this.state.context, mode: mode === 'generate' ? 'generate' : mode, history: this.state.history.slice(-4) }
    let acc = ''
    try {
      await api.ai.inlineEdit(req, (ev) => {
        if (this.requestId !== requestId) return
        if (ev.type === 'delta') { acc += ev.text; this.set({ streamed: acc }) }
        else if (ev.type === 'done') { acc = ev.text || acc; this.finish(acc) }
        else if (ev.type === 'error') this.set({ phase: 'error', error: ev.message })
      })
      if (this.requestId === requestId && this.state.phase === 'streaming') this.finish(acc)
    } catch (e) { if (this.requestId === requestId) this.set({ phase: 'error', error: String((e as Error).message ?? e) }) }
  }

  private finish(text: string): void {
    const model = this.editor.getModel(); if (!model) return
    if (this.state.mode === 'question') { this.set({ phase: 'answer', answer: text, history: [...this.state.history, { instruction: this.state.instruction, result: text }] }); return }
    const cleaned = stripFences(text)
    const { startLine, endLine } = this.state
    const range = this.state.mode === 'generate' ? new monaco.Range(startLine, 1, startLine, model.getLineMaxColumn(startLine)) : new monaco.Range(startLine, 1, endLine, model.getLineMaxColumn(endLine))
    this.original = model.getValueInRange(range)
    this.originalRange = range
    const newText = cleaned.replace(/\n$/, '')
    // apply preview edit (undoable) and compute the new range
    model.pushEditOperations([], [{ range, text: newText }], () => null)
    const newLineCount = newText.split('\n').length
    const previewEnd = startLine + newLineCount - 1
    this.previewRange = new monaco.Range(startLine, 1, previewEnd, model.getLineMaxColumn(previewEnd))
    this.decorations?.set([{ range: this.previewRange, options: { isWholeLine: true, className: 'caret-inline-added', linesDecorationsClassName: 'caret-inline-added-gutter' } }])
    // removed lines view zone (red) above the new text
    if (this.original.trim().length && this.original !== newText) {
      const node = document.createElement('div'); node.className = 'caret-inline-removed'
      const pre = document.createElement('pre'); pre.textContent = this.original; node.appendChild(pre)
      const lines = this.original.split('\n').length
      this.editor.changeViewZones((acc) => { if (this.removedZoneId) acc.removeZone(this.removedZoneId); this.removedZoneId = acc.addZone({ afterLineNumber: startLine - 1, heightInLines: Math.min(lines, 12), domNode: node, suppressMouseDown: true }) })
    }
    this.ctxPreview.set(true)
    this.set({ phase: 'preview', streamed: newText, endLine: previewEnd, history: [...this.state.history, { instruction: this.state.instruction, result: newText }] })
    this.editor.revealLinesInCenterIfOutsideViewport(startLine, previewEnd)
  }

  accept(close = true): void {
    if (this.state.phase !== 'preview') { if (close) this.close(); return }
    this.clearPreviewArtifacts()
    this.original = null; this.originalRange = null; this.previewRange = null
    this.ctxPreview.set(false)
    if (close) this.close(); else this.set({ phase: 'input' })
  }
  reject(): void {
    const model = this.editor.getModel()
    if (this.state.phase === 'preview' && model && this.previewRange && this.original !== null) {
      model.pushEditOperations([], [{ range: this.previewRange, text: this.original }], () => null)
    }
    if (this.state.phase === 'streaming' && this.requestId) { void api.ai.cancel(this.requestId).catch(() => {}) }
    this.clearPreviewArtifacts()
    this.ctxPreview.set(false)
    this.close()
  }
  private clearPreviewArtifacts(): void {
    if (this.removedZoneId) { const id = this.removedZoneId; this.removedZoneId = null; this.editor.changeViewZones((acc) => acc.removeZone(id)) }
    this.decorations?.clear()
  }
  close(): void {
    if (this.state.phase === 'preview') { this.reject(); return }
    if (this.state.phase === 'streaming' && this.requestId) { void api.ai.cancel(this.requestId).catch(() => {}); this.requestId = null }
    this.unmount?.(); this.unmount = null
    if (this.zoneId) { const id = this.zoneId; this.zoneId = null; this.editor.changeViewZones((acc) => acc.removeZone(id)) }
    this.clearPreviewArtifacts()
    this.decorations?.clear(); this.decorations = null
    this.ctxOpen.set(false); this.ctxPreview.set(false)
    suspendTab(0)
    this.set({ phase: 'input', streamed: '', answer: '' })
    this.editor.focus()
  }
  addAnswerToChat(): void {
    const model = this.editor.getModel(); if (!model) return
    const { startLine, endLine, instruction, answer } = this.state
    void runCommand('chat.sendPrompt', { text: `${instruction}\n\n(Previous inline answer)\n${answer}`, context: [{ id: `sel-${Date.now()}`, kind: 'code', label: `${model.uri.path.split('/').pop()}:${startLine}-${endLine}`, path: model.uri.path, range: { startLine, endLine } }], submit: false })
    this.close()
  }
  dispose(): void { this.close(); for (const d of this.disposables) d.dispose() }
}

export function stripFences(text: string): string {
  const m = /^\s*```[\w+-]*\n([\s\S]*?)\n?```\s*$/.exec(text)
  return (m ? m[1] : text).replace(/\r\n/g, '\n')
}

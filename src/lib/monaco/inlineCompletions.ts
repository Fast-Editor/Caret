/**
 * Tab: AI inline completions (ghost text, partial accept) + next-edit prediction rendered as Monaco inline edits.
 */
import { monaco } from './setup'
import { api } from '@/lib/api'
import { useSettings } from '@/store/settings'
import { useEditor, openFilePaths, getModel, activeEditor } from '@/store/editor'
import { diagnosticsSummary } from '@/store/problems'
import { subscribe as busSubscribe } from '@/lib/bus'
import { lineCommentPrefix } from '@/lib/languages'
import { trimSuffixOverlap, isUselessCompletion, cleanCompletion, limitLines, inLineComment, hash } from './completionUtils'
import type { CompletionRequest, NextEditResult } from '@/types'

interface CacheEntry { text: string; at: number }
const cache = new Map<string, CacheEntry>()
const nextEdits = new Map<string, { version: number; result: NextEditResult }>() // path → prediction
let inflight: { requestId: string; cancel: () => void } | null = null
let reqSeq = 0
let disposed: monaco.IDisposable[] = []
const changeEmitter = new monaco.Emitter<void>()
let suspendedUntil = 0

function tabEnabled(): boolean { return useSettings.getState().settings?.tab.enabled ?? true }
export function setTabEnabled(on: boolean): void { void useSettings.getState().update({ tab: { enabled: on } }); useEditor.getState().setTabStatus(on ? 'idle' : 'off') }
export function toggleTab(): void { setTabEnabled(!tabEnabled()) }
/** Temporarily suppress automatic completions (e.g. while Cmd+K is active). */
export function suspendTab(ms: number): void { suspendedUntil = Date.now() + ms }

function cacheKey(req: CompletionRequest): string { return `${req.path}|${hash(req.prefix.slice(-600))}|${hash(req.suffix.slice(0, 200))}` }
function pruneCache(max: number) { if (cache.size <= max) return; const entries = [...cache.entries()].sort((a, b) => a[1].at - b[1].at); for (const [k] of entries.slice(0, cache.size - max)) cache.delete(k) }

async function fetchCompletion(req: CompletionRequest, token: monaco.CancellationToken): Promise<string | null> {
  const key = cacheKey(req)
  const hit = cache.get(key); if (hit && Date.now() - hit.at < 5 * 60_000) return hit.text
  if (inflight) { inflight.cancel(); inflight = null }
  let cancelled = false
  const cancel = () => { if (cancelled) return; cancelled = true; void api.ai.cancel(req.requestId).catch(() => {}) }
  inflight = { requestId: req.requestId, cancel }
  const sub = token.onCancellationRequested(cancel)
  useEditor.getState().setTabStatus('loading')
  try {
    const res = await api.ai.complete(req)
    if (cancelled || token.isCancellationRequested) return null
    const text = cleanCompletion(res.text)
    cache.set(key, { text, at: Date.now() }); pruneCache(useSettings.getState().settings?.tab.cacheSize ?? 200)
    return text
  } catch (e) {
    if (!cancelled) { console.debug('[tab] completion failed', e); useEditor.getState().setTabStatus('error'); setTimeout(() => { if (useEditor.getState().tabStatus === 'error') useEditor.getState().setTabStatus('idle') }, 4000) }
    return null
  } finally { sub.dispose(); if (inflight?.requestId === req.requestId) inflight = null }
}

function buildRequest(model: monaco.editor.ITextModel, position: monaco.Position, manual: boolean): CompletionRequest | null {
  const s = useSettings.getState().settings; if (!s) return null
  const language = model.getLanguageId()
  if (!manual && s.tab.disabledLanguages.includes(language)) return null
  const path = model.uri.path
  const lineContent = model.getLineContent(position.lineNumber)
  const linePrefix = lineContent.slice(0, position.column - 1)
  const lineSuffix = lineContent.slice(position.column - 1)
  if (!manual && !s.tab.suggestInComments && inLineComment(linePrefix, lineCommentPrefix(language))) return null
  const ctxLines = s.tab.contextLines || 80
  const startLine = Math.max(1, position.lineNumber - ctxLines)
  const endLine = Math.min(model.getLineCount(), position.lineNumber + Math.ceil(ctxLines / 2))
  const prefix = model.getValueInRange(new monaco.Range(startLine, 1, position.lineNumber, position.column))
  const suffix = model.getValueInRange(new monaco.Range(position.lineNumber, position.column, endLine, model.getLineMaxColumn(endLine)))
  const open = openFilePaths().filter((p) => p !== path).slice(0, 3).map((p) => { const m = getModel(p); return m ? { path: p, snippet: m.getValueInRange(new monaco.Range(1, 1, Math.min(60, m.getLineCount()), 1)) } : null }).filter(Boolean) as { path: string; snippet: string }[]
  const diag = diagnosticsSummary([path], 8)
  return {
    requestId: `tab-${++reqSeq}-${Date.now().toString(36)}`, path, language, prefix, suffix,
    cursorLine: position.lineNumber, cursorColumn: position.column,
    recentEdits: useEditor.getState().recentEdits.filter((e) => Date.now() - e.at < 10 * 60_000).slice(0, 8),
    diagnostics: diag ? diag.split('\n') : undefined, openFiles: open,
    multiline: lineSuffix.trim().length === 0, manual,
  }
}

const provider: monaco.languages.InlineCompletionsProvider = {
  displayName: 'Caret Tab',
  debounceDelayMs: useSettings.getState().settings?.tab.debounceMs ?? 250,
  onDidChangeInlineCompletions: changeEmitter.event,
  async provideInlineCompletions(model, position, context, token) {
    if (model.uri.scheme !== 'file') return { items: [] }
    const manual = context.triggerKind === monaco.languages.InlineCompletionTriggerKind.Explicit
    // 1) next-edit prediction (inline edit) if we have one for this version
    if (context.includeInlineEdits) {
      const ne = nextEdits.get(model.uri.path)
      if (ne && ne.version === model.getVersionId() && ne.result.found && ne.result.startLine && ne.result.endLine && ne.result.newText !== undefined) {
        const r = ne.result
        const range = new monaco.Range(r.startLine!, 1, r.endLine!, model.getLineMaxColumn(r.endLine!))
        const current = model.getValueInRange(range)
        if (current !== r.newText) {
          return { items: [{ insertText: r.newText!, range, isInlineEdit: true, showInlineEditMenu: true, jumpToPosition: { lineNumber: r.startLine!, column: 1 }, completeBracketPairs: false } as monaco.languages.InlineCompletion], enableForwardStability: true }
        }
      }
    }
    if (!context.includeInlineCompletions) return { items: [] }
    if (!tabEnabled() && !manual) return { items: [] }
    if (!manual && Date.now() < suspendedUntil) return { items: [] }
    if (context.selectedSuggestionInfo) return { items: [] } // suggest widget drives insert preview
    const req = buildRequest(model, position, manual); if (!req) return { items: [] }
    const lineContent = model.getLineContent(position.lineNumber)
    const linePrefix = lineContent.slice(0, position.column - 1)
    const lineSuffix = lineContent.slice(position.column - 1)
    let text = await fetchCompletion(req, token)
    if (!text || token.isCancellationRequested) { if (useEditor.getState().tabStatus === 'loading') useEditor.getState().setTabStatus('idle'); return { items: [] } }
    text = trimSuffixOverlap(text, lineSuffix)
    text = limitLines(text, useSettings.getState().settings?.tab.maxLines ?? 12)
    if (!req.multiline) text = text.split('\n')[0]
    if (isUselessCompletion(text, linePrefix)) { useEditor.getState().setTabStatus('idle'); return { items: [] } }
    useEditor.getState().setTabStatus('shown')
    return { items: [{ insertText: text, range: new monaco.Range(position.lineNumber, position.column, position.lineNumber, position.column), completeBracketPairs: true }], enableForwardStability: true }
  },
  handleItemDidShow() { useEditor.getState().setTabStatus('shown') },
  handleEndOfLifetime(_c, item, reason) {
    if (useEditor.getState().tabStatus === 'shown') useEditor.getState().setTabStatus('idle')
    // 0 = Accepted in InlineCompletionEndOfLifeReasonKind
    const kind = (reason as { kind: number }).kind
    if (kind === 0) {
      if ((item as monaco.languages.InlineCompletion).isInlineEdit) { nextEdits.clear(); scheduleNextEdit(400) }
      else scheduleNextEdit(150)
    }
  },
  disposeInlineCompletions() { /* nothing retained per list */ },
}

// ───────── next-edit prediction ─────────
let nextEditTimer: ReturnType<typeof setTimeout> | null = null
let nextEditSeq = 0
function nextEditEnabled(): boolean { const s = useSettings.getState().settings; return !!s && s.tab.enabled && s.tab.nextEditPrediction && s.beta.nextEditPrediction }
export function scheduleNextEdit(delay = 1200): void {
  if (!nextEditEnabled()) return
  if (nextEditTimer) clearTimeout(nextEditTimer)
  nextEditTimer = setTimeout(() => { nextEditTimer = null; void predictNextEdit() }, delay)
}
async function predictNextEdit(): Promise<void> {
  const ed = activeEditor(); const model = ed?.getModel(); const pos = ed?.getPosition()
  if (!ed || !model || !pos || model.uri.scheme !== 'file' || !ed.hasTextFocus()) return
  if (model.getValueLength() > 200_000) return
  const edits = useEditor.getState().recentEdits.filter((e) => e.path === model.uri.path && Date.now() - e.at < 5 * 60_000)
  if (!edits.length) return
  const version = model.getVersionId()
  const seq = ++nextEditSeq
  try {
    const res = await api.ai.nextEdit({ requestId: `ne-${seq}`, path: model.uri.path, language: model.getLanguageId(), content: model.getValue(), cursorLine: pos.lineNumber, recentEdits: edits.slice(0, 10) })
    if (seq !== nextEditSeq || model.isDisposed() || model.getVersionId() !== version) return
    if (!res.found || !res.startLine || !res.endLine || res.newText === undefined) { nextEdits.delete(model.uri.path); return }
    if (res.startLine < 1 || res.endLine > model.getLineCount() || res.endLine < res.startLine) return
    nextEdits.set(model.uri.path, { version, result: res })
    changeEmitter.fire()
    // Trigger rendering if nothing is showing
    ed.trigger('caret', 'editor.action.inlineSuggest.trigger', {})
  } catch (e) { console.debug('[tab] next edit failed', e) }
}
export function dismissNextEdit(path?: string): void { if (path) nextEdits.delete(path); else nextEdits.clear() }

export function registerInlineCompletions(): monaco.IDisposable {
  disposeInlineCompletions()
  disposed.push(monaco.languages.registerInlineCompletionsProvider({ scheme: 'file' }, provider))
  disposed.push({ dispose: busSubscribe<{ path: string }>('editor:changed', ({ path }) => { nextEdits.delete(path); scheduleNextEdit() }) })
  const unsub = useSettings.subscribe((s, prev) => {
    if (s.settings?.tab.debounceMs !== prev.settings?.tab.debounceMs) (provider as { debounceDelayMs?: number }).debounceDelayMs = s.settings?.tab.debounceMs ?? 250
    if (s.settings?.tab.enabled !== prev.settings?.tab.enabled) useEditor.getState().setTabStatus(s.settings?.tab.enabled ? 'idle' : 'off')
  })
  disposed.push({ dispose: unsub })
  useEditor.getState().setTabStatus(tabEnabled() ? 'idle' : 'off')
  return { dispose: disposeInlineCompletions }
}
function disposeInlineCompletions() { for (const d of disposed) d.dispose(); disposed = []; cache.clear(); nextEdits.clear() }

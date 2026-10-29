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
interface Inflight { key: string; requestId: string; promise: Promise<string | null>; adopt: () => void; release: () => void; kill: () => void }
const inflight = new Map<string, Inflight>() // key → shared request; obsolete ones die through Monaco's cancellation tokens
let reqSeq = 0
let disposed: monaco.IDisposable[] = []
const changeEmitter = new monaco.Emitter<void>()
let suspendedUntil = 0
/** Last provider round-trip, for the automation bridge (`window.__caret.tab`). */
export const tabDebug: { last: Record<string, unknown> | null; history: Record<string, unknown>[] } = { last: null, history: [] }

function tabEnabled(): boolean { return useSettings.getState().settings?.tab.enabled ?? true }
export function setTabEnabled(on: boolean): void { void useSettings.getState().update({ tab: { enabled: on } }); useEditor.getState().setTabStatus(on ? 'idle' : 'off') }
export function toggleTab(): void { setTabEnabled(!tabEnabled()) }
/** Temporarily suppress automatic completions (e.g. while Cmd+K is active). */
export function suspendTab(ms: number): void { suspendedUntil = Date.now() + ms }

function cacheKey(req: CompletionRequest): string { return `${req.path}|${hash(req.prefix.slice(-600))}|${hash(req.suffix.slice(0, 200))}` }
function pruneCache(max: number) { if (cache.size <= max) return; const entries = [...cache.entries()].sort((a, b) => a[1].at - b[1].at); for (const [k] of entries.slice(0, cache.size - max)) cache.delete(k) }

function fetchCompletion(req: CompletionRequest, token: monaco.CancellationToken): Promise<string | null> {
  const key = cacheKey(req)
  const hit = cache.get(key); if (hit && Date.now() - hit.at < 5 * 60_000) return hit.text ? Promise.resolve(hit.text) : Promise.resolve(null)
  // Monaco often cancels a call and re-issues it for the same context a few ms later (suggest selection settling, version bumps).
  // Sharing the in-flight request instead of cancelling it saves a full model round-trip.
  const cur = inflight.get(key)
  if (cur) {
    cur.adopt()
    const sub = token.onCancellationRequested(() => { sub.dispose(); cur.release() })
    return cur.promise
  }
  let interested = 1, killed = false, timer: ReturnType<typeof setTimeout> | null = null
  const kill = () => { if (killed) return; killed = true; if (timer) { clearTimeout(timer); timer = null }; if (inflight.get(key) === entry) inflight.delete(key); void api.ai.cancel(req.requestId).catch(() => {}) }
  const release = () => { interested -= 1; if (interested <= 0 && !timer && !killed) timer = setTimeout(() => { timer = null; if (interested <= 0) kill() }, 60) }
  const adopt = () => { interested += 1; if (timer) { clearTimeout(timer); timer = null } }
  const entry: Inflight = { key, requestId: req.requestId, adopt, release, kill, promise: Promise.resolve(null) }
  inflight.set(key, entry)
  const sub = token.onCancellationRequested(() => { sub.dispose(); release() })
  useEditor.getState().setTabStatus('loading')
  entry.promise = (async () => {
    try {
      const res = await api.ai.complete(req)
      if (killed) return null
      const text = cleanCompletion(res.text)
      if (text) { cache.set(key, { text, at: Date.now() }); pruneCache(useSettings.getState().settings?.tab.cacheSize ?? 200) }
      return text || null
    } catch (e) {
      if (!killed) { console.debug('[tab] completion failed', e); useEditor.getState().setTabStatus('error'); setTimeout(() => { if (useEditor.getState().tabStatus === 'error') useEditor.getState().setTabStatus('idle') }, 4000) }
      return null
    } finally { sub.dispose(); if (timer) { clearTimeout(timer); timer = null }; if (inflight.get(key) === entry) inflight.delete(key) }
  })()
  return entry.promise
}

/** A suggest-widget item treated as already accepted: Tab then continues *after* it (Copilot-style augmentation). */
export interface AppliedSuggestion { range: monaco.IRange; text: string }
interface BuiltRequest { req: CompletionRequest; linePrefix: string; lineSuffix: string }

export function buildRequest(model: monaco.editor.ITextModel, position: monaco.Position, manual: boolean, applied?: AppliedSuggestion): BuiltRequest | null {
  const s = useSettings.getState().settings; if (!s) return null
  const language = model.getLanguageId()
  if (!manual && s.tab.disabledLanguages.includes(language)) return null
  const path = model.uri.path
  const start = applied ? monaco.Range.lift(applied.range).getStartPosition() : position
  const end = applied ? monaco.Range.lift(applied.range).getEndPosition() : position
  const appliedText = applied?.text ?? ''
  const linePrefix = model.getLineContent(start.lineNumber).slice(0, start.column - 1) + appliedText
  const lineSuffix = model.getLineContent(end.lineNumber).slice(end.column - 1)
  if (!manual && !s.tab.suggestInComments && inLineComment(linePrefix, lineCommentPrefix(language))) return null
  const ctxLines = s.tab.contextLines || 80
  const startLine = Math.max(1, start.lineNumber - ctxLines)
  const endLine = Math.min(model.getLineCount(), end.lineNumber + Math.ceil(ctxLines / 2))
  const prefix = model.getValueInRange(new monaco.Range(startLine, 1, start.lineNumber, start.column)) + appliedText
  const suffix = model.getValueInRange(new monaco.Range(end.lineNumber, end.column, endLine, model.getLineMaxColumn(endLine)))
  const open = openFilePaths().filter((p) => p !== path).slice(0, 3).map((p) => { const m = getModel(p); return m ? { path: p, snippet: m.getValueInRange(new monaco.Range(1, 1, Math.min(60, m.getLineCount()), 1)) } : null }).filter(Boolean) as { path: string; snippet: string }[]
  const diag = diagnosticsSummary([path], 8)
  const req: CompletionRequest = {
    requestId: `tab-${++reqSeq}-${Date.now().toString(36)}`, path, language, prefix, suffix,
    cursorLine: start.lineNumber, cursorColumn: start.column + appliedText.length,
    recentEdits: useEditor.getState().recentEdits.filter((e) => Date.now() - e.at < 10 * 60_000).slice(0, 8),
    diagnostics: diag ? diag.split('\n') : undefined, openFiles: open,
    multiline: lineSuffix.trim().length === 0, manual,
  }
  return { req, linePrefix, lineSuffix }
}

/** Only plain, single-line suggest items are worth continuing; snippets with placeholders are not. */
export function augmentableSuggestion(info: monaco.languages.SelectedSuggestionInfo | null | undefined): AppliedSuggestion | null {
  // Monaco hands us snippet items with placeholders already resolved to their default text, so single-line snippets are fine too.
  if (!info || !info.text || info.text.includes('\n') || info.text.length > 200) return null
  const r = monaco.Range.lift(info.range); if (r.startLineNumber !== r.endLineNumber) return null
  return { range: info.range, text: info.text }
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
    // With the suggest widget open, continue after the highlighted item so the ghost text extends it (Monaco only shows an inline completion that augments the selection).
    const applied = augmentableSuggestion(context.selectedSuggestionInfo)
    if (context.selectedSuggestionInfo && !applied) { tabDebug.last = { at: Date.now(), skipped: 'suggest item not augmentable', info: context.selectedSuggestionInfo }; tabDebug.history.push(tabDebug.last); return { items: [] } }
    const built = buildRequest(model, position, manual, applied ?? undefined); if (!built) return { items: [] }
    const { req, linePrefix, lineSuffix } = built
    let text = await fetchCompletion(req, token)
    tabDebug.last = { at: Date.now(), path: req.path, prefixTail: req.prefix.slice(-80), applied: applied ?? null, raw: text, cancelled: token.isCancellationRequested }
    tabDebug.history.push(tabDebug.last); if (tabDebug.history.length > 12) tabDebug.history.shift()
    if (!text || token.isCancellationRequested) { if (useEditor.getState().tabStatus === 'loading') useEditor.getState().setTabStatus('idle'); return { items: [] } }
    text = trimSuffixOverlap(text, lineSuffix)
    text = limitLines(text, useSettings.getState().settings?.tab.maxLines ?? 12)
    if (!req.multiline) text = text.split('\n')[0]
    if (isUselessCompletion(text, linePrefix)) { tabDebug.last.useless = true; useEditor.getState().setTabStatus('idle'); return { items: [] } }
    tabDebug.last.final = applied ? applied.text + text : text
    useEditor.getState().setTabStatus('shown')
    if (applied) return { items: [{ insertText: applied.text + text, range: applied.range, completeBracketPairs: true }], enableForwardStability: true }
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

import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { monaco, initMonaco, editorOptionsFromSettings } from '@/lib/monaco/setup'
import { useSettings } from '@/store/settings'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import { useReview, useLatestReport, useSessionComments } from '@/store/review'
import { orderHunks, classifyHunk, fileKind, KIND_LABEL, findingsForPath, hashLine, reanchor, CHECK_TITLES, type HunkClass } from '@/lib/review'
import { Columns2, Rows2, Check, X, ArrowDown, ArrowUp, MessageSquarePlus, ShieldAlert, FoldVertical, UnfoldVertical } from 'lucide-react'
import type { DiffMeta } from '@/store/editor'
import type { ReviewComment } from '@/types'

/** Additions longer than this (pure insertions) are folded to a summary bar by default. */
const FOLD_THRESHOLD = 150
const FOLD_HEAD = 3

interface Hunk { index: number; startLine: number; endLine: number; added: number; deleted: number; origStart: number; origEnd: number; cls: HunkClass }
interface Fold { start: number; end: number }
type PathFinding = ReturnType<typeof findingsForPath>[number]

/** Per-comment anchor hashes (line text at creation) so comments follow their line when content shifts. Session-lifetime only. */
const anchorHashes = new Map<string, string>()
const samePath = (a: string | undefined, b: string | undefined) => !!a && !!b && (a === b || a.endsWith('/' + b) || b.endsWith('/' + a))

let commandsReady = false
function ensureMonacoCommands() {
  if (commandsReady) return
  commandsReady = true
  monaco.editor.registerCommand('caret.verify.applyFix', async (_acc: unknown, args: { reportId: string; findingId: string }) => {
    const pc = await useReview.getState().applyFix(args.reportId, args.findingId)
    if (pc) void runCommand('editor.openDiff', { path: pc.path, original: pc.before ?? '', modified: pc.after ?? '', title: `fix: ${pc.path.split('/').pop()}`, changeId: pc.changeId, sessionId: pc.sessionId })
    else void runCommand('app.notify', { level: 'warn', title: 'No automatic fix available', message: 'Use “Fix in chat” to ask the agent instead.' })
  })
  monaco.editor.registerCommand('caret.verify.fixInChat', (_acc: unknown, args: { path?: string; line?: number; message: string; rule?: string; suggestion?: string }) => {
    const where = args.path ? ` in ${args.path.split('/').pop()}${args.line ? `:${args.line}` : ''}` : ''
    void runCommand('chat.sendPrompt', { text: `Fix this verification finding${where}: ${args.message}${args.rule ? ` (${args.rule})` : ''}${args.suggestion ? `\n\nSuggested fix:\n\`\`\`\n${args.suggestion}\n\`\`\`` : ''}`, context: args.path ? [{ id: `vf_${Date.now()}`, kind: 'file', label: args.path.split('/').pop() ?? args.path, path: args.path, range: args.line ? { startLine: Math.max(1, args.line - 25), endLine: args.line + 25 } : undefined }] : [], mode: 'agent', submit: true })
  })
}

function findingMarkdown(f: PathFinding, reportId: string, path: string | undefined, lang: string): monaco.IMarkdownString {
  const enc = (o: unknown) => encodeURIComponent(JSON.stringify(o))
  const lines = [`**${f.severity.toUpperCase()}**${f.rule ? ` · \`${f.rule}\`` : ''} · ${CHECK_TITLES[f.checkKind] ?? f.checkKind}`, '', f.message]
  if (f.suggestion) lines.push('', '```' + lang, f.suggestion, '```')
  const actions: string[] = []
  if (f.fixable) actions.push(`[Apply fix](command:caret.verify.applyFix?${enc({ reportId, findingId: f.id })})`)
  actions.push(`[Fix in chat](command:caret.verify.fixInChat?${enc({ path: f.path ?? path, line: f.line, message: f.message, rule: f.rule, suggestion: f.suggestion })})`)
  if (f.url) actions.push(`[Reference](${f.url})`)
  lines.push('', actions.join(' · '))
  return { value: lines.join('\n'), isTrusted: true }
}

/** Owns a batch of view zones + their React roots on one editor; `set` replaces the batch atomically. */
class ZoneSet {
  private ids: string[] = []; private roots: Root[] = []; private ed: monaco.editor.ICodeEditor | null = null
  set(ed: monaco.editor.ICodeEditor, items: { afterLine: number; height: number; node: ReactNode; className?: string }[]) {
    this.clear(); this.ed = ed
    if (!items.length) return
    try {
      ed.changeViewZones((acc) => {
        for (const it of items) {
          const dom = document.createElement('div'); dom.className = it.className ?? ''
          const root = createRoot(dom); root.render(it.node); this.roots.push(root)
          this.ids.push(acc.addZone({ afterLineNumber: it.afterLine, heightInPx: it.height, domNode: dom, suppressMouseDown: false }))
        }
      })
    } catch { /* editor disposed */ }
  }
  clear() {
    const ed = this.ed, ids = this.ids, roots = this.roots; this.ids = []; this.roots = []
    if (ed && ids.length) { try { ed.changeViewZones((acc) => ids.forEach((id) => acc.removeZone(id))) } catch { /* disposed */ } }
    if (roots.length) setTimeout(() => roots.forEach((r) => { try { r.unmount() } catch { /* ignore */ } }), 0)
  }
}

function setHidden(ed: monaco.editor.ICodeEditor, ranges: monaco.IRange[]) {
  const fn = (ed as unknown as { setHiddenAreas?: (r: monaco.IRange[], source?: unknown) => void }).setHiddenAreas
  if (fn) { try { fn.call(ed, ranges, 'caret-fold') } catch { /* ignore */ } }
}

function CommentComposer({ line, onSubmit, onCancel }: { line: number; onSubmit: (text: string) => void; onCancel: () => void }) {
  const [text, setText] = useState('')
  const ref = useRef<HTMLTextAreaElement>(null)
  useEffect(() => { ref.current?.focus() }, [])
  return (
    <div className="h-full flex flex-col gap-1" onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); onCancel() } if ((e.metaKey || e.ctrlKey) && e.key === 'Enter' && text.trim()) { e.preventDefault(); onSubmit(text.trim()) } }}>
      <textarea ref={ref} value={text} onChange={(e) => setText(e.target.value)} placeholder={`Comment on line ${line}… (⌘⏎ to add, Esc to cancel)`} className="flex-1 w-full resize-none rounded border border-border bg-input px-2 py-1 text-[12px] outline-none focus:border-accent" />
      <div className="flex items-center gap-1 justify-end">
        <button type="button" onClick={onCancel} className="px-2 h-5 rounded border border-border text-[11px] hover:bg-hover">Cancel</button>
        <button type="button" disabled={!text.trim()} onClick={() => onSubmit(text.trim())} className="px-2 h-5 rounded bg-accent text-accent-fg text-[11px] disabled:opacity-50">Add comment</button>
      </div>
    </div>
  )
}

function CommentCard({ c, sessionId }: { c: ReviewComment; sessionId: string }) {
  const resolve = useReview((s) => s.resolveComment), del = useReview((s) => s.deleteComment)
  return (
    <div className={`h-full flex gap-2 ${c.resolved ? 'opacity-60' : ''}`}>
      <span className={`mt-0.5 shrink-0 px-1 rounded text-[9.5px] uppercase font-semibold ${c.author === 'bot' ? 'bg-info/20 text-info' : 'bg-accent/20 text-accent'}`}>{c.author === 'bot' ? 'agent' : 'you'}</span>
      <div className="flex-1 min-w-0 text-[12px] whitespace-pre-wrap break-words overflow-auto selectable">{c.text}</div>
      <div className="flex items-start gap-1 shrink-0">
        <button type="button" onClick={() => void resolve(sessionId, c.id, !c.resolved)} className="px-1.5 h-5 rounded border border-border text-[10.5px] hover:bg-hover">{c.resolved ? 'Reopen' : 'Resolve'}</button>
        <button type="button" onClick={() => void del(sessionId, c.id)} title="Delete" className="w-5 h-5 flex items-center justify-center rounded hover:bg-hover text-muted hover:text-danger"><X size={11} /></button>
      </div>
    </div>
  )
}

function FoldBar({ fold, kind, expanded, summary, onToggle, onSummarize, sideBySide }: { fold: Fold; kind: string; expanded: boolean; summary?: { text: string; loading: boolean; error?: string }; onToggle: () => void; onSummarize: () => void; sideBySide: boolean }) {
  const n = fold.end - fold.start + 1 - FOLD_HEAD
  return (
    <div className="caret-fold-zone">
      {!summary ? (
        <>
          <span>{expanded || sideBySide ? `${n} added lines` : `${n} added lines folded`} · new {kind}</span>
          <button type="button" onClick={onToggle}>{sideBySide ? 'Fold (inline view)' : expanded ? 'Fold' : 'Show full'}</button>
          <button type="button" onClick={onSummarize}>Summarize as pseudocode</button>
        </>
      ) : (
        <>
          <pre>{summary.loading && !summary.text ? 'Summarizing…' : summary.error ? `Summary failed: ${summary.error}` : summary.text}</pre>
          <div className="flex flex-col gap-1 shrink-0 self-start"><button type="button" onClick={onToggle}>{sideBySide ? 'Fold (inline)' : expanded ? 'Fold' : 'Show full'}</button></div>
        </>
      )}
    </div>
  )
}

/** Read-only diff view: logically ordered hunk navigation, gutter review comments, verification findings and folded large additions; agent changes get Accept/Reject. */
export function DiffEditor({ meta, onDecision }: { meta: DiffMeta; onDecision?: () => void }) {
  const hostRef = useRef<HTMLDivElement>(null)
  const edRef = useRef<monaco.editor.IStandaloneDiffEditor | null>(null)
  const hunksRef = useRef<Hunk[]>([]); const navIdx = useRef(-1)
  const autoInline = useRef(false)
  const [sideBySide, setSideBySide] = useState(true)
  const [stats, setStats] = useState({ add: 0, del: 0, changes: 0 })
  const [edVersion, setEdVersion] = useState(0)
  const [navState, setNavState] = useState<{ idx: number; n: number; cls?: HunkClass }>({ idx: -1, n: 0 })
  const [folds, setFolds] = useState<Fold[]>([])
  const [expanded, setExpanded] = useState<Record<number, boolean>>({})
  const [summaries, setSummaries] = useState<Record<number, { text: string; loading: boolean; error?: string }>>({})
  const [composer, setComposer] = useState<number | null>(null)
  const ui = useSettings((s) => s.settings?.ui)
  const commentsEnabled = !!(meta.sessionId && meta.path)
  const report = useLatestReport(meta.sessionId)
  const findings = useMemo(() => (meta.path ? findingsForPath(report, meta.path) : []), [report, meta.path])
  const allComments = useSessionComments(meta.sessionId)
  const fileComments = useMemo(() => allComments.filter((c) => samePath(c.path, meta.path)), [allComments, meta.path])
  const modLines = useMemo(() => meta.modified.split('\n'), [meta.modified])
  const kind = fileKind(meta.path ?? meta.title ?? '')
  const lang = meta.language ?? 'plaintext'
  const foldZones = useRef(new ZoneSet()), commentZones = useRef(new ZoneSet()), composerZone = useRef(new ZoneSet())
  const hoverDeco = useRef<monaco.editor.IEditorDecorationsCollection | null>(null)
  const findingDeco = useRef<monaco.editor.IEditorDecorationsCollection | null>(null)
  const commentDeco = useRef<monaco.editor.IEditorDecorationsCollection | null>(null)

  const nav = useCallback((dir: 1 | -1) => {
    const ed = edRef.current, hs = hunksRef.current; if (!ed || !hs.length) return
    navIdx.current = (navIdx.current + dir + hs.length) % hs.length
    const h = hs[navIdx.current]
    const target = h.added ? ed.getModifiedEditor() : ed.getOriginalEditor()
    const line = h.added ? h.startLine : h.origStart
    target.revealLineInCenter(line); target.setPosition({ lineNumber: line, column: 1 }); target.focus()
    setNavState({ idx: navIdx.current, n: hs.length, cls: h.cls })
  }, [])

  useEffect(() => {
    initMonaco(); ensureMonacoCommands()
    const el = hostRef.current!
    const ed = monaco.editor.createDiffEditor(el, { ...editorOptionsFromSettings(useSettings.getState().settings?.ui), readOnly: true, originalEditable: false, renderSideBySide: sideBySide, automaticLayout: true, renderOverviewRuler: true, diffWordWrap: 'off', renderIndicators: true, ignoreTrimWhitespace: false, useInlineViewWhenSpaceIsLimited: true, hideUnchangedRegions: { enabled: meta.original.length > 20_000 } })
    edRef.current = ed
    const original = monaco.editor.createModel(meta.original, lang)
    const modified = monaco.editor.createModel(meta.modified, lang)
    ed.setModel({ original, modified })
    const modEd = ed.getModifiedEditor(), origEd = ed.getOriginalEditor()
    modEd.updateOptions({ glyphMargin: true })
    hoverDeco.current = modEd.createDecorationsCollection(); findingDeco.current = modEd.createDecorationsCollection(); commentDeco.current = modEd.createDecorationsCollection()
    const origLines = meta.original.split('\n'), mLines = meta.modified.split('\n')
    const d = ed.onDidUpdateDiff(() => {
      const changes = ed.getLineChanges() ?? []
      let add = 0, del = 0
      const raw = changes.map((c, i) => {
        const added = c.modifiedEndLineNumber ? c.modifiedEndLineNumber - c.modifiedStartLineNumber + 1 : 0
        const deleted = c.originalEndLineNumber ? c.originalEndLineNumber - c.originalStartLineNumber + 1 : 0
        add += added; del += deleted
        return { index: i, startLine: c.modifiedStartLineNumber, endLine: added ? c.modifiedEndLineNumber : c.modifiedStartLineNumber, added, deleted, origStart: c.originalStartLineNumber, origEnd: c.originalEndLineNumber }
      })
      const getLines = (h: typeof raw[number]) => h.added ? mLines.slice(h.startLine - 1, h.endLine) : origLines.slice(h.origStart - 1, h.origEnd)
      hunksRef.current = orderHunks(raw, getLines).map((h) => ({ ...h, cls: classifyHunk(getLines(h)) }))
      navIdx.current = -1
      setStats({ add, del, changes: changes.length }); setNavState({ idx: -1, n: raw.length })
      const fl = raw.filter((h) => h.deleted === 0 && h.added > FOLD_THRESHOLD).map((h) => ({ start: h.startLine, end: h.endLine }))
      setFolds((prev) => (prev.length === fl.length && prev.every((p, i) => p.start === fl[i].start && p.end === fl[i].end) ? prev : fl))
      if (fl.length && !autoInline.current) { autoInline.current = true; setSideBySide(false) }
    })
    // ⌥↓ / ⌥↑ walk hunks in logical order (types → exports → functions → other → tests)
    for (const e of [modEd, origEd]) { e.addCommand(monaco.KeyMod.Alt | monaco.KeyCode.DownArrow, () => nav(1)); e.addCommand(monaco.KeyMod.Alt | monaco.KeyCode.UpArrow, () => nav(-1)) }
    // gutter “+” on hover, click to comment
    const subs: monaco.IDisposable[] = []
    if (meta.sessionId && meta.path) {
      const T = monaco.editor.MouseTargetType
      subs.push(modEd.onMouseMove((e) => {
        const ln = e.target.position?.lineNumber
        const inGutter = e.target.type === T.GUTTER_GLYPH_MARGIN || e.target.type === T.GUTTER_LINE_NUMBERS || e.target.type === T.GUTTER_LINE_DECORATIONS
        hoverDeco.current?.set(inGutter && ln ? [{ range: new monaco.Range(ln, 1, ln, 1), options: { glyphMarginClassName: 'caret-comment-add-glyph', glyphMarginHoverMessage: { value: 'Add review comment' } } }] : [])
      }))
      subs.push(modEd.onMouseLeave(() => hoverDeco.current?.set([])))
      subs.push(modEd.onMouseDown((e) => { if (e.target.type === T.GUTTER_GLYPH_MARGIN && e.target.position) { e.event.preventDefault(); setComposer(e.target.position.lineNumber) } }))
    }
    setEdVersion((v) => v + 1)
    return () => { d.dispose(); subs.forEach((s) => s.dispose()); foldZones.current.clear(); commentZones.current.clear(); composerZone.current.clear(); ed.dispose(); original.dispose(); modified.dispose(); edRef.current = null }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [meta.original, meta.modified, meta.language])
  useEffect(() => { edRef.current?.updateOptions({ renderSideBySide: sideBySide }) }, [sideBySide])
  useEffect(() => { const ed = edRef.current; if (!ed) return; ed.updateOptions({ ...editorOptionsFromSettings(ui), readOnly: true }); ed.getModifiedEditor().updateOptions({ glyphMargin: true }) }, [ui, edVersion])

  // verification findings → glyphs + hover with Apply fix / Fix in chat command links
  useEffect(() => {
    const ed = edRef.current; if (!ed || !findingDeco.current) return
    const reportId = report?.id ?? ''
    findingDeco.current.set(findings.filter((f) => f.line).map((f) => {
      const hi = f.severity === 'critical' || f.severity === 'high'
      const md = findingMarkdown(f, reportId, meta.path, lang)
      return { range: new monaco.Range(f.line!, 1, f.endLine ?? f.line!, 1), options: { isWholeLine: true, className: hi ? 'caret-verify-line-high' : 'caret-verify-line', glyphMarginClassName: `caret-verify-glyph caret-verify-${f.severity}`, glyphMarginHoverMessage: md, hoverMessage: md, overviewRuler: { color: hi ? 'var(--color-danger)' : 'var(--color-warning)', position: monaco.editor.OverviewRulerLane.Right } } }
    }))
  }, [findings, report?.id, meta.path, lang, edVersion])

  // folded large additions (inline view only; side-by-side keeps the bar but not the hidden lines)
  useEffect(() => {
    const ed = edRef.current; if (!ed) return
    const modEd = ed.getModifiedEditor()
    const lh = modEd.getOption(monaco.editor.EditorOption.lineHeight)
    const hidden = sideBySide ? [] : folds.filter((f) => !expanded[f.start]).map((f) => new monaco.Range(f.start + FOLD_HEAD, 1, f.end, 1))
    setHidden(modEd, hidden)
    const summarize = (f: Fold) => {
      if (summaries[f.start]) return
      setSummaries((s) => ({ ...s, [f.start]: { text: '', loading: true } }))
      const selected = modLines.slice(f.start - 1, f.end).join('\n')
      let acc = ''
      void api.ai.inlineEdit({ requestId: `fold_${Date.now()}`, path: meta.path ?? '', language: lang, fullText: meta.modified, startLine: f.start, endLine: f.end, selectedText: selected.length > 24_000 ? selected.slice(0, 24_000) : selected, instruction: 'Summarize this newly added code as terse pseudocode (max 12 short lines): what it defines, its inputs/outputs and side effects. No prose, no markdown fences.', context: [], mode: 'question' }, (e) => {
        if (e.type === 'delta') { acc += e.text; setSummaries((s) => ({ ...s, [f.start]: { text: acc.replace(/^\*\[[^\]]*\][^\n]*\*\s*$/gm, '').trim(), loading: true } })) }
        else if (e.type === 'done') setSummaries((s) => ({ ...s, [f.start]: { text: (e.text || acc).replace(/^\*\[[^\]]*\][^\n]*\*\s*$/gm, '').replace(/^```[^\n]*\n?|\n?```$/g, '').trim(), loading: false } }))
        else if (e.type === 'error') setSummaries((s) => ({ ...s, [f.start]: { text: '', loading: false, error: e.message } }))
      }).catch((err: unknown) => setSummaries((s) => ({ ...s, [f.start]: { text: '', loading: false, error: String((err as Error).message ?? err) } })))
    }
    foldZones.current.set(modEd, folds.map((f) => {
      const sm = summaries[f.start]
      const h = sm ? Math.min(280, 28 + 16 * Math.max(1, (sm.text || 'x').split('\n').length)) : 26
      return { afterLine: f.start + FOLD_HEAD - 1, height: h, node: <FoldBar fold={f} kind={KIND_LABEL[kind].toLowerCase()} expanded={!!expanded[f.start]} summary={sm} sideBySide={sideBySide} onToggle={() => { if (sideBySide) { setSideBySide(false); setExpanded((x) => ({ ...x, [f.start]: false })) } else setExpanded((x) => ({ ...x, [f.start]: !x[f.start] })) }} onSummarize={() => summarize(f)} /> }
    }))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [folds, expanded, summaries, sideBySide, edVersion, modLines])

  // review comments → re-anchored view zones + line decorations; composer zone
  useEffect(() => {
    const ed = edRef.current; if (!ed || !meta.sessionId) return
    const modEd = ed.getModifiedEditor(), origEd = ed.getOriginalEditor()
    const origLines = meta.original.split('\n')
    const placed = fileComments.map((c) => {
      const lines = c.side === 'before' ? origLines : modLines
      let hash = anchorHashes.get(c.id); if (!hash) { hash = hashLine(lines[c.line - 1] ?? ''); anchorHashes.set(c.id, hash) }
      return { c, line: reanchor(lines, { line: c.line, hash }) ?? c.line }
    })
    const height = (c: ReviewComment) => Math.min(160, 30 + 16 * Math.max(1, Math.ceil(c.text.length / 90) + (c.text.match(/\n/g)?.length ?? 0)))
    const after = placed.filter((p) => p.c.side === 'after' || !sideBySide)
    commentZones.current.set(modEd, after.map((p) => ({ afterLine: p.line, height: height(p.c), className: 'caret-comment-zone', node: <CommentCard c={p.c} sessionId={meta.sessionId!} /> })))
    commentDeco.current?.set(after.filter((p) => !p.c.resolved).map((p) => ({ range: new monaco.Range(p.line, 1, p.line, 1), options: { isWholeLine: true, className: 'caret-comment-line', glyphMarginClassName: 'caret-comment-glyph', glyphMarginHoverMessage: { value: p.c.text } } })))
    const before = sideBySide ? placed.filter((p) => p.c.side === 'before') : []
    const origZones = new ZoneSet(); origZones.set(origEd, before.map((p) => ({ afterLine: p.line, height: height(p.c), className: 'caret-comment-zone', node: <CommentCard c={p.c} sessionId={meta.sessionId!} /> })))
    return () => origZones.clear()
  }, [fileComments, modLines, meta.original, meta.sessionId, sideBySide, edVersion])
  useEffect(() => {
    const ed = edRef.current; if (!ed) return
    if (composer === null || !meta.sessionId || !meta.path) { composerZone.current.clear(); return }
    const sid = meta.sessionId, path = meta.path, line = composer
    composerZone.current.set(ed.getModifiedEditor(), [{ afterLine: line, height: 92, className: 'caret-comment-zone', node: <CommentComposer line={line} onCancel={() => setComposer(null)} onSubmit={(text) => { setComposer(null); void useReview.getState().addComment(sid, { path, line, side: 'after', text, changeId: meta.changeId }).then((saved) => anchorHashes.set(saved.id, hashLine(modLines[line - 1] ?? ''))) }} /> }])
    ed.getModifiedEditor().revealLineInCenterIfOutsideViewport(line)
  }, [composer, meta.sessionId, meta.path, meta.changeId, modLines, edVersion])

  const decide = async (accept: boolean) => {
    if (!meta.changeId || !meta.sessionId) return
    try { if (accept) await api.chat.acceptChanges(meta.sessionId, [meta.changeId]); else await api.chat.rejectChanges(meta.sessionId, [meta.changeId]); onDecision?.() }
    catch (e) { void runCommand('app.notify', { level: 'error', title: accept ? 'Accept failed' : 'Reject failed', message: String((e as Error).message ?? e) }) }
  }
  const openCount = fileComments.filter((c) => !c.resolved).length
  const allFolded = folds.length > 0 && folds.every((f) => !expanded[f.start]) && !sideBySide
  return (
    <div className="h-full w-full flex flex-col bg-editor" data-diff-editor>
      <div className="h-7 shrink-0 flex items-center gap-2 px-2 border-b border-border text-xs text-muted">
        <span className="truncate">{meta.originalLabel ?? 'Original'} ↔ {meta.modifiedLabel ?? 'Modified'}</span>
        <span className="text-success">+{stats.add}</span><span className="text-danger">−{stats.del}</span>
        {meta.path && kind !== 'other' && <span className="px-1.5 h-4 rounded bg-input text-[10px] uppercase tracking-wide" title={`${KIND_LABEL[kind]} file`}>{kind}</span>}
        {findings.length > 0 && <span className="flex items-center gap-1 px-1.5 h-4 rounded bg-warning/15 text-warning text-[10.5px]" title={findings.map((f) => `[${f.severity}] ${f.message}`).join('\n')}><ShieldAlert size={10} /> {findings.length} finding{findings.length === 1 ? '' : 's'}</span>}
        {openCount > 0 && <span className="text-[10.5px] text-accent">{openCount} comment{openCount === 1 ? '' : 's'}</span>}
        <span className="flex-1" />
        {navState.n > 0 && <span className="text-[10.5px] tabular-nums" title="Hunks are visited in logical order: types → exports → functions → other → tests">{navState.idx >= 0 ? `${navState.idx + 1}/${navState.n}` : navState.n}{navState.cls ? ` · ${navState.cls}` : ''}</span>}
        <button className="p-1 rounded hover:bg-hover" title="Previous change (⌥↑)" onClick={() => nav(-1)}><ArrowUp size={13} /></button>
        <button className="p-1 rounded hover:bg-hover" title="Next change (⌥↓)" onClick={() => nav(1)}><ArrowDown size={13} /></button>
        {folds.length > 0 && <button className="p-1 rounded hover:bg-hover" title={allFolded ? 'Show all folded additions' : 'Fold large additions'} onClick={() => { if (allFolded) setExpanded(Object.fromEntries(folds.map((f) => [f.start, true]))); else { setSideBySide(false); setExpanded({}) } }}>{allFolded ? <UnfoldVertical size={13} /> : <FoldVertical size={13} />}</button>}
        {commentsEnabled && <button className="p-1 rounded hover:bg-hover" title="Comment on current line" onClick={() => { const ln = edRef.current?.getModifiedEditor().getPosition()?.lineNumber ?? 1; setComposer(ln) }}><MessageSquarePlus size={13} /></button>}
        <button className="p-1 rounded hover:bg-hover" title={sideBySide ? 'Inline view' : 'Side by side'} onClick={() => setSideBySide((v) => !v)}>{sideBySide ? <Rows2 size={13} /> : <Columns2 size={13} />}</button>
        {meta.changeId && <>
          <button className="ml-1 px-2 py-0.5 rounded bg-accent text-accent-fg flex items-center gap-1" onClick={() => void decide(true)}><Check size={12} />Accept</button>
          <button className="px-2 py-0.5 rounded bg-active hover:bg-hover flex items-center gap-1" onClick={() => void decide(false)}><X size={12} />Reject</button>
        </>}
      </div>
      <div ref={hostRef} className="flex-1 min-h-0" />
    </div>
  )
}

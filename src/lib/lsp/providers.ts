/**
 * Monaco language feature providers backed by LSP clients. Registered once per language id.
 */
import { monaco } from '@/lib/monaco/setup'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import type { LspClient } from './client'
import type * as L from './types'
import { toMonacoRange, toLspRange, toLspPosition, toMonacoPosition, pathToUri, uriToPath, COMPLETION_KIND_MAP, SYMBOL_KIND_MAP, hoverToMarkdown, docToMarkdown } from './convert'

type Resolver = (model: monaco.editor.ITextModel) => LspClient | null
const docId = (model: monaco.editor.ITextModel) => ({ uri: pathToUri(model.uri.path) })
const posParams = (model: monaco.editor.ITextModel, p: monaco.Position): L.TextDocumentPositionParams => ({ textDocument: docId(model), position: toLspPosition(p) })

function wordRange(model: monaco.editor.ITextModel, position: monaco.Position): monaco.IRange {
  const w = model.getWordUntilPosition(position)
  return { startLineNumber: position.lineNumber, startColumn: w.startColumn, endLineNumber: position.lineNumber, endColumn: w.endColumn }
}

function completionItemToMonaco(item: L.CompletionItem, list: L.CompletionList | null, model: monaco.editor.ITextModel, position: monaco.Position): monaco.languages.CompletionItem & { __lsp: L.CompletionItem } {
  const label = typeof item.label === 'string' ? item.label : item.label.label
  const labelDetail = typeof item.label === 'string' ? undefined : item.label
  let range: monaco.IRange | monaco.languages.CompletionItemRanges = wordRange(model, position)
  let insertText = item.insertText ?? label
  const te = item.textEdit ?? (list?.itemDefaults?.editRange ? { newText: item.insertText ?? label, ...(('insert' in list.itemDefaults.editRange) ? list.itemDefaults.editRange : { range: list.itemDefaults.editRange }) } as L.TextEdit : undefined)
  if (te) {
    insertText = te.newText
    if ('range' in te) range = toMonacoRange(te.range)
    else { const t = te as { insert: L.LspRange; replace: L.LspRange }; range = { insert: toMonacoRange(t.insert), replace: toMonacoRange(t.replace) } }
  }
  const fmt = item.insertTextFormat ?? list?.itemDefaults?.insertTextFormat
  return {
    label: labelDetail ? { label, detail: labelDetail.detail, description: labelDetail.description } : label,
    kind: (COMPLETION_KIND_MAP[item.kind ?? 1] ?? 18) as monaco.languages.CompletionItemKind,
    detail: item.detail, documentation: docToMarkdown(item.documentation) ? { value: docToMarkdown(item.documentation)! } : undefined,
    sortText: item.sortText, filterText: item.filterText, preselect: item.preselect, insertText, range,
    insertTextRules: fmt === 2 ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet : undefined,
    commitCharacters: item.commitCharacters, tags: item.tags?.includes(1) || item.deprecated ? [monaco.languages.CompletionItemTag.Deprecated] : undefined,
    additionalTextEdits: item.additionalTextEdits?.map((e) => ({ range: toMonacoRange(e.range), text: e.newText })),
    command: item.command ? { id: 'caret.lsp.executeCommand', title: item.command.title, arguments: [item.command] } : undefined,
    __lsp: item,
  }
}

export function registerLspProviders(language: string, resolve: Resolver): monaco.IDisposable[] {
  const d: monaco.IDisposable[] = []
  d.push(monaco.languages.registerCompletionItemProvider(language, {
    triggerCharacters: ['.', ':', '<', '"', "'", '/', '@', '#', '(', ' ', '{', '[', ',', '-', '>', '='],
    async provideCompletionItems(model, position, context, token) {
      const c = resolve(model); if (!c || !c.capabilities.completionProvider) return null
      const trig = c.capabilities.completionProvider.triggerCharacters ?? []
      if (context.triggerKind === monaco.languages.CompletionTriggerKind.TriggerCharacter && context.triggerCharacter && !trig.includes(context.triggerCharacter)) {
        // not a trigger character for this server → let Monaco re-request on word typing
        if (!/[\w$]/.test(context.triggerCharacter)) return null
      }
      const res = await c.request<L.CompletionList | L.CompletionItem[] | null>('textDocument/completion', { ...posParams(model, position), context: { triggerKind: context.triggerKind === monaco.languages.CompletionTriggerKind.TriggerCharacter && trig.includes(context.triggerCharacter ?? '') ? 2 : context.triggerKind === monaco.languages.CompletionTriggerKind.TriggerForIncompleteCompletions ? 3 : 1, triggerCharacter: context.triggerCharacter } })
      if (token.isCancellationRequested || !res) return null
      const list = Array.isArray(res) ? null : res
      const items = Array.isArray(res) ? res : res.items
      return { suggestions: items.map((it) => completionItemToMonaco(it, list, model, position)), incomplete: list?.isIncomplete ?? false }
    },
    async resolveCompletionItem(item, _token) {
      const it = item as monaco.languages.CompletionItem & { __lsp?: L.CompletionItem; __client?: LspClient }
      const lsp = it.__lsp; if (!lsp) return item
      const model = monaco.editor.getModels().find((m) => true); if (!model) return item
      // find a client that can resolve: any running client for this language
      const c = resolveAny(resolve, language); if (!c?.capabilities.completionProvider?.resolveProvider) return item
      try {
        const r = await c.request<L.CompletionItem>('textDocument/completion/resolve', lsp, 5000)
        if (r) { it.detail = r.detail ?? it.detail; const doc = docToMarkdown(r.documentation); if (doc) it.documentation = { value: doc }; if (r.additionalTextEdits) it.additionalTextEdits = r.additionalTextEdits.map((e) => ({ range: toMonacoRange(e.range), text: e.newText })) }
      } catch { /* ignore */ }
      return it
    },
  }))
  d.push(monaco.languages.registerHoverProvider(language, {
    async provideHover(model, position, token) {
      const c = resolve(model); if (!c?.capabilities.hoverProvider) return null
      const h = await c.request<L.Hover | null>('textDocument/hover', posParams(model, position), 8000)
      if (!h || token.isCancellationRequested) return null
      const contents = hoverToMarkdown(h.contents).map((value) => ({ value, isTrusted: true }))
      return contents.length ? { contents, range: h.range ? toMonacoRange(h.range) : undefined } : null
    },
  }))
  d.push(monaco.languages.registerSignatureHelpProvider(language, {
    signatureHelpTriggerCharacters: ['(', ',', '<'], signatureHelpRetriggerCharacters: [')'],
    async provideSignatureHelp(model, position, token, context) {
      const c = resolve(model); if (!c?.capabilities.signatureHelpProvider) return null
      const r = await c.request<L.SignatureHelp | null>('textDocument/signatureHelp', { ...posParams(model, position), context: { triggerKind: context.triggerKind, triggerCharacter: context.triggerCharacter, isRetrigger: context.isRetrigger } }, 8000)
      if (!r || token.isCancellationRequested) return null
      return { dispose() {}, value: { activeSignature: r.activeSignature ?? 0, activeParameter: r.activeParameter ?? 0, signatures: r.signatures.map((s) => ({ label: s.label, documentation: docToMarkdown(s.documentation) ? { value: docToMarkdown(s.documentation)! } : undefined, activeParameter: s.activeParameter, parameters: (s.parameters ?? []).map((p) => ({ label: p.label, documentation: docToMarkdown(p.documentation) ? { value: docToMarkdown(p.documentation)! } : undefined })) })) } }
    },
  }))
  const locProvider = (method: string, cap: keyof L.ServerCapabilities) => async (model: monaco.editor.ITextModel, position: monaco.Position, token: monaco.CancellationToken) => {
    const c = resolve(model); if (!c || !c.capabilities[cap]) return null
    const r = await c.request<L.Location | L.Location[] | L.LocationLink[] | null>(method, { ...posParams(model, position), ...(method === 'textDocument/references' ? { context: { includeDeclaration: true } } : {}) }, 15000)
    if (!r || token.isCancellationRequested) return null
    return toLocations(r)
  }
  d.push(monaco.languages.registerDefinitionProvider(language, { provideDefinition: locProvider('textDocument/definition', 'definitionProvider') }))
  d.push(monaco.languages.registerTypeDefinitionProvider(language, { provideTypeDefinition: locProvider('textDocument/typeDefinition', 'typeDefinitionProvider') }))
  d.push(monaco.languages.registerImplementationProvider(language, { provideImplementation: locProvider('textDocument/implementation', 'implementationProvider') }))
  d.push(monaco.languages.registerReferenceProvider(language, { provideReferences: (m, p, _ctx, t) => locProvider('textDocument/references', 'referencesProvider')(m, p, t) as Promise<monaco.languages.Location[]> }))
  d.push(monaco.languages.registerDocumentHighlightProvider(language, {
    async provideDocumentHighlights(model, position, token) {
      const c = resolve(model); if (!c?.capabilities.documentHighlightProvider) return null
      const r = await c.request<L.DocumentHighlight[] | null>('textDocument/documentHighlight', posParams(model, position), 5000)
      if (!r || token.isCancellationRequested) return null
      return r.map((h) => ({ range: toMonacoRange(h.range), kind: (h.kind ?? 1) - 1 as monaco.languages.DocumentHighlightKind }))
    },
  }))
  d.push(monaco.languages.registerDocumentSymbolProvider(language, {
    displayName: 'LSP',
    async provideDocumentSymbols(model, token) {
      const c = resolve(model); if (!c?.capabilities.documentSymbolProvider) return null
      const r = await c.request<L.DocumentSymbol[] | L.SymbolInformation[] | null>('textDocument/documentSymbol', { textDocument: docId(model) }, 15000)
      if (!r || token.isCancellationRequested) return null
      return symbolsToMonaco(r)
    },
  }))
  d.push(monaco.languages.registerRenameProvider(language, {
    async provideRenameEdits(model, position, newName, token) {
      const c = resolve(model); if (!c?.capabilities.renameProvider) return null
      const r = await c.request<L.WorkspaceEdit | null>('textDocument/rename', { ...posParams(model, position), newName }, 20000)
      if (!r || token.isCancellationRequested) return null
      return workspaceEditToMonaco(r)
    },
    async resolveRenameLocation(model, position, token) {
      const c = resolve(model); const cap = c?.capabilities.renameProvider
      if (!c || !cap || typeof cap !== 'object' || !cap.prepareProvider) { const w = model.getWordAtPosition(position); return w ? { range: { startLineNumber: position.lineNumber, endLineNumber: position.lineNumber, startColumn: w.startColumn, endColumn: w.endColumn }, text: w.word } : { range: new monaco.Range(position.lineNumber, position.column, position.lineNumber, position.column), text: '', rejectReason: 'Nothing to rename here' } }
      const r = await c.request<L.LspRange | { range: L.LspRange; placeholder: string } | { defaultBehavior: boolean } | null>('textDocument/prepareRename', posParams(model, position), 8000)
      if (token.isCancellationRequested) return null
      if (!r) return { range: new monaco.Range(position.lineNumber, position.column, position.lineNumber, position.column), text: '', rejectReason: 'This element cannot be renamed' }
      if ('placeholder' in r) return { range: toMonacoRange(r.range), text: r.placeholder }
      if ('start' in r) return { range: toMonacoRange(r), text: model.getValueInRange(toMonacoRange(r)) }
      const w = model.getWordAtPosition(position); return w ? { range: { startLineNumber: position.lineNumber, endLineNumber: position.lineNumber, startColumn: w.startColumn, endColumn: w.endColumn }, text: w.word } : null
    },
  }))
  d.push(monaco.languages.registerDocumentFormattingEditProvider(language, {
    displayName: 'LSP',
    async provideDocumentFormattingEdits(model, options, token) {
      const c = resolve(model); if (!c?.capabilities.documentFormattingProvider) return null
      const r = await c.request<L.TextEdit[] | null>('textDocument/formatting', { textDocument: docId(model), options: { tabSize: options.tabSize, insertSpaces: options.insertSpaces, trimTrailingWhitespace: true, insertFinalNewline: true } }, 20000)
      if (!r || token.isCancellationRequested) return null
      return r.map((e) => ({ range: toMonacoRange(e.range), text: e.newText }))
    },
  }))
  d.push(monaco.languages.registerDocumentRangeFormattingEditProvider(language, {
    displayName: 'LSP',
    async provideDocumentRangeFormattingEdits(model, range, options, token) {
      const c = resolve(model); if (!c?.capabilities.documentRangeFormattingProvider) return null
      const r = await c.request<L.TextEdit[] | null>('textDocument/rangeFormatting', { textDocument: docId(model), range: toLspRange(range), options: { tabSize: options.tabSize, insertSpaces: options.insertSpaces } }, 20000)
      if (!r || token.isCancellationRequested) return null
      return r.map((e) => ({ range: toMonacoRange(e.range), text: e.newText }))
    },
  }))
  d.push(monaco.languages.registerCodeActionProvider(language, {
    async provideCodeActions(model, range, context, token) {
      const c = resolve(model); if (!c?.capabilities.codeActionProvider) return null
      const diagnostics = context.markers.map((m) => ({ range: toLspRange(m), message: m.message, severity: m.severity === 8 ? 1 : m.severity === 4 ? 2 : m.severity === 2 ? 3 : 4, code: typeof m.code === 'object' ? m.code?.value : m.code, source: m.source }))
      const r = await c.request<(L.CodeAction | L.Command)[] | null>('textDocument/codeAction', { textDocument: docId(model), range: toLspRange(range), context: { diagnostics, only: context.only ? [context.only] : undefined, triggerKind: context.trigger } }, 10000)
      if (!r || token.isCancellationRequested) return null
      const actions: monaco.languages.CodeAction[] = r.map((a) => {
        if ('command' in a && typeof a.command === 'string') { const cmd = a as L.Command; return { title: cmd.title, command: { id: 'caret.lsp.executeCommand', title: cmd.title, arguments: [cmd, c] } } }
        const ca = a as L.CodeAction
        return { title: ca.title, kind: ca.kind, isPreferred: ca.isPreferred, disabled: ca.disabled?.reason, diagnostics: context.markers, edit: ca.edit ? workspaceEditToMonaco(ca.edit) : undefined, command: ca.command ? { id: 'caret.lsp.executeCommand', title: ca.command.title, arguments: [ca.command, c] } : undefined, ...( !ca.edit && !ca.command && ca.data ? { __resolve: ca } : {}) } as monaco.languages.CodeAction
      })
      return { actions, dispose() {} }
    },
    async resolveCodeAction(codeAction, token) {
      const ca = (codeAction as unknown as { __resolve?: L.CodeAction }).__resolve
      if (!ca) return codeAction
      const c = resolveAny(resolve, language); if (!c) return codeAction
      try { const r = await c.request<L.CodeAction>('codeAction/resolve', ca, 10000); if (r?.edit && !token.isCancellationRequested) codeAction.edit = workspaceEditToMonaco(r.edit) } catch { /* ignore */ }
      return codeAction
    },
  }))
  d.push(monaco.languages.registerInlayHintsProvider(language, {
    async provideInlayHints(model, range, token) {
      const c = resolve(model); if (!c?.capabilities.inlayHintProvider) return null
      const r = await c.request<L.InlayHint[] | null>('textDocument/inlayHint', { textDocument: docId(model), range: toLspRange(range) }, 10000)
      if (!r || token.isCancellationRequested) return null
      return { hints: r.map((h) => ({ position: toMonacoPosition(h.position), label: typeof h.label === 'string' ? h.label : h.label.map((p) => ({ label: p.value, tooltip: docToMarkdown(p.tooltip) ? { value: docToMarkdown(p.tooltip)! } : undefined, location: p.location ? { uri: monaco.Uri.parse(p.location.uri), range: toMonacoRange(p.location.range) } : undefined })), kind: h.kind === 2 ? monaco.languages.InlayHintKind.Parameter : monaco.languages.InlayHintKind.Type, paddingLeft: h.paddingLeft, paddingRight: h.paddingRight, tooltip: docToMarkdown(h.tooltip) ? { value: docToMarkdown(h.tooltip)! } : undefined })), dispose() {} }
    },
  }))
  d.push(monaco.languages.registerFoldingRangeProvider(language, {
    async provideFoldingRanges(model, _ctx, token) {
      const c = resolve(model); if (!c?.capabilities.foldingRangeProvider) return null
      const r = await c.request<L.FoldingRange[] | null>('textDocument/foldingRange', { textDocument: docId(model) }, 10000)
      if (!r || token.isCancellationRequested) return null
      return r.map((f) => ({ start: f.startLine + 1, end: f.endLine + 1, kind: f.kind === 'comment' ? monaco.languages.FoldingRangeKind.Comment : f.kind === 'imports' ? monaco.languages.FoldingRangeKind.Imports : f.kind === 'region' ? monaco.languages.FoldingRangeKind.Region : undefined }))
    },
  }))
  return d
}

function resolveAny(resolve: Resolver, language: string): LspClient | null {
  for (const m of monaco.editor.getModels()) if (m.getLanguageId() === language) { const c = resolve(m); if (c) return c }
  return null
}

export function toLocations(r: L.Location | L.Location[] | L.LocationLink[]): monaco.languages.Definition {
  const arr = Array.isArray(r) ? r : [r]
  return arr.map((l) => 'targetUri' in l
    ? { uri: monaco.Uri.file(uriToPath(l.targetUri)), range: toMonacoRange(l.targetSelectionRange ?? l.targetRange), originSelectionRange: l.originSelectionRange ? toMonacoRange(l.originSelectionRange) : undefined } as monaco.languages.LocationLink
    : { uri: monaco.Uri.file(uriToPath(l.uri)), range: toMonacoRange(l.range) })
}

export function symbolsToMonaco(r: L.DocumentSymbol[] | L.SymbolInformation[]): monaco.languages.DocumentSymbol[] {
  const conv = (s: L.DocumentSymbol): monaco.languages.DocumentSymbol => ({ name: s.name || '<unnamed>', detail: s.detail ?? '', kind: (SYMBOL_KIND_MAP[s.kind] ?? 12) as monaco.languages.SymbolKind, tags: s.tags?.includes(1) || s.deprecated ? [monaco.languages.SymbolTag.Deprecated] : [], range: toMonacoRange(s.range), selectionRange: toMonacoRange(s.selectionRange), children: s.children?.map(conv) })
  if (!r.length) return []
  if ('location' in r[0]) return (r as L.SymbolInformation[]).map((s) => ({ name: s.name, detail: s.containerName ?? '', kind: (SYMBOL_KIND_MAP[s.kind] ?? 12) as monaco.languages.SymbolKind, tags: [], range: toMonacoRange(s.location.range), selectionRange: toMonacoRange(s.location.range) }))
  return (r as L.DocumentSymbol[]).map(conv)
}

export function workspaceEditToMonaco(we: L.WorkspaceEdit): monaco.languages.WorkspaceEdit {
  const edits: (monaco.languages.IWorkspaceTextEdit | monaco.languages.IWorkspaceFileEdit)[] = []
  if (we.documentChanges) {
    for (const dc of we.documentChanges) {
      if ('kind' in dc) {
        if (dc.kind === 'create') edits.push({ newResource: monaco.Uri.parse(dc.uri), options: { overwrite: dc.options?.overwrite, ignoreIfExists: dc.options?.ignoreIfExists } })
        else if (dc.kind === 'rename') edits.push({ oldResource: monaco.Uri.parse(dc.oldUri), newResource: monaco.Uri.parse(dc.newUri), options: { overwrite: dc.options?.overwrite, ignoreIfExists: dc.options?.ignoreIfExists } })
        else if (dc.kind === 'delete') edits.push({ oldResource: monaco.Uri.parse(dc.uri), options: { recursive: dc.options?.recursive, ignoreIfNotExists: dc.options?.ignoreIfNotExists } })
      } else for (const e of dc.edits) edits.push({ resource: monaco.Uri.file(uriToPath(dc.textDocument.uri)), versionId: undefined, textEdit: { range: toMonacoRange(e.range), text: e.newText } })
    }
  } else if (we.changes) {
    for (const [uri, list] of Object.entries(we.changes)) for (const e of list) edits.push({ resource: monaco.Uri.file(uriToPath(uri)), versionId: undefined, textEdit: { range: toMonacoRange(e.range), text: e.newText } })
  }
  return { edits }
}

/** Apply an LSP WorkspaceEdit: open models get in-place edits (undoable); other files go through api.fs. */
export async function applyWorkspaceEdit(we: L.WorkspaceEdit): Promise<boolean> {
  const byFile = new Map<string, L.TextEdit[]>()
  const fileOps: (L.CreateFile | L.RenameFile | L.DeleteFile)[] = []
  if (we.documentChanges) for (const dc of we.documentChanges) { if ('kind' in dc) fileOps.push(dc); else { const p = uriToPath(dc.textDocument.uri); byFile.set(p, [...(byFile.get(p) ?? []), ...dc.edits]) } }
  else if (we.changes) for (const [u, l] of Object.entries(we.changes)) byFile.set(uriToPath(u), l)
  for (const op of fileOps) {
    try {
      if (op.kind === 'create') await api.fs.writeFile(uriToPath(op.uri), '', { createDirs: true })
      else if (op.kind === 'rename') await api.fs.rename(uriToPath(op.oldUri), uriToPath(op.newUri))
      else if (op.kind === 'delete') await api.fs.delete([uriToPath(op.uri)], true)
    } catch (e) { console.warn('[lsp] file op failed', op, e); return false }
  }
  for (const [path, edits] of byFile) {
    const model = monaco.editor.getModel(monaco.Uri.file(path))
    if (model) {
      model.pushEditOperations([], edits.map((e) => ({ range: toMonacoRange(e.range), text: e.newText })), () => null)
    } else {
      try {
        const fc = await api.fs.readFile(path)
        const text = applyTextEdits(fc.content, edits)
        await api.fs.writeFile(path, text, { lineEnding: fc.lineEnding === 'crlf' ? 'crlf' : 'lf' })
      } catch (e) { console.warn('[lsp] edit of closed file failed', path, e); return false }
    }
  }
  return true
}

import { applyTextEdits } from './providers-pure'
export { applyTextEdits }

let commandRegistered = false
/** Registers the Monaco command used by code actions / completion commands to run LSP workspace/executeCommand. */
export function registerLspCommands(): void {
  if (commandRegistered) return
  commandRegistered = true
  monaco.editor.registerCommand('caret.lsp.executeCommand', async (_acc, cmd: L.Command, client?: LspClient) => {
    if (!cmd) return
    const c = client ?? null
    if (c && !c.isClosed) { try { await c.request('workspace/executeCommand', { command: cmd.command, arguments: cmd.arguments }, 20000) } catch (e) { void runCommand('app.notify', { level: 'warn', title: 'Command failed', message: String((e as Error).message ?? e) }) } }
  })
}

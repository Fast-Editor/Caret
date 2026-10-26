/** Minimal LSP wire types used by the lean client (subset of the 3.17 spec). */
import type { LspPosition, LspRange } from '@/types'
export type { LspPosition, LspRange }
export interface TextDocumentIdentifier { uri: string }
export interface VersionedTextDocumentIdentifier extends TextDocumentIdentifier { version: number }
export interface TextDocumentPositionParams { textDocument: TextDocumentIdentifier; position: LspPosition }
export interface Location { uri: string; range: LspRange }
export interface LocationLink { originSelectionRange?: LspRange; targetUri: string; targetRange: LspRange; targetSelectionRange: LspRange }
export interface TextEdit { range: LspRange; newText: string }
export interface MarkupContent { kind: 'plaintext' | 'markdown'; value: string }
export type MarkedString = string | { language: string; value: string }
export interface Hover { contents: MarkupContent | MarkedString | MarkedString[]; range?: LspRange }
export interface CompletionItem {
  label: string | { label: string; detail?: string; description?: string }; kind?: number; tags?: number[]; detail?: string; documentation?: string | MarkupContent
  deprecated?: boolean; preselect?: boolean; sortText?: string; filterText?: string; insertText?: string; insertTextFormat?: 1 | 2
  textEdit?: TextEdit | { newText: string; insert: LspRange; replace: LspRange }; additionalTextEdits?: TextEdit[]; commitCharacters?: string[]; command?: Command; data?: unknown
}
export interface CompletionList { isIncomplete: boolean; items: CompletionItem[]; itemDefaults?: { editRange?: LspRange | { insert: LspRange; replace: LspRange }; insertTextFormat?: 1 | 2; data?: unknown } }
export interface Command { title: string; command: string; arguments?: unknown[] }
export interface SignatureHelp { signatures: { label: string; documentation?: string | MarkupContent; parameters?: { label: string | [number, number]; documentation?: string | MarkupContent }[]; activeParameter?: number }[]; activeSignature?: number; activeParameter?: number }
export interface DocumentSymbol { name: string; detail?: string; kind: number; tags?: number[]; deprecated?: boolean; range: LspRange; selectionRange: LspRange; children?: DocumentSymbol[] }
export interface SymbolInformation { name: string; kind: number; tags?: number[]; deprecated?: boolean; location: Location; containerName?: string }
export interface CodeAction { title: string; kind?: string; diagnostics?: unknown[]; isPreferred?: boolean; disabled?: { reason: string }; edit?: WorkspaceEdit; command?: Command; data?: unknown }
export interface WorkspaceEdit { changes?: Record<string, TextEdit[]>; documentChanges?: (TextDocumentEdit | CreateFile | RenameFile | DeleteFile)[] }
export interface TextDocumentEdit { textDocument: { uri: string; version: number | null }; edits: TextEdit[] }
export interface CreateFile { kind: 'create'; uri: string; options?: { overwrite?: boolean; ignoreIfExists?: boolean } }
export interface RenameFile { kind: 'rename'; oldUri: string; newUri: string; options?: { overwrite?: boolean; ignoreIfExists?: boolean } }
export interface DeleteFile { kind: 'delete'; uri: string; options?: { recursive?: boolean; ignoreIfNotExists?: boolean } }
export interface InlayHint { position: LspPosition; label: string | { value: string; tooltip?: string | MarkupContent; location?: Location }[]; kind?: 1 | 2; textEdits?: TextEdit[]; tooltip?: string | MarkupContent; paddingLeft?: boolean; paddingRight?: boolean }
export interface DocumentHighlight { range: LspRange; kind?: 1 | 2 | 3 }
export interface FoldingRange { startLine: number; startCharacter?: number; endLine: number; endCharacter?: number; kind?: string }
export interface ServerCapabilities {
  textDocumentSync?: number | { openClose?: boolean; change?: number; save?: boolean | { includeText?: boolean } }
  completionProvider?: { triggerCharacters?: string[]; resolveProvider?: boolean; allCommitCharacters?: string[] }
  hoverProvider?: boolean | object; signatureHelpProvider?: { triggerCharacters?: string[]; retriggerCharacters?: string[] }
  definitionProvider?: boolean | object; typeDefinitionProvider?: boolean | object; implementationProvider?: boolean | object; referencesProvider?: boolean | object
  documentHighlightProvider?: boolean | object; documentSymbolProvider?: boolean | object; codeActionProvider?: boolean | { codeActionKinds?: string[]; resolveProvider?: boolean }
  documentFormattingProvider?: boolean | object; documentRangeFormattingProvider?: boolean | object; renameProvider?: boolean | { prepareProvider?: boolean }
  foldingRangeProvider?: boolean | object; inlayHintProvider?: boolean | { resolveProvider?: boolean }; workspaceSymbolProvider?: boolean | object; codeLensProvider?: { resolveProvider?: boolean }
}
export interface PublishDiagnosticsParams { uri: string; version?: number; diagnostics: import('@/types').LspDiagnostic[] }

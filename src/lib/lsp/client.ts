/**
 * Lean JSON-RPC LSP client over the Rust relay (api.lsp.start/send). One instance per running server.
 */
import { api } from '@/lib/api'
import type { LspRawMessage, LspServerConfig, LspServerStatus } from '@/types'
import type { ServerCapabilities, PublishDiagnosticsParams } from './types'
import { pathToUri } from './convert'

type Pending = { resolve: (v: unknown) => void; reject: (e: Error) => void; method: string; timer: ReturnType<typeof setTimeout> }
export type NotificationHandler = (method: string, params: unknown) => void
export type ServerRequestHandler = (method: string, params: unknown) => Promise<unknown> | unknown

export class LspRequestError extends Error { constructor(public code: number, message: string, public data?: unknown) { super(message); this.name = 'LspRequestError' } }

export class LspClient {
  serverId = ''
  status: LspServerStatus | null = null
  capabilities: ServerCapabilities = {}
  private nextId = 1
  private pending = new Map<number, Pending>()
  private notificationHandlers = new Set<NotificationHandler>()
  private requestHandlers = new Map<string, ServerRequestHandler>()
  private closed = false
  private onExit: (() => void) | null = null
  initialized = false
  readonly openDocs = new Map<string, number>() // uri → version

  constructor(public readonly config: LspServerConfig, public readonly root: string) {}

  async start(onExit?: () => void): Promise<void> {
    this.onExit = onExit ?? null
    this.status = await api.lsp.start(this.config.id, this.root, (m) => this.onMessage(m))
    this.serverId = this.status.serverId
    const rootUri = pathToUri(this.root)
    const result = (await this.request('initialize', {
      processId: null,
      clientInfo: { name: 'Caret', version: '0.1.0' },
      locale: 'en',
      rootPath: this.root, rootUri,
      workspaceFolders: [{ uri: rootUri, name: this.root.split('/').pop() ?? this.root }],
      initializationOptions: this.config.initializationOptions ?? undefined,
      capabilities: {
        workspace: {
          applyEdit: true, workspaceEdit: { documentChanges: true, resourceOperations: ['create', 'rename', 'delete'], failureHandling: 'textOnlyTransactional' },
          configuration: true, workspaceFolders: true, didChangeConfiguration: { dynamicRegistration: true }, didChangeWatchedFiles: { dynamicRegistration: true, relativePatternSupport: true },
          symbol: { dynamicRegistration: false }, executeCommand: { dynamicRegistration: false }, inlayHint: { refreshSupport: true }, semanticTokens: { refreshSupport: false }, diagnostics: { refreshSupport: true },
        },
        textDocument: {
          synchronization: { dynamicRegistration: false, willSave: false, willSaveWaitUntil: false, didSave: true },
          completion: { dynamicRegistration: false, contextSupport: true, completionItem: { snippetSupport: true, commitCharactersSupport: true, documentationFormat: ['markdown', 'plaintext'], deprecatedSupport: true, preselectSupport: true, insertReplaceSupport: true, resolveSupport: { properties: ['documentation', 'detail', 'additionalTextEdits'] }, labelDetailsSupport: true, tagSupport: { valueSet: [1] } }, completionItemKind: { valueSet: Array.from({ length: 25 }, (_, i) => i + 1) }, completionList: { itemDefaults: ['editRange', 'insertTextFormat', 'data'] } },
          hover: { dynamicRegistration: false, contentFormat: ['markdown', 'plaintext'] },
          signatureHelp: { dynamicRegistration: false, contextSupport: true, signatureInformation: { documentationFormat: ['markdown', 'plaintext'], parameterInformation: { labelOffsetSupport: true }, activeParameterSupport: true } },
          definition: { dynamicRegistration: false, linkSupport: true }, typeDefinition: { dynamicRegistration: false, linkSupport: true }, implementation: { dynamicRegistration: false, linkSupport: true }, references: { dynamicRegistration: false },
          documentHighlight: { dynamicRegistration: false }, documentSymbol: { dynamicRegistration: false, hierarchicalDocumentSymbolSupport: true, symbolKind: { valueSet: Array.from({ length: 26 }, (_, i) => i + 1) }, tagSupport: { valueSet: [1] } },
          codeAction: { dynamicRegistration: false, isPreferredSupport: true, disabledSupport: true, dataSupport: true, resolveSupport: { properties: ['edit'] }, codeActionLiteralSupport: { codeActionKind: { valueSet: ['', 'quickfix', 'refactor', 'refactor.extract', 'refactor.inline', 'refactor.rewrite', 'source', 'source.organizeImports', 'source.fixAll'] } } },
          formatting: { dynamicRegistration: false }, rangeFormatting: { dynamicRegistration: false }, rename: { dynamicRegistration: false, prepareSupport: true, prepareSupportDefaultBehavior: 1, honorsChangeAnnotations: false },
          publishDiagnostics: { relatedInformation: true, tagSupport: { valueSet: [1, 2] }, versionSupport: true, codeDescriptionSupport: true, dataSupport: true },
          foldingRange: { dynamicRegistration: false, lineFoldingOnly: true }, inlayHint: { dynamicRegistration: false, resolveSupport: { properties: ['tooltip', 'textEdits', 'label.tooltip', 'label.location'] } },
          codeLens: { dynamicRegistration: false }, semanticTokens: { dynamicRegistration: false, requests: { range: false, full: false }, tokenTypes: [], tokenModifiers: [], formats: ['relative'] },
        },
        window: { workDoneProgress: true, showMessage: { messageActionItem: { additionalPropertiesSupport: false } }, showDocument: { support: true } },
        general: { positionEncodings: ['utf-16'], markdown: { parser: 'marked', version: '1.1.0' } },
      },
    }, 60_000)) as { capabilities: ServerCapabilities }
    this.capabilities = result?.capabilities ?? {}
    this.notify('initialized', {})
    if (this.config.settings) this.notify('workspace/didChangeConfiguration', { settings: this.config.settings })
    this.initialized = true
  }

  onNotification(h: NotificationHandler): () => void { this.notificationHandlers.add(h); return () => { this.notificationHandlers.delete(h) } }
  onRequest(method: string, h: ServerRequestHandler): void { this.requestHandlers.set(method, h) }

  private onMessage(m: LspRawMessage): void {
    if (this.closed) return
    // Relay-level lifecycle messages (sent by the Rust side when the process exits)
    if (m.method === '$/caret/exit') { this.handleExit(); return }
    if (m.id !== undefined && m.method === undefined) {
      const p = this.pending.get(Number(m.id))
      if (!p) return
      this.pending.delete(Number(m.id)); clearTimeout(p.timer)
      if (m.error) p.reject(new LspRequestError(m.error.code, m.error.message, m.error.data)); else p.resolve(m.result)
      return
    }
    if (m.method && m.id !== undefined) {
      // server → client request
      const h = this.requestHandlers.get(m.method)
      Promise.resolve()
        .then(() => (h ? h(m.method!, m.params) : this.defaultServerRequest(m.method!, m.params)))
        .then((result) => this.send({ jsonrpc: '2.0', id: m.id, result: result ?? null }))
        .catch((e: Error) => this.send({ jsonrpc: '2.0', id: m.id, error: { code: -32601, message: e?.message ?? 'unhandled' } }))
      return
    }
    if (m.method) { for (const h of this.notificationHandlers) { try { h(m.method, m.params) } catch (e) { console.error('[lsp] notification handler', e) } } }
  }

  private defaultServerRequest(method: string, params: unknown): unknown {
    switch (method) {
      case 'workspace/configuration': {
        const items = (params as { items: { section?: string }[] })?.items ?? []
        const settings = (this.config.settings ?? {}) as Record<string, unknown>
        return items.map((it) => (it.section ? it.section.split('.').reduce<unknown>((acc, k) => (acc && typeof acc === 'object' ? (acc as Record<string, unknown>)[k] : undefined), settings) ?? null : settings))
      }
      case 'workspace/workspaceFolders': return [{ uri: pathToUri(this.root), name: this.root.split('/').pop() }]
      case 'client/registerCapability': case 'client/unregisterCapability': case 'window/workDoneProgress/create': case 'workspace/semanticTokens/refresh': case 'workspace/inlayHint/refresh': case 'workspace/codeLens/refresh': case 'workspace/diagnostic/refresh': return null
      case 'window/showMessageRequest': return null
      default: return null
    }
  }

  request<T = unknown>(method: string, params: unknown, timeoutMs = 30_000): Promise<T> {
    if (this.closed) return Promise.reject(new Error(`LSP ${this.config.name} stopped`))
    const id = this.nextId++
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error(`LSP ${method} timed out`)) }, timeoutMs)
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject, method, timer })
      this.send({ jsonrpc: '2.0', id, method, params }).catch((e) => { this.pending.delete(id); clearTimeout(timer); reject(e) })
    })
  }
  notify(method: string, params: unknown): void { void this.send({ jsonrpc: '2.0', method, params }) }
  private send(message: LspRawMessage): Promise<void> {
    if (this.closed) return Promise.resolve()
    return api.lsp.send(this.serverId, message).catch((e) => { console.warn('[lsp] send failed', this.config.id, e); throw e })
  }

  // ─── document sync helpers ───
  didOpen(uri: string, languageId: string, text: string): void { this.openDocs.set(uri, 1); this.notify('textDocument/didOpen', { textDocument: { uri, languageId, version: 1, text } }) }
  didChange(uri: string, changes: { range?: unknown; text: string }[]): number | null {
    const v = this.openDocs.get(uri); if (v === undefined) return null
    const version = v + 1; this.openDocs.set(uri, version)
    const sync = this.capabilities.textDocumentSync
    const kind = typeof sync === 'number' ? sync : sync?.change ?? 1
    const contentChanges = kind === 2 ? changes : [{ text: changes[changes.length - 1]?.text ?? '' }]
    this.notify('textDocument/didChange', { textDocument: { uri, version }, contentChanges })
    return version
  }
  didSave(uri: string, text?: string): void { if (!this.openDocs.has(uri)) return; const sync = this.capabilities.textDocumentSync; const includeText = typeof sync === 'object' && typeof sync.save === 'object' && sync.save.includeText; this.notify('textDocument/didSave', { textDocument: { uri }, ...(includeText ? { text } : {}) }) }
  didClose(uri: string): void { if (!this.openDocs.delete(uri)) return; this.notify('textDocument/didClose', { textDocument: { uri } }) }

  async stop(): Promise<void> {
    if (this.closed) return
    try { await Promise.race([this.request('shutdown', null, 3000), new Promise((r) => setTimeout(r, 3000))]); this.notify('exit', null) } catch { /* ignore */ }
    this.handleExit(false)
    try { await api.lsp.stop(this.serverId) } catch { /* ignore */ }
  }
  private handleExit(notify = true): void {
    if (this.closed) return
    this.closed = true
    for (const p of this.pending.values()) { clearTimeout(p.timer); p.reject(new Error(`LSP ${this.config.name} exited`)) }
    this.pending.clear()
    if (notify) this.onExit?.()
  }
  get isClosed(): boolean { return this.closed }
  handlesDiagnostics(h: (p: PublishDiagnosticsParams) => void): () => void { return this.onNotification((m, p) => { if (m === 'textDocument/publishDiagnostics') h(p as PublishDiagnosticsParams) }) }
}

/**
 * LspManager: lazy server lifecycle per workspace, Monaco document sync, diagnostics → markers + problems store.
 */
import { monaco } from '@/lib/monaco/setup'
import { api } from '@/lib/api'
import { useLsp } from '@/store/lsp'
import { useProblems } from '@/store/problems'
import { useSettings } from '@/store/settings'
import { useWorkspace } from '@/store/workspace'
import { runCommand } from '@/lib/commands'
import type { LspServerConfig, LspDiagnostic } from '@/types'
import { LspClient } from './client'
import { pathToUri, uriToPath, toMonacoRange, SEVERITY_MAP, toLspContentChange } from './convert'
import { lspLanguageId } from '@/lib/languages'
import { registerLspProviders } from './providers'

const MAX_RESTARTS = 3

interface BuiltinTsDefaults { setModeConfiguration?: (m: Record<string, boolean>) => void; setEagerModelSync?: (v: boolean) => void }
interface BuiltinTs { typescriptDefaults: BuiltinTsDefaults; javascriptDefaults: BuiltinTsDefaults }

class LspManagerImpl {
  private clients = new Map<string, LspClient>() // configId → client
  private starting = new Map<string, Promise<LspClient | null>>()
  private root: string | null = null
  private disposables: monaco.IDisposable[] = []
  private changeTimers = new Map<string, ReturnType<typeof setTimeout>>()
  private pendingChanges = new Map<string, { range: monaco.IRange; text: string }[]>()
  private providersRegistered = new Set<string>()
  private started = false
  private builtinTsEnabled = true

  init(): void {
    if (this.started) return
    this.started = true
    this.disposables.push(monaco.editor.onDidCreateModel((m) => this.onModelCreated(m)))
    this.disposables.push(monaco.editor.onWillDisposeModel((m) => this.onModelDisposed(m)))
    // Built-in worker diagnostics (TS/JSON/CSS/HTML) → problems store + backend, so agent tools can read lints.
    this.disposables.push(monaco.editor.onDidChangeMarkers((uris) => this.syncBuiltinMarkers(uris)))
    useWorkspace.subscribe((s, prev) => { if (s.workspace?.root !== prev.workspace?.root) void this.setRoot(s.workspace?.root ?? null) })
    void this.setRoot(useWorkspace.getState().workspace?.root ?? null)
    useSettings.subscribe((s, prev) => {
      if (!s.settings) return
      if (s.settings.lsp.enabled !== prev.settings?.lsp.enabled && !s.settings.lsp.enabled) { void this.stopAll(); return }
      if (s.settings.lsp.servers !== prev.settings?.lsp.servers || s.settings.lsp.enabled !== prev.settings?.lsp.enabled) void this.refreshConfigs()
    })
  }

  /** Re-read server configs (after settings changes) and attach already-open models. */
  private async refreshConfigs(): Promise<void> {
    if (!this.root) return
    try {
      const configs = await api.lsp.configs()
      useLsp.getState().setConfigs(configs)
      for (const { config } of configs) for (const lang of config.languages) this.ensureProviders(lang)
      for (const m of monaco.editor.getModels()) this.onModelCreated(m)
    } catch (e) { console.warn('[lsp] configs refresh failed', e) }
  }

  /**
   * When a real TypeScript/JavaScript server is running, hand the language over to it: silence Monaco's
   * built-in TS worker (no duplicate diagnostics/completions, and no eager model sync memory cost).
   */
  private setBuiltinTypescript(enabled: boolean): void {
    this.builtinTsEnabled = enabled
    const m = monaco as unknown as { typescript?: BuiltinTs; languages?: { typescript?: BuiltinTs } }
    const ts = m.typescript ?? m.languages?.typescript
    if (!ts) return
    const mode: Record<string, boolean> = { completionItems: enabled, hovers: enabled, documentSymbols: enabled, definitions: enabled, references: enabled, documentHighlights: enabled, rename: enabled, diagnostics: enabled, documentRangeFormattingEdits: enabled, signatureHelp: enabled, onTypeFormattingEdits: enabled, codeActions: enabled, inlayHints: enabled }
    for (const d of [ts.typescriptDefaults, ts.javascriptDefaults]) {
      try { d.setModeConfiguration?.(mode) } catch { /* older monaco */ }
      try { d.setEagerModelSync?.(enabled) } catch { /* ignore */ }
    }
    if (!enabled) { for (const model of monaco.editor.getModels()) { const l = model.getLanguageId(); if (l === 'typescript' || l === 'javascript' || l === 'typescriptreact' || l === 'javascriptreact') monaco.editor.setModelMarkers(model, 'typescript', []) } }
  }

  private async setRoot(root: string | null): Promise<void> {
    if (root === this.root) return
    await this.stopAll()
    this.root = root
    useLsp.getState().reset()
    if (!root) return
    try {
      const configs = await api.lsp.configs()
      useLsp.getState().setConfigs(configs)
      for (const { config } of configs) for (const lang of config.languages) this.ensureProviders(lang)
      // attach already-open models
      for (const m of monaco.editor.getModels()) this.onModelCreated(m)
    } catch (e) { console.warn('[lsp] configs failed', e) }
  }

  private ensureProviders(language: string): void {
    if (this.providersRegistered.has(language)) return
    this.providersRegistered.add(language)
    this.disposables.push(...registerLspProviders(language, (model) => this.clientForModel(model)))
  }

  /** Config whose languages include the model language (first wins). */
  private configFor(model: monaco.editor.ITextModel): LspServerConfig | null {
    if (model.uri.scheme !== 'file') return null
    const lang = model.getLanguageId()
    const path = model.uri.path
    const ext = path.split('.').pop()?.toLowerCase() ?? ''
    const entries = Object.values(useLsp.getState().servers)
    for (const e of entries) {
      if (!e.config.enabled || !e.installed) continue
      if (e.config.languages.includes(lang) || e.config.extensions.includes(ext)) return e.config
    }
    return null
  }

  clientForModel(model: monaco.editor.ITextModel): LspClient | null {
    const cfg = this.configFor(model); if (!cfg) return null
    const c = this.clients.get(cfg.id)
    return c && c.initialized && !c.isClosed ? c : null
  }
  clientForPath(path: string): LspClient | null { const m = monaco.editor.getModel(monaco.Uri.file(path)); return m ? this.clientForModel(m) : null }
  get clientsList(): LspClient[] { return [...this.clients.values()] }

  private async ensureClient(cfg: LspServerConfig): Promise<LspClient | null> {
    const existing = this.clients.get(cfg.id)
    if (existing && !existing.isClosed) return existing
    const inflight = this.starting.get(cfg.id)
    if (inflight) return inflight
    const root = this.root; if (!root) return null
    if (!(useSettings.getState().settings?.lsp.enabled ?? true)) return null
    const p = (async () => {
      const client = new LspClient(cfg, root)
      useLsp.getState().setStatus(cfg.id, { serverId: '', configId: cfg.id, name: cfg.name, state: 'starting', languages: cfg.languages, root })
      try {
        await client.start(() => this.onClientExit(cfg))
        this.clients.set(cfg.id, client)
        client.handlesDiagnostics((p) => this.onDiagnostics(cfg.id, p))
        client.onNotification((method, params) => {
          if (method === 'window/showMessage' || method === 'window/logMessage') {
            const { type, message } = params as { type: number; message: string }
            if (method === 'window/showMessage') void runCommand('app.notify', { level: type === 1 ? 'error' : type === 2 ? 'warn' : 'info', title: cfg.name, message })
          }
        })
        client.onRequest('workspace/applyEdit', async (_m, params) => { const { applyWorkspaceEdit } = await import('./providers'); const ok = await applyWorkspaceEdit((params as { edit: import('./types').WorkspaceEdit }).edit); return { applied: ok } })
        client.onRequest('window/showDocument', async (_m, params) => { const { uri, selection } = params as { uri: string; selection?: import('@/types').LspRange }; if (uri.startsWith('file://')) await runCommand('editor.openFile', { path: uriToPath(uri), line: selection ? selection.start.line + 1 : undefined, column: selection ? selection.start.character + 1 : undefined }); else await api.app.openExternal(uri); return { success: true } })
        useLsp.getState().setStatus(cfg.id, { ...client.status!, state: 'running', name: cfg.name, languages: cfg.languages, root })
        if (cfg.languages.some((l) => l === 'typescript' || l === 'javascript')) this.setBuiltinTypescript(false)
        // open already-created models for this server
        for (const m of monaco.editor.getModels()) if (this.configFor(m)?.id === cfg.id) this.openModel(client, m)
        return client
      } catch (e) {
        console.warn(`[lsp] ${cfg.name} failed to start`, e)
        useLsp.getState().setStatus(cfg.id, { serverId: '', configId: cfg.id, name: cfg.name, state: 'error', error: String((e as Error)?.message ?? e), languages: cfg.languages, root })
        return null
      } finally { this.starting.delete(cfg.id) }
    })()
    this.starting.set(cfg.id, p)
    return p
  }

  private onClientExit(cfg: LspServerConfig): void {
    this.clients.delete(cfg.id)
    useProblems.getState().clearOwner(cfg.id)
    for (const m of monaco.editor.getModels()) monaco.editor.setModelMarkers(m, `lsp:${cfg.id}`, [])
    const n = useLsp.getState().bumpRestarts(cfg.id)
    if (n <= MAX_RESTARTS && this.root) {
      useLsp.getState().setStatus(cfg.id, { serverId: '', configId: cfg.id, name: cfg.name, state: 'starting', languages: cfg.languages, root: this.root })
      setTimeout(() => { void this.ensureClient(cfg) }, 1000 * n)
    } else {
      useLsp.getState().setStatus(cfg.id, { serverId: '', configId: cfg.id, name: cfg.name, state: 'error', error: 'Server exited repeatedly; restart from the status bar', languages: cfg.languages, root: this.root ?? '' })
    }
  }

  async restart(configId: string): Promise<void> {
    const cfg = useLsp.getState().servers[configId]?.config; if (!cfg) return
    const c = this.clients.get(configId); if (c) { this.clients.delete(configId); await c.stop() }
    const s = useLsp.getState(); s.servers[configId] && (s.servers[configId].restarts = 0)
    await this.ensureClient(cfg)
  }
  async stopServer(configId: string): Promise<void> { const c = this.clients.get(configId); if (c) { this.clients.delete(configId); await c.stop() } useLsp.getState().setStatus(configId, undefined); useProblems.getState().clearOwner(configId); for (const m of monaco.editor.getModels()) monaco.editor.setModelMarkers(m, `lsp:${configId}`, []); if (configId === 'typescript') this.setBuiltinTypescript(true) }
  async stopAll(): Promise<void> { const cs = [...this.clients.values()]; this.clients.clear(); this.setBuiltinTypescript(true); await Promise.all(cs.map((c) => c.stop().catch(() => {}))); for (const id of Object.keys(useLsp.getState().servers)) { useLsp.getState().setStatus(id, undefined); useProblems.getState().clearOwner(id) } }

  private onModelCreated(model: monaco.editor.ITextModel): void {
    const cfg = this.configFor(model); if (!cfg) return
    const client = this.clients.get(cfg.id)
    if (client && client.initialized && !client.isClosed) this.openModel(client, model)
    else void this.ensureClient(cfg)
  }
  private openModel(client: LspClient, model: monaco.editor.ITextModel): void {
    const uri = pathToUri(model.uri.path)
    if (client.openDocs.has(uri)) return
    client.didOpen(uri, lspLanguageId(model.getLanguageId(), model.uri.path), model.getValue())
    const d = model.onDidChangeContent((e) => {
      const list = this.pendingChanges.get(uri) ?? []
      for (const c of e.changes) list.push({ range: c.range, text: c.text })
      this.pendingChanges.set(uri, list)
      const t = this.changeTimers.get(uri); if (t) clearTimeout(t)
      this.changeTimers.set(uri, setTimeout(() => this.flushChanges(client, model, uri), 150))
    })
    this.disposables.push(d)
  }
  private flushChanges(client: LspClient, model: monaco.editor.ITextModel, uri: string): void {
    this.changeTimers.delete(uri)
    const list = this.pendingChanges.get(uri); this.pendingChanges.delete(uri)
    if (!list?.length || model.isDisposed() || client.isClosed) return
    const sync = client.capabilities.textDocumentSync
    const kind = typeof sync === 'number' ? sync : sync?.change ?? 1
    client.didChange(uri, kind === 2 ? list.map(toLspContentChange) : [{ text: model.getValue() }])
  }
  /** Called by the editor store after a save. */
  didSave(path: string, text: string): void {
    const uri = pathToUri(path)
    for (const c of this.clients.values()) if (c.openDocs.has(uri)) { const t = this.changeTimers.get(uri); if (t) { clearTimeout(t); const m = monaco.editor.getModel(monaco.Uri.file(path)); if (m) this.flushChanges(c, m, uri) } c.didSave(uri, text) }
  }
  private onModelDisposed(model: monaco.editor.ITextModel): void {
    const uri = pathToUri(model.uri.path)
    const t = this.changeTimers.get(uri); if (t) { clearTimeout(t); this.changeTimers.delete(uri) }
    this.pendingChanges.delete(uri)
    for (const c of this.clients.values()) c.didClose(uri)
  }

  private onDiagnostics(owner: string, p: { uri: string; diagnostics: LspDiagnostic[]; version?: number }): void {
    const path = uriToPath(p.uri)
    useProblems.getState().set(path, owner, p.diagnostics)
    const model = monaco.editor.getModel(monaco.Uri.file(path))
    // Marker owner is namespaced so it can never collide with Monaco's built-in workers ('typescript', 'json', …).
    if (model) monaco.editor.setModelMarkers(model, `lsp:${owner}`, p.diagnostics.map((d) => toMarker(d)))
    // The Rust relay already records publishDiagnostics in the shared DiagnosticsStore — no UI re-report needed.
  }

  private builtinTimer: ReturnType<typeof setTimeout> | null = null
  private builtinDirty = new Set<string>()
  private syncBuiltinMarkers(uris: readonly monaco.Uri[]): void {
    for (const u of uris) if (u.scheme === 'file') this.builtinDirty.add(u.path)
    if (this.builtinTimer) return
    this.builtinTimer = setTimeout(() => {
      this.builtinTimer = null
      const paths = [...this.builtinDirty]; this.builtinDirty.clear()
      for (const path of paths) {
        const uri = monaco.Uri.file(path)
        if (!this.builtinTsEnabled) {
          // A real TS/JS server owns these languages now; drop any late markers from the built-in worker.
          const model = monaco.editor.getModel(uri)
          const lang = model?.getLanguageId()
          if (lang === 'typescript' || lang === 'javascript' || lang === 'typescriptreact' || lang === 'javascriptreact') {
            for (const owner of ['typescript', 'javascript']) if (monaco.editor.getModelMarkers({ resource: uri, owner }).length) monaco.editor.setModelMarkers(model!, owner, [])
          }
        }
        const markers = monaco.editor.getModelMarkers({ resource: uri }).filter((m) => !m.owner.startsWith('lsp:') && !(!this.builtinTsEnabled && (m.owner === 'typescript' || m.owner === 'javascript')))
        const diags: LspDiagnostic[] = markers.map((m) => ({ range: { start: { line: m.startLineNumber - 1, character: m.startColumn - 1 }, end: { line: m.endLineNumber - 1, character: m.endColumn - 1 } }, severity: m.severity === 8 ? 1 : m.severity === 4 ? 2 : m.severity === 2 ? 3 : 4, code: typeof m.code === 'object' ? m.code?.value : m.code, source: m.source ?? m.owner, message: m.message }))
        useProblems.getState().set(path, 'builtin', diags)
        void api.lsp.reportDiagnostics({ uri: pathToUri(path), path, diagnostics: diags, serverId: 'builtin' }).catch(() => {})
      }
    }, 400)
  }

  dispose(): void { for (const d of this.disposables) d.dispose(); this.disposables = []; void this.stopAll(); this.started = false }
}

export function toMarker(d: LspDiagnostic): monaco.editor.IMarkerData {
  const r = toMonacoRange(d.range)
  return {
    ...r, severity: (SEVERITY_MAP[d.severity ?? 1] ?? 8) as monaco.MarkerSeverity, message: d.message, source: d.source,
    code: d.code === undefined ? undefined : d.codeDescription?.href ? { value: String(d.code), target: monaco.Uri.parse(d.codeDescription.href) } : String(d.code),
    tags: d.tags?.map((t) => (t === 1 ? monaco.MarkerTag.Unnecessary : monaco.MarkerTag.Deprecated)),
    relatedInformation: d.relatedInformation?.map((ri) => ({ resource: monaco.Uri.parse(ri.location.uri), message: ri.message, ...toMonacoRange(ri.location.range) })),
  }
}

export const lspManager = new LspManagerImpl()

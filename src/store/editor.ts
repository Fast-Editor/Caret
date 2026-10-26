/**
 * Editor state: groups/tabs, Monaco model lifecycle (LRU-bounded), dirty tracking, save/autosave,
 * external change handling, recent edits ring, navigation history, session persistence.
 * Monaco objects live in module-level maps (not in zustand) — the store only holds serializable flags.
 */
import { create } from 'zustand'
import { monaco, initMonaco } from '@/lib/monaco/setup'
import { api } from '@/lib/api'
import { Events, on } from '@/lib/tauri'
import { emit as busEmit } from '@/lib/bus'
import { runCommand } from '@/lib/commands'
import { useSettings } from '@/store/settings'
import { useWorkspace } from '@/store/workspace'
import { languageForPath, isImagePath } from '@/lib/languages'
import type { RecentEdit, FileContent, FsChangeEvent } from '@/types'

export type TabKind = 'file' | 'diff' | 'settings' | 'welcome' | 'plan' | 'browser' | 'image' | 'markdown-preview' | 'review' | 'keybindings' | 'untitled'
export interface DiffMeta { original: string; modified: string; title?: string; language?: string; changeId?: string; sessionId?: string; path?: string; originalLabel?: string; modifiedLabel?: string }
export interface EditorTab {
  id: string; kind: TabKind; path?: string; title: string; preview?: boolean; pinned?: boolean; dirty: boolean; language?: string
  diffMeta?: DiffMeta; settingsPage?: string; settingsQuery?: string; url?: string; browserTabId?: string; readonly?: boolean; notice?: string; conflict?: boolean
}
export interface EditorGroup { id: string; tabs: EditorTab[]; activeTabId: string | null }
export interface NavEntry { path: string; line: number; column: number }
export interface ConfirmClose { groupId: string; tabId: string; title: string; resolve: (choice: 'save' | 'discard' | 'cancel') => void }

interface ModelEntry { model: monaco.editor.ITextModel; mtime: number; lineEnding: 'lf' | 'crlf' | 'mixed'; savedVersion: number; lastAccess: number; disposables: monaco.IDisposable[]; lineCache: Map<number, string>; autosaveTimer: ReturnType<typeof setTimeout> | null; size: number }

const models = new Map<string, ModelEntry>()
const viewStates = new Map<string, monaco.editor.ICodeEditorViewState | null>() // tabId → view state
const editors = new Map<string, monaco.editor.IStandaloneCodeEditor>() // groupId → editor instance
const MAX_MODELS = 40
const MAX_RECENT_EDITS = 20
const LARGE_FILE_BYTES = 2 * 1024 * 1024
let idSeq = 1
const nid = (p: string) => `${p}-${idSeq++}-${Math.random().toString(36).slice(2, 7)}`

interface EditorStore {
  groups: EditorGroup[]
  activeGroupId: string
  recentFiles: string[]
  recentEdits: RecentEdit[]
  navBack: NavEntry[]
  navForward: NavEntry[]
  confirmClose: ConfirmClose | null
  blameEnabled: boolean
  tabStatus: 'idle' | 'loading' | 'shown' | 'error' | 'off'
  /** cursor of the active editor (status bar) */
  cursor: { line: number; column: number; selected: number }
  /** bump to force re-render after model-level changes */
  tick: number

  openFile: (path: string, opts?: { line?: number; column?: number; endLine?: number; preview?: boolean; focus?: boolean; group?: string | number; pinned?: boolean }) => Promise<EditorTab | null>
  openDiff: (meta: DiffMeta) => EditorTab
  openSpecial: (kind: Exclude<TabKind, 'file' | 'diff'>, opts?: { title?: string; path?: string; settingsPage?: string; settingsQuery?: string; url?: string; browserTabId?: string; preview?: boolean }) => EditorTab
  setActive: (groupId: string, tabId: string) => void
  setActiveGroup: (groupId: string) => void
  closeTab: (groupId: string, tabId: string, opts?: { force?: boolean }) => Promise<boolean>
  closeOthers: (groupId: string, tabId: string) => Promise<void>
  closeToRight: (groupId: string, tabId: string) => Promise<void>
  closeAll: (groupId?: string) => Promise<void>
  closeGroup: (groupId: string) => Promise<void>
  pinTab: (groupId: string, tabId: string, pinned: boolean) => void
  keepTab: (groupId: string, tabId: string) => void
  reorderTab: (groupId: string, from: number, to: number) => void
  moveTab: (fromGroup: string, tabId: string, toGroup: string, index?: number) => void
  split: () => void
  save: (path?: string) => Promise<boolean>
  saveAll: () => Promise<void>
  revert: (path: string) => Promise<void>
  nextTab: (dir: 1 | -1) => void
  pushNav: (e: NavEntry) => void
  goBack: () => void
  goForward: () => void
  setBlame: (on: boolean) => void
  setTabStatus: (s: EditorStore['tabStatus']) => void
}

export const useEditor = create<EditorStore>((set, get) => ({
  groups: [{ id: 'g1', tabs: [], activeTabId: null }],
  activeGroupId: 'g1',
  recentFiles: [], recentEdits: [], navBack: [], navForward: [], confirmClose: null, blameEnabled: false, tabStatus: 'idle', cursor: { line: 1, column: 1, selected: 0 }, tick: 0,

  openFile: async (path, opts = {}) => {
    const s = get()
    if (isImagePath(path)) return s.openSpecial('image', { path, title: baseName(path), preview: opts.preview })
    // Canonicalize (macOS /tmp → /private/tmp, symlinks) so watcher events and tabs agree on one key.
    try { const st = await api.fs.stat(path); if (st.exists && st.path && st.path !== path) path = st.path } catch { /* keep as-is */ }
    const groupId = resolveGroup(s, opts.group)
    const group = s.groups.find((g) => g.id === groupId)!
    let tab = group.tabs.find((t) => t.kind === 'file' && t.path === path)
    if (!tab) {
      try { await ensureModel(path) } catch (e) { void runCommand('app.notify', { level: 'error', title: 'Cannot open file', message: `${baseName(path)}: ${String((e as Error).message ?? e)}` }); return null }
      const entry = models.get(path)!
      const previewTabs = useSettings.getState().settings?.ui.previewTabs ?? true
      tab = { id: nid('tab'), kind: 'file', path, title: baseName(path), preview: previewTabs && (opts.preview ?? true) && !opts.pinned, pinned: opts.pinned, dirty: isDirty(entry), language: entry.model.getLanguageId(), readonly: entry.size > LARGE_FILE_BYTES, notice: entry.size > LARGE_FILE_BYTES ? 'Large file: opened read-only' : undefined }
      set((st) => ({ groups: st.groups.map((g) => {
        if (g.id !== groupId) return g
        const tabs = [...g.tabs]
        const previewIdx = tabs.findIndex((t) => t.preview && !t.dirty)
        if (tab!.preview && previewIdx >= 0) { viewStates.delete(tabs[previewIdx].id); releaseTabModel(tabs[previewIdx], st.groups); tabs[previewIdx] = tab! }
        else { const idx = g.activeTabId ? tabs.findIndex((t) => t.id === g.activeTabId) : -1; tabs.splice(idx >= 0 ? idx + 1 : tabs.length, 0, tab!) }
        return { ...g, tabs, activeTabId: tab!.id }
      }), activeGroupId: groupId, recentFiles: [path, ...st.recentFiles.filter((p) => p !== path)].slice(0, 50) }))
    } else {
      if (opts.preview === false || opts.pinned) set((st) => ({ groups: st.groups.map((g) => g.id === groupId ? { ...g, tabs: g.tabs.map((t) => t.id === tab!.id ? { ...t, preview: false } : t) } : g) }))
      get().setActive(groupId, tab.id)
    }
    if (opts.line) {
      requestAnimationFrame(() => { const ed = editors.get(groupId); if (!ed) return; const line = opts.line!; const col = opts.column ?? 1; const endLine = opts.endLine ?? line; ed.revealRangeInCenterIfOutsideViewport(new monaco.Range(line, 1, endLine, 1)); if (opts.endLine && opts.endLine !== line) ed.setSelection(new monaco.Range(line, 1, endLine + 1, 1)); else ed.setPosition({ lineNumber: line, column: col }); if (opts.focus !== false) ed.focus() })
    } else if (opts.focus !== false) requestAnimationFrame(() => editors.get(groupId)?.focus())
    persistSession()
    return get().groups.find((g) => g.id === groupId)!.tabs.find((t) => t.id === tab!.id) ?? tab
  },

  openDiff: (meta) => {
    const s = get(); const groupId = s.activeGroupId
    const title = meta.title ?? `${baseName(meta.path ?? 'diff')} (diff)`
    const existing = s.groups.find((g) => g.id === groupId)!.tabs.find((t) => t.kind === 'diff' && t.diffMeta?.changeId && t.diffMeta.changeId === meta.changeId && meta.changeId)
    if (existing) { set((st) => ({ groups: st.groups.map((g) => g.id === groupId ? { ...g, tabs: g.tabs.map((t) => t.id === existing.id ? { ...t, diffMeta: meta } : t), activeTabId: existing.id } : g) })); return existing }
    const tab: EditorTab = { id: nid('diff'), kind: 'diff', title, path: meta.path, dirty: false, language: meta.language ?? (meta.path ? languageForPath(meta.path) : 'plaintext'), diffMeta: meta, preview: true }
    set((st) => ({ groups: st.groups.map((g) => g.id === groupId ? { ...g, tabs: [...g.tabs.filter((t) => !(t.preview && t.kind === 'diff')), tab], activeTabId: tab.id } : g) }))
    return tab
  },

  openSpecial: (kind, opts = {}) => {
    const s = get(); const groupId = s.activeGroupId
    const group = s.groups.find((g) => g.id === groupId)!
    const match = group.tabs.find((t) => t.kind === kind && (kind === 'settings' || kind === 'keybindings' ? true : t.path === opts.path && t.url === opts.url))
    if (match) {
      set((st) => ({ groups: st.groups.map((g) => g.id === groupId ? { ...g, tabs: g.tabs.map((t) => t.id === match.id ? { ...t, settingsPage: opts.settingsPage ?? t.settingsPage, settingsQuery: opts.settingsQuery, url: opts.url ?? t.url, browserTabId: opts.browserTabId ?? t.browserTabId } : t), activeTabId: match.id } : g) }))
      return match
    }
    const titles: Record<string, string> = { settings: 'Settings', welcome: 'Welcome', keybindings: 'Keyboard Shortcuts', review: 'Review', browser: opts.url ? opts.url.replace(/^https?:\/\//, '').slice(0, 30) : 'Browser', untitled: 'Untitled', plan: opts.path ? baseName(opts.path) : 'Plan', image: opts.path ? baseName(opts.path) : 'Image', 'markdown-preview': opts.path ? `Preview ${baseName(opts.path)}` : 'Preview' }
    const tab: EditorTab = { id: nid(kind), kind, title: opts.title ?? titles[kind] ?? kind, path: opts.path, dirty: false, settingsPage: opts.settingsPage, settingsQuery: opts.settingsQuery, url: opts.url, browserTabId: opts.browserTabId, preview: opts.preview ?? (kind === 'image' || kind === 'markdown-preview'), language: kind === 'untitled' ? 'plaintext' : undefined }
    if (kind === 'untitled') { const m = monaco.editor.createModel('', 'plaintext', monaco.Uri.parse(`untitled:${tab.id}`)); models.set(`untitled:${tab.id}`, { model: m, mtime: 0, lineEnding: 'lf', savedVersion: m.getAlternativeVersionId(), lastAccess: Date.now(), disposables: [m.onDidChangeContent(() => bumpDirty(`untitled:${tab.id}`))], lineCache: new Map(), autosaveTimer: null, size: 0 }) }
    set((st) => ({ groups: st.groups.map((g) => g.id === groupId ? { ...g, tabs: [...g.tabs, tab], activeTabId: tab.id } : g) }))
    persistSession()
    return tab
  },

  setActive: (groupId, tabId) => { set((st) => ({ activeGroupId: groupId, groups: st.groups.map((g) => g.id === groupId ? { ...g, activeTabId: tabId } : g) })); persistSession() },
  setActiveGroup: (groupId) => set({ activeGroupId: groupId }),

  closeTab: async (groupId, tabId, opts = {}) => {
    const s = get(); const group = s.groups.find((g) => g.id === groupId); const tab = group?.tabs.find((t) => t.id === tabId)
    if (!group || !tab) return true
    if (tab.dirty && !opts.force) {
      const key = modelKey(tab)
      const others = s.groups.flatMap((g) => g.tabs).filter((t) => t.id !== tabId && modelKey(t) === key)
      if (!others.length) {
        const choice = await new Promise<'save' | 'discard' | 'cancel'>((resolve) => set({ confirmClose: { groupId, tabId, title: tab.title, resolve } }))
        set({ confirmClose: null })
        if (choice === 'cancel') return false
        if (choice === 'save') { const ok = tab.kind === 'untitled' ? await saveUntitled(tab) : await get().save(tab.path); if (!ok) return false }
        else if (tab.kind === 'file' && tab.path) await get().revert(tab.path)
      }
    }
    viewStates.delete(tabId)
    set((st) => {
      let groups = st.groups.map((g) => {
        if (g.id !== groupId) return g
        const idx = g.tabs.findIndex((t) => t.id === tabId)
        const tabs = g.tabs.filter((t) => t.id !== tabId)
        let activeTabId = g.activeTabId
        if (activeTabId === tabId) activeTabId = tabs[Math.min(idx, tabs.length - 1)]?.id ?? null
        return { ...g, tabs, activeTabId }
      })
      let activeGroupId = st.activeGroupId
      if (groups.length > 1) { const g = groups.find((x) => x.id === groupId)!; if (!g.tabs.length) { groups = groups.filter((x) => x.id !== groupId); if (activeGroupId === groupId) activeGroupId = groups[0].id } }
      releaseTabModel(tab, groups)
      return { groups, activeGroupId }
    })
    persistSession()
    return true
  },
  closeOthers: async (groupId, tabId) => { const g = get().groups.find((x) => x.id === groupId); if (!g) return; for (const t of [...g.tabs]) if (t.id !== tabId && !t.pinned) await get().closeTab(groupId, t.id) },
  closeToRight: async (groupId, tabId) => { const g = get().groups.find((x) => x.id === groupId); if (!g) return; const idx = g.tabs.findIndex((t) => t.id === tabId); for (const t of g.tabs.slice(idx + 1)) if (!t.pinned) await get().closeTab(groupId, t.id) },
  closeAll: async (groupId) => { for (const g of [...get().groups]) { if (groupId && g.id !== groupId) continue; for (const t of [...g.tabs]) await get().closeTab(g.id, t.id) } },
  closeGroup: async (groupId) => { await get().closeAll(groupId); set((st) => { if (st.groups.length <= 1) return {}; const groups = st.groups.filter((g) => g.id !== groupId); return { groups, activeGroupId: groups[0].id } }) },
  pinTab: (groupId, tabId, pinned) => set((st) => ({ groups: st.groups.map((g) => { if (g.id !== groupId) return g; const tabs = g.tabs.map((t) => t.id === tabId ? { ...t, pinned, preview: false } : t); tabs.sort((a, b) => Number(!!b.pinned) - Number(!!a.pinned)); return { ...g, tabs } }) })),
  keepTab: (groupId, tabId) => set((st) => ({ groups: st.groups.map((g) => g.id === groupId ? { ...g, tabs: g.tabs.map((t) => t.id === tabId ? { ...t, preview: false } : t) } : g) })),
  reorderTab: (groupId, from, to) => set((st) => ({ groups: st.groups.map((g) => { if (g.id !== groupId || from === to) return g; const tabs = [...g.tabs]; const [t] = tabs.splice(from, 1); tabs.splice(to, 0, t); return { ...g, tabs } }) })),
  moveTab: (fromGroup, tabId, toGroup, index) => set((st) => {
    if (fromGroup === toGroup) return {}
    const src = st.groups.find((g) => g.id === fromGroup); const tab = src?.tabs.find((t) => t.id === tabId); if (!src || !tab) return {}
    let groups = st.groups.map((g) => {
      if (g.id === fromGroup) { const tabs = g.tabs.filter((t) => t.id !== tabId); return { ...g, tabs, activeTabId: g.activeTabId === tabId ? tabs[0]?.id ?? null : g.activeTabId } }
      if (g.id === toGroup) { const tabs = [...g.tabs]; tabs.splice(index ?? tabs.length, 0, tab); return { ...g, tabs, activeTabId: tab.id } }
      return g
    })
    groups = groups.filter((g) => g.tabs.length || groups.length === 1)
    return { groups, activeGroupId: toGroup }
  }),
  split: () => set((st) => {
    if (st.groups.length >= 3) return {}
    const cur = st.groups.find((g) => g.id === st.activeGroupId)!
    const active = cur.tabs.find((t) => t.id === cur.activeTabId)
    const id = nid('g')
    const tabs = active ? [{ ...active, id: nid('tab'), preview: false }] : []
    const idx = st.groups.findIndex((g) => g.id === st.activeGroupId)
    const groups = [...st.groups]; groups.splice(idx + 1, 0, { id, tabs, activeTabId: tabs[0]?.id ?? null })
    return { groups, activeGroupId: id }
  }),

  save: async (path) => {
    const s = get()
    const target = path ?? activeTab(s)?.path
    if (!target) { const t = activeTab(s); if (t?.kind === 'untitled') return saveUntitled(t); return false }
    const entry = models.get(target); if (!entry) return false
    try {
      const ui = useSettings.getState().settings?.ui
      if (ui?.formatOnSave) { const ed = [...editors.values()].find((e) => e.getModel() === entry.model); if (ed) { try { await ed.getAction('editor.action.formatDocument')?.run() } catch { /* no formatter */ } } }
      const content = entry.model.getValue(monaco.editor.EndOfLinePreference.LF)
      const res = await api.fs.writeFile(target, content, { lineEnding: entry.lineEnding === 'crlf' ? 'crlf' : 'lf' })
      entry.mtime = res.mtime; entry.savedVersion = entry.model.getAlternativeVersionId(); entry.size = res.size
      bumpDirty(target)
      set((st) => ({ groups: st.groups.map((g) => ({ ...g, tabs: g.tabs.map((t) => t.path === target && t.kind === 'file' ? { ...t, dirty: false, conflict: false, notice: t.conflict ? undefined : t.notice } : t) })) }))
      busEmit('editor:saved', { path: target, content })
      const { lspManager } = await import('@/lib/lsp/manager'); lspManager.didSave(target, content)
      return true
    } catch (e) { void runCommand('app.notify', { level: 'error', title: 'Save failed', message: `${baseName(target)}: ${String((e as Error).message ?? e)}` }); return false }
  },
  saveAll: async () => { for (const [key, entry] of models) if (isDirty(entry) && !key.startsWith('untitled:')) await get().save(key) },
  revert: async (path) => { const entry = models.get(path); if (!entry) return; try { const fc = await api.fs.readFile(path); replaceModelContent(entry, fc) } catch { /* ignore */ } },
  nextTab: (dir) => { const s = get(); const g = s.groups.find((x) => x.id === s.activeGroupId)!; if (g.tabs.length < 2) return; const idx = g.tabs.findIndex((t) => t.id === g.activeTabId); const next = g.tabs[(idx + dir + g.tabs.length) % g.tabs.length]; s.setActive(g.id, next.id) },
  pushNav: (e) => set((st) => { const last = st.navBack[st.navBack.length - 1]; if (last && last.path === e.path && Math.abs(last.line - e.line) < 5) return {}; return { navBack: [...st.navBack, e].slice(-100), navForward: [] } }),
  goBack: () => { const s = get(); const cur = currentNav(); const back = [...s.navBack]; let target = back.pop(); if (target && cur && target.path === cur.path && target.line === cur.line) target = back.pop(); if (!target) return; set({ navBack: back, navForward: cur ? [cur, ...s.navForward].slice(0, 100) : s.navForward }); void s.openFile(target.path, { line: target.line, column: target.column, preview: false }) },
  goForward: () => { const s = get(); const [target, ...rest] = s.navForward; if (!target) return; const cur = currentNav(); set({ navForward: rest, navBack: cur ? [...s.navBack, cur] : s.navBack }); void s.openFile(target.path, { line: target.line, column: target.column, preview: false }) },
  setBlame: (on) => set({ blameEnabled: on }),
  setTabStatus: (tabStatus) => set({ tabStatus }),
}))

// ───────────────────────── helpers ─────────────────────────
export const baseName = (p: string) => p.split('/').pop() ?? p
const modelKey = (t: EditorTab) => t.kind === 'untitled' ? `untitled:${t.id}` : t.path ?? ''
function resolveGroup(s: EditorStore, group?: string | number): string {
  if (typeof group === 'number') return s.groups[Math.min(group, s.groups.length - 1)]?.id ?? s.activeGroupId
  if (typeof group === 'string' && s.groups.some((g) => g.id === group)) return group
  return s.activeGroupId
}
export function activeTab(s: EditorStore = useEditor.getState()): EditorTab | undefined { const g = s.groups.find((x) => x.id === s.activeGroupId); return g?.tabs.find((t) => t.id === g.activeTabId) }
export function activeEditor(): monaco.editor.IStandaloneCodeEditor | undefined { return editors.get(useEditor.getState().activeGroupId) }
export function editorForGroup(id: string) { return editors.get(id) }
export function registerEditor(groupId: string, ed: monaco.editor.IStandaloneCodeEditor): () => void { editors.set(groupId, ed); return () => { if (editors.get(groupId) === ed) editors.delete(groupId) } }
export function getModelEntry(path: string) { return models.get(path) }
export function getModel(path: string): monaco.editor.ITextModel | undefined { return models.get(path)?.model }
export function saveViewState(tabId: string, vs: monaco.editor.ICodeEditorViewState | null) { viewStates.set(tabId, vs) }
export function loadViewState(tabId: string) { return viewStates.get(tabId) ?? null }
function currentNav(): NavEntry | null { const ed = activeEditor(); const t = activeTab(); const pos = ed?.getPosition(); return t?.path && pos ? { path: t.path, line: pos.lineNumber, column: pos.column } : null }
export function isDirty(entry: ModelEntry): boolean { return entry.model.getAlternativeVersionId() !== entry.savedVersion }
function bumpDirty(key: string) {
  const entry = models.get(key); if (!entry) return
  const dirty = isDirty(entry)
  useEditor.setState((st) => {
    let changed = false
    const groups = st.groups.map((g) => ({ ...g, tabs: g.tabs.map((t) => { if (modelKey(t) !== key || t.dirty === dirty) return t; changed = true; return { ...t, dirty } }) }))
    return changed ? { groups } : {}
  })
}

/** Load (or reuse) the Monaco model for a file. Evicts cold, clean models beyond MAX_MODELS. */
export async function ensureModel(path: string): Promise<monaco.editor.ITextModel> {
  initMonaco()
  const existing = models.get(path)
  if (existing) { existing.lastAccess = Date.now(); return existing.model }
  const fc: FileContent = await api.fs.readFile(path)
  if (fc.isBinary) throw new Error('binary file')
  const uri = monaco.Uri.file(path)
  let model = monaco.editor.getModel(uri)
  if (!model) model = monaco.editor.createModel(fc.content, languageForPath(path), uri)
  else model.setValue(fc.content)
  model.setEOL(fc.lineEnding === 'crlf' ? monaco.editor.EndOfLineSequence.CRLF : monaco.editor.EndOfLineSequence.LF)
  const entry: ModelEntry = { model, mtime: fc.mtime, lineEnding: fc.lineEnding, savedVersion: model.getAlternativeVersionId(), lastAccess: Date.now(), disposables: [], lineCache: new Map(), autosaveTimer: null, size: fc.size }
  entry.disposables.push(model.onDidChangeContent((e) => onContentChange(path, entry, e)))
  models.set(path, entry)
  evictModels()
  return model
}
function evictModels() {
  if (models.size <= MAX_MODELS) return
  const openKeys = new Set(useEditor.getState().groups.flatMap((g) => g.tabs.map(modelKey)))
  const candidates = [...models.entries()].filter(([k, e]) => !openKeys.has(k) && !isDirty(e)).sort((a, b) => a[1].lastAccess - b[1].lastAccess)
  for (const [k] of candidates) { if (models.size <= MAX_MODELS) break; disposeModel(k) }
}
function disposeModel(key: string) { const e = models.get(key); if (!e) return; if (e.autosaveTimer) clearTimeout(e.autosaveTimer); e.disposables.forEach((d) => d.dispose()); e.model.dispose(); models.delete(key) }
/** Release a model when its last tab closes (clean models stay cached up to the LRU cap). */
function releaseTabModel(tab: EditorTab, groups: EditorGroup[]) {
  const key = modelKey(tab); if (!key) return
  if (groups.some((g) => g.tabs.some((t) => modelKey(t) === key))) return
  if (tab.kind === 'untitled') { disposeModel(key); return }
  const e = models.get(key); if (e && isDirty(e)) { /* keep dirty models alive (closed via force/discard revert first) */ }
  evictModels()
}
function replaceModelContent(entry: ModelEntry, fc: FileContent) {
  const m = entry.model
  m.pushEditOperations([], [{ range: m.getFullModelRange(), text: fc.content }], () => null)
  entry.mtime = fc.mtime; entry.lineEnding = fc.lineEnding; entry.savedVersion = m.getAlternativeVersionId(); entry.size = fc.size
  bumpDirty(m.uri.path)
  useEditor.setState((st) => ({ groups: st.groups.map((g) => ({ ...g, tabs: g.tabs.map((t) => t.path === m.uri.path ? { ...t, conflict: false, notice: undefined } : t) })) }))
}

function onContentChange(path: string, entry: ModelEntry, e: monaco.editor.IModelContentChangedEvent) {
  bumpDirty(path)
  // recent-edits ring: approximate `before` via cached line texts around previous edits
  const model = entry.model
  for (const c of e.changes) {
    const line = c.range.startLineNumber
    const beforeLines: string[] = []
    for (let l = c.range.startLineNumber; l <= c.range.endLineNumber; l++) beforeLines.push(entry.lineCache.get(l) ?? '')
    const endLine = Math.min(model.getLineCount(), line + Math.max(0, c.text.split('\n').length - 1))
    const afterLines: string[] = []
    for (let l = line; l <= endLine; l++) afterLines.push(model.getLineContent(l))
    const edit: RecentEdit = { path, before: beforeLines.join('\n').slice(0, 400), after: afterLines.join('\n').slice(0, 400), line, at: Date.now() }
    useEditor.setState((st) => {
      const last = st.recentEdits[0]
      if (last && last.path === path && Math.abs(last.line - line) <= 1 && Date.now() - last.at < 2000) return { recentEdits: [{ ...edit, before: last.before }, ...st.recentEdits.slice(1)] }
      return { recentEdits: [edit, ...st.recentEdits].slice(0, MAX_RECENT_EDITS) }
    })
    // refresh line cache around the edit
    entry.lineCache.clear()
    for (let l = Math.max(1, line - 3); l <= Math.min(model.getLineCount(), endLine + 3); l++) entry.lineCache.set(l, model.getLineContent(l))
  }
  busEmit('editor:changed', { path })
  const ui = useSettings.getState().settings?.ui
  if (ui?.autoSave === 'afterDelay') { if (entry.autosaveTimer) clearTimeout(entry.autosaveTimer); entry.autosaveTimer = setTimeout(() => { entry.autosaveTimer = null; if (isDirty(entry) && !path.startsWith('untitled:')) void useEditor.getState().save(path) }, ui.autoSaveDelayMs || 1000) }
}
/** Call from editors on cursor move so lineCache has "before" text for upcoming edits. */
export function primeLineCache(path: string, line: number) {
  const entry = models.get(path); if (!entry) return
  const m = entry.model
  for (let l = Math.max(1, line - 3); l <= Math.min(m.getLineCount(), line + 3); l++) if (!entry.lineCache.has(l)) entry.lineCache.set(l, m.getLineContent(l))
  if (entry.lineCache.size > 60) { const keys = [...entry.lineCache.keys()].filter((k) => Math.abs(k - line) > 10); for (const k of keys) entry.lineCache.delete(k) }
}

async function saveUntitled(tab: EditorTab): Promise<boolean> {
  const entry = models.get(`untitled:${tab.id}`); if (!entry) return false
  const { save } = await import('@tauri-apps/plugin-dialog')
  const root = useWorkspace.getState().workspace?.root
  const target = await save({ defaultPath: root ? `${root}/${tab.title === 'Untitled' ? 'untitled.txt' : tab.title}` : undefined })
  if (!target) return false
  try {
    await api.fs.writeFile(target, entry.model.getValue(), { createDirs: true })
    const st = useEditor.getState()
    const groupId = st.groups.find((g) => g.tabs.some((t) => t.id === tab.id))?.id ?? st.activeGroupId
    await st.closeTab(groupId, tab.id, { force: true })
    await st.openFile(target, { preview: false })
    return true
  } catch (e) { void runCommand('app.notify', { level: 'error', title: 'Save failed', message: String((e as Error).message ?? e) }); return false }
}

// ───────────────────────── external changes / session ─────────────────────────
let initialized = false
export function initEditorStore(): void {
  if (initialized) return
  initialized = true
  initMonaco()
  on<FsChangeEvent>(Events.fsChanged, async (ev) => {
    for (const p of ev.paths) {
      const entry = models.get(p); if (!entry) continue
      if (ev.kind === 'remove') { useEditor.setState((st) => ({ groups: st.groups.map((g) => ({ ...g, tabs: g.tabs.map((t) => t.path === p ? { ...t, notice: 'File was deleted on disk', conflict: isDirty(entry) } : t) })) })); continue }
      try {
        const stat = await api.fs.stat(p)
        if (!stat.exists || stat.mtime === entry.mtime) continue
        if (isDirty(entry)) { useEditor.setState((st) => ({ groups: st.groups.map((g) => ({ ...g, tabs: g.tabs.map((t) => t.path === p ? { ...t, conflict: true, notice: 'File changed on disk. Save to overwrite or revert to reload.' } : t) })) })); continue }
        const fc = await api.fs.readFile(p)
        replaceModelContent(entry, fc)
      } catch { /* ignore */ }
    }
  })
  window.addEventListener('blur', () => { if (useSettings.getState().settings?.ui.autoSave === 'onFocusChange') void useEditor.getState().saveAll() })
  useWorkspace.subscribe((s, prev) => { if (s.workspace?.root !== prev.workspace?.root) { void useEditor.getState().closeAll().then(() => { if (s.workspace?.root) restoreSession(s.workspace.root) }) } })
  if (useWorkspace.getState().workspace?.root) restoreSession(useWorkspace.getState().workspace!.root)
}

const sessionKey = (root: string) => `caret.editor.session.${root}`
let persistTimer: ReturnType<typeof setTimeout> | null = null
function persistSession() {
  if (persistTimer) return
  persistTimer = setTimeout(() => {
    persistTimer = null
    const root = useWorkspace.getState().workspace?.root; if (!root) return
    const s = useEditor.getState()
    const data = { groups: s.groups.map((g) => ({ tabs: g.tabs.filter((t) => t.kind === 'file' && t.path).map((t) => ({ path: t.path, pinned: t.pinned, preview: t.preview })), active: g.tabs.find((t) => t.id === g.activeTabId)?.path })), activeGroup: s.groups.findIndex((g) => g.id === s.activeGroupId) }
    try { localStorage.setItem(sessionKey(root), JSON.stringify(data)) } catch { /* quota */ }
  }, 500)
}
async function restoreSession(root: string) {
  let data: { groups: { tabs: { path: string; pinned?: boolean; preview?: boolean }[]; active?: string }[]; activeGroup: number } | null = null
  try { data = JSON.parse(localStorage.getItem(sessionKey(root)) ?? 'null') } catch { data = null }
  if (!data?.groups?.length) return
  const st = useEditor.getState()
  for (let gi = 0; gi < Math.min(3, data.groups.length); gi++) {
    if (gi > 0) st.split()
    const g = data.groups[gi]
    for (const t of g.tabs.slice(0, 12)) { try { await st.openFile(t.path, { preview: t.preview, pinned: t.pinned, focus: false }) } catch { /* gone */ } }
    if (g.active) { const grp = useEditor.getState().groups[gi]; const tab = grp?.tabs.find((x) => x.path === g.active); if (grp && tab) st.setActive(grp.id, tab.id) }
  }
  const groups = useEditor.getState().groups
  const ag = groups[Math.min(data.activeGroup ?? 0, groups.length - 1)]
  if (ag) st.setActiveGroup(ag.id)
}

/** Open files list for the agent context: paths of all file tabs (active first). */
export function openFilePaths(): string[] { const s = useEditor.getState(); const a = activeTab(s)?.path; const all = s.groups.flatMap((g) => g.tabs).filter((t) => t.kind === 'file' && t.path).map((t) => t.path!); return a ? [a, ...all.filter((p) => p !== a)] : all }
export function currentSelection(): { path: string; startLine: number; endLine: number; text: string } | null {
  const ed = activeEditor(); const t = activeTab(); const sel = ed?.getSelection(); const m = ed?.getModel()
  if (!ed || !t?.path || !sel || !m) return null
  const text = m.getValueInRange(sel)
  if (!text) { const line = sel.startLineNumber; return { path: t.path, startLine: line, endLine: line, text: m.getLineContent(line) } }
  return { path: t.path, startLine: sel.startLineNumber, endLine: sel.endColumn === 1 && sel.endLineNumber > sel.startLineNumber ? sel.endLineNumber - 1 : sel.endLineNumber, text }
}

/** Editor module bootstrap: registers commands, Monaco providers and the Cmd+K host. */
import { registerCommands } from '@/lib/commands'
import { emit as busEmit } from '@/lib/bus'
import { useSettings } from '@/store/settings'
import { monaco, initMonaco } from '@/lib/monaco/setup'
import { useEditor, initEditorStore, activeEditor, activeTab, openFilePaths, currentSelection, getModel, ensureModel } from '@/store/editor'
import { useProblems, diagnosticsSummary } from '@/store/problems'
import { registerInlineCompletions, toggleTab } from '@/lib/monaco/inlineCompletions'
import { inlineEditFor } from '@/lib/monaco/inlineEdit'
import { registerInlineEditHost } from './InlineEditWidget'
import { lspManager } from '@/lib/lsp/manager'
import { registerLspCommands } from '@/lib/lsp/providers'
import { openSymbolPicker } from './SymbolPicker'
import { api } from '@/lib/api'
import { useWorkspace } from '@/store/workspace'
import { isMarkdownPath } from '@/lib/languages'
import type { ContextItem } from '@/types'

let booted = false
export function initEditorModule(): void {
  if (booted) return
  booted = true
  initMonaco()
  initEditorStore()
  registerInlineEditHost()
  registerInlineCompletions()
  registerLspCommands()
  lspManager.init()

  const st = () => useEditor.getState()
  const act = (id: string) => () => { void activeEditor()?.getAction(id)?.run() }
  wireBusTopics()
  registerCommands([
    { id: 'editor.getOpenEditors', title: 'Open Editors (detailed)', hidden: true, run: () => openEditorInfos() },
    { id: 'editor.openFile', title: 'Open File', hidden: true, run: (a) => { const o = (a ?? {}) as { path: string; line?: number; column?: number; endLine?: number; preview?: boolean; focus?: boolean; group?: number }; return st().openFile(o.path, o) } },
    { id: 'editor.openDiff', title: 'Open Diff', hidden: true, run: (a) => { const o = a as { path: string; original: string; modified: string; title?: string; language?: string; changeId?: string; sessionId?: string }; return st().openDiff({ ...o, originalLabel: o.changeId ? 'Before' : 'HEAD', modifiedLabel: o.changeId ? 'After' : 'Working tree' }) } },
    { id: 'editor.closeTab', title: 'Close Editor', category: 'View', keybinding: 'mod+w', run: () => { const s = st(); const g = s.groups.find((x) => x.id === s.activeGroupId); if (g?.activeTabId) return s.closeTab(g.id, g.activeTabId); if (s.groups.length > 1) return s.closeGroup(s.activeGroupId) } },
    { id: 'editor.closeAllTabs', title: 'Close All Editors', category: 'View', keybinding: 'mod+k mod+w', run: () => st().closeAll() },
    { id: 'editor.closeOthers', title: 'Close Other Editors', category: 'View', run: () => { const s = st(); const g = s.groups.find((x) => x.id === s.activeGroupId); if (g?.activeTabId) return s.closeOthers(g.id, g.activeTabId) } },
    { id: 'editor.reopenClosed', title: 'Reopen Closed Editor', category: 'View', keybinding: 'mod+shift+t', run: () => { const s = st(); const open = new Set(openFilePaths()); const p = s.recentFiles.find((f) => !open.has(f)); if (p) return s.openFile(p, { preview: false }) } },
    { id: 'editor.split', title: 'Split Editor Right', category: 'View', keybinding: 'mod+\\', run: () => st().split() },
    { id: 'editor.focusGroup', title: 'Focus Editor Group', hidden: true, run: (a) => { const i = typeof a === 'number' ? a : (a as { index?: number })?.index ?? 0; const g = st().groups[i]; if (g) { st().setActiveGroup(g.id); requestAnimationFrame(() => activeEditor()?.focus()) } } },
    { id: 'editor.focusGroup1', title: 'Focus 1st Editor Group', category: 'View', keybinding: 'mod+1', run: () => { const g = st().groups[0]; if (g) { st().setActiveGroup(g.id); activeEditor()?.focus() } } },
    { id: 'editor.focusGroup2', title: 'Focus 2nd Editor Group', category: 'View', keybinding: 'mod+2', run: () => { const g = st().groups[1]; if (g) { st().setActiveGroup(g.id); activeEditor()?.focus() } } },
    { id: 'editor.focusGroup3', title: 'Focus 3rd Editor Group', category: 'View', keybinding: 'mod+3', run: () => { const g = st().groups[2]; if (g) { st().setActiveGroup(g.id); activeEditor()?.focus() } } },
    { id: 'editor.nextTab', title: 'Next Editor', category: 'View', keybinding: 'mod+alt+right', run: () => st().nextTab(1) },
    { id: 'editor.prevTab', title: 'Previous Editor', category: 'View', keybinding: 'mod+alt+left', run: () => st().nextTab(-1) },
    { id: 'editor.pinTab', title: 'Pin Editor', category: 'View', run: () => { const s = st(); const g = s.groups.find((x) => x.id === s.activeGroupId); const t = activeTab(); if (g && t) s.pinTab(g.id, t.id, !t.pinned) } },
    { id: 'editor.goToLine', title: 'Go to Line/Column…', category: 'Go', keybinding: 'ctrl+g', run: act('editor.action.gotoLine') },
    { id: 'editor.goToSymbol', title: 'Go to Symbol in Editor…', category: 'Go', keybinding: 'mod+shift+o', run: () => openSymbolPicker() },
    { id: 'editor.goToDefinition', title: 'Go to Definition', category: 'Go', keybinding: 'f12', run: act('editor.action.revealDefinition') },
    { id: 'editor.peekDefinition', title: 'Peek Definition', category: 'Go', keybinding: 'alt+f12', run: act('editor.action.peekDefinition') },
    { id: 'editor.goToTypeDefinition', title: 'Go to Type Definition', category: 'Go', run: act('editor.action.goToTypeDefinition') },
    { id: 'editor.goToImplementation', title: 'Go to Implementations', category: 'Go', keybinding: 'mod+f12', run: act('editor.action.goToImplementation') },
    { id: 'editor.findReferences', title: 'Go to References', category: 'Go', keybinding: 'shift+f12', run: act('editor.action.goToReferences') },
    { id: 'editor.rename', title: 'Rename Symbol', category: 'Edit', keybinding: 'f2', run: act('editor.action.rename') },
    { id: 'editor.format', title: 'Format Document', category: 'Edit', keybinding: 'shift+alt+f', run: act('editor.action.formatDocument') },
    { id: 'editor.formatSelection', title: 'Format Selection', category: 'Edit', keybinding: 'mod+k mod+f', run: act('editor.action.formatSelection') },
    { id: 'editor.quickFix', title: 'Quick Fix…', category: 'Edit', keybinding: 'mod+.', run: act('editor.action.quickFix') },
    { id: 'editor.find', title: 'Find', category: 'Edit', keybinding: 'mod+f', run: act('actions.find') },
    { id: 'editor.replace', title: 'Replace', category: 'Edit', keybinding: 'mod+alt+f', run: act('editor.action.startFindReplaceAction') },
    { id: 'editor.toggleComment', title: 'Toggle Line Comment', category: 'Edit', keybinding: 'mod+/', run: act('editor.action.commentLine') },
    { id: 'editor.toggleMinimap', title: 'Toggle Minimap', category: 'View', run: async () => { const { useSettings } = await import('@/store/settings'); const cur = useSettings.getState().settings?.ui.minimap ?? true; await useSettings.getState().update({ ui: { minimap: !cur } }) } },
    { id: 'editor.toggleWordWrap', title: 'Toggle Word Wrap', category: 'View', keybinding: 'alt+z', run: async () => { const { useSettings } = await import('@/store/settings'); const cur = useSettings.getState().settings?.ui.wordWrap ?? 'off'; await useSettings.getState().update({ ui: { wordWrap: cur === 'off' ? 'on' : 'off' } }) } },
    { id: 'editor.toggleStickyScroll', title: 'Toggle Sticky Scroll', category: 'View', run: async () => { const { useSettings } = await import('@/store/settings'); const cur = useSettings.getState().settings?.ui.stickyScroll ?? true; await useSettings.getState().update({ ui: { stickyScroll: !cur } }) } },
    { id: 'editor.revealRange', title: 'Reveal Range', hidden: true, run: (a) => { const o = a as { path: string; startLine: number; endLine: number }; return st().openFile(o.path, { line: o.startLine, endLine: o.endLine, preview: true }) } },
    { id: 'editor.openWelcome', title: 'Welcome', category: 'Help', run: () => st().openSpecial('welcome') },
    { id: 'editor.openPlan', title: 'Open Plan', hidden: true, run: (a) => st().openSpecial('plan', { path: (a as { path: string }).path }) },
    { id: 'editor.openBrowser', title: 'Open Browser', category: 'View', run: (a) => st().openSpecial('browser', { url: (a as { url?: string } | undefined)?.url, browserTabId: (a as { tabId?: string } | undefined)?.tabId }) },
    { id: 'editor.openImage', title: 'Open Image', hidden: true, run: (a) => st().openSpecial('image', { path: (a as { path: string }).path }) },
    { id: 'editor.openReview', title: 'Open Review Tab', hidden: true, run: () => st().openSpecial('review') },
    { id: 'editor.markdownPreview', title: 'Open Markdown Preview', category: 'View', keybinding: 'mod+shift+v', run: (a) => { const p = (a as { path?: string } | undefined)?.path ?? activeTab()?.path; if (p && isMarkdownPath(p)) { st().openSpecial('markdown-preview', { path: p }) } } },
    { id: 'editor.toggleBlame', title: 'Toggle Inline Git Blame', category: 'Git', run: () => st().setBlame(!st().blameEnabled) },
    { id: 'editor.nextProblem', title: 'Go to Next Problem', category: 'Go', keybinding: 'f8', run: act('editor.action.marker.nextInFiles') },
    { id: 'editor.prevProblem', title: 'Go to Previous Problem', category: 'Go', keybinding: 'shift+f8', run: act('editor.action.marker.prevInFiles') },
    { id: 'editor.selectAllOccurrences', title: 'Select All Occurrences', category: 'Edit', keybinding: 'mod+shift+l', hidden: true, run: act('editor.action.selectHighlights') },
    { id: 'editor.foldAll', title: 'Fold All', category: 'View', keybinding: 'mod+k mod+0', run: act('editor.foldAll') },
    { id: 'editor.unfoldAll', title: 'Unfold All', category: 'View', keybinding: 'mod+k mod+j', run: act('editor.unfoldAll') },
    { id: 'editor.changeLanguage', title: 'Change Language Mode', category: 'View', keybinding: 'mod+k m', run: () => { const m = activeEditor()?.getModel(); if (!m) return; const langs = monaco.languages.getLanguages().map((l) => l.id); const next = window.prompt(`Language id (${langs.length} available), current: ${m.getLanguageId()}`); if (next && langs.includes(next)) monaco.editor.setModelLanguage(m, next) } },
    // navigation
    { id: 'go.back', title: 'Go Back', category: 'Go', keybinding: 'ctrl+-', run: () => st().goBack() },
    { id: 'go.forward', title: 'Go Forward', category: 'Go', keybinding: 'ctrl+shift+-', run: () => st().goForward() },
    { id: 'go.symbol', title: 'Go to Symbol in Editor…', hidden: true, run: () => openSymbolPicker() },
    { id: 'go.line', title: 'Go to Line…', hidden: true, run: act('editor.action.gotoLine') },
    { id: 'go.definition', title: 'Go to Definition', hidden: true, run: act('editor.action.revealDefinition') },
    { id: 'go.references', title: 'Go to References', hidden: true, run: act('editor.action.goToReferences') },
    // files
    { id: 'file.new', title: 'New Untitled File', category: 'File', keybinding: 'mod+n', run: () => st().openSpecial('untitled') },
    { id: 'file.save', title: 'Save', category: 'File', keybinding: 'mod+s', run: () => st().save() },
    { id: 'file.saveAll', title: 'Save All', category: 'File', keybinding: 'mod+alt+s', run: () => st().saveAll() },
    { id: 'file.revert', title: 'Revert File', category: 'File', run: () => { const p = activeTab()?.path; if (p) return st().revert(p) } },
    { id: 'file.copyPath', title: 'Copy Path of Active File', category: 'File', run: () => { const p = activeTab()?.path; if (p) void navigator.clipboard.writeText(p) } },
    { id: 'file.copyRelativePath', title: 'Copy Relative Path of Active File', category: 'File', run: async () => { const p = activeTab()?.path; if (p) { const { relPath } = await import('@/store/workspace'); void navigator.clipboard.writeText(relPath(p)) } } },
    // settings tabs
    { id: 'settings.open', title: 'Open Settings', category: 'Preferences', keybinding: 'mod+,', run: (a) => { const o = (a ?? {}) as { page?: string; query?: string }; st().openSpecial('settings', { settingsPage: o.page, settingsQuery: o.query }) } },
    { id: 'settings.caret', title: 'Caret Settings (AI)', category: 'Preferences', keybinding: 'mod+shift+j', run: () => st().openSpecial('settings', { settingsPage: 'models' }) },
    { id: 'settings.keybindings', title: 'Open Keyboard Shortcuts', category: 'Preferences', keybinding: 'mod+k mod+s', run: () => st().openSpecial('keybindings') },
    { id: 'settings.models', title: 'Settings: Models', hidden: true, run: () => st().openSpecial('settings', { settingsPage: 'models' }) },
    { id: 'settings.rules', title: 'Settings: Rules', hidden: true, run: () => st().openSpecial('settings', { settingsPage: 'rules' }) },
    { id: 'settings.mcp', title: 'Settings: MCP', hidden: true, run: () => st().openSpecial('settings', { settingsPage: 'mcp' }) },
    { id: 'settings.indexing', title: 'Settings: Indexing & Docs', hidden: true, run: () => st().openSpecial('settings', { settingsPage: 'indexing' }) },
    // AI
    { id: 'ai.inlineEdit', title: 'Inline Edit (Cmd+K)', category: 'AI', keybinding: 'mod+k', run: async (a) => { const o = (a ?? {}) as { path?: string; instruction?: string; mode?: 'edit' | 'question' | 'generate' }; if (o.path && activeTab()?.path !== o.path) await st().openFile(o.path, { preview: false }); const ed = activeEditor(); if (!ed || !ed.getModel()) return; inlineEditFor(ed).open({ instruction: o.instruction, mode: o.mode }) } },
    { id: 'ai.quickQuestion', title: 'Quick Question about Selection', category: 'AI', run: () => { const ed = activeEditor(); if (ed?.getModel()) inlineEditFor(ed).open({ mode: 'question' }) } },
    { id: 'ai.tab.accept', title: 'Accept Inline Suggestion', hidden: true, run: () => activeEditor()?.trigger('caret', 'editor.action.inlineSuggest.commit', {}) },
    { id: 'ai.tab.acceptWord', title: 'Accept Next Word of Inline Suggestion', hidden: true, run: () => activeEditor()?.trigger('caret', 'editor.action.inlineSuggest.acceptNextWord', {}) },
    { id: 'ai.tab.acceptLine', title: 'Accept Next Line of Inline Suggestion', hidden: true, run: () => activeEditor()?.trigger('caret', 'editor.action.inlineSuggest.acceptNextLine', {}) },
    { id: 'ai.tab.trigger', title: 'Trigger Tab Suggestion', category: 'AI', keybinding: 'alt+\\', run: () => activeEditor()?.trigger('caret', 'editor.action.inlineSuggest.trigger', {}) },
    { id: 'ai.tab.toggle', title: 'Toggle Tab Completions', category: 'AI', run: () => toggleTab() },
    { id: 'ai.explain', title: 'Explain Selection', category: 'AI', run: async () => { const sel = currentSelection(); if (!sel) return; const { runCommand } = await import('@/lib/commands'); const ctx: ContextItem = { id: `code-${Date.now()}`, kind: 'code', label: `${sel.path.split('/').pop()}:${sel.startLine}-${sel.endLine}`, path: sel.path, range: { startLine: sel.startLine, endLine: sel.endLine }, data: sel.text }; await runCommand('chat.sendPrompt', { text: 'Explain this code: what it does, how it works, and anything surprising or risky.', context: [ctx], mode: 'ask', submit: true }) } },
    { id: 'ai.explainFile', title: 'Explain Current File', category: 'AI', run: async () => { const p = activeTab()?.path; if (!p) return; const { runCommand } = await import('@/lib/commands'); await runCommand('chat.sendPrompt', { text: 'Walk me through this file.', context: [{ id: `file-${p}`, kind: 'file', label: p.split('/').pop()!, path: p }], mode: 'ask', submit: true }) } },
    // data commands for other modules
    { id: 'editor.getActiveFile', title: 'Active File', hidden: true, run: () => { const t = activeTab(); const ed = activeEditor(); const pos = ed?.getPosition(); return t?.path ? { path: t.path, language: t.language, line: pos?.lineNumber, column: pos?.column, dirty: t.dirty } : null } },
    { id: 'editor.getSelection', title: 'Selection', hidden: true, run: () => currentSelection() },
    { id: 'editor.getOpenFiles', title: 'Open Files', hidden: true, run: () => openFilePaths() },
    { id: 'editor.getRecentEdits', title: 'Recent Edits', hidden: true, run: () => st().recentEdits },
    { id: 'editor.getDiagnosticsSummary', title: 'Diagnostics Summary', hidden: true, run: (a) => diagnosticsSummary((a as { paths?: string[] } | undefined)?.paths) },
    { id: 'editor.getProblems', title: 'Problems', hidden: true, run: (a) => { const p = (a as { path?: string } | undefined)?.path; return p ? useProblems.getState().forPath(p) : useProblems.getState().byPath } },
    { id: 'editor.insertAtCursor', title: 'Insert at Cursor', hidden: true, run: (a) => { const ed = activeEditor(); const text = (a as { text: string }).text; if (!ed) return false; const pos = ed.getPosition(); if (!pos) return false; ed.executeEdits('caret', [{ range: new monaco.Range(pos.lineNumber, pos.column, pos.lineNumber, pos.column), text }]); ed.focus(); return true } },
    { id: 'editor.replaceSelection', title: 'Replace Selection', hidden: true, run: (a) => { const ed = activeEditor(); const text = (a as { text: string }).text; const sel = ed?.getSelection(); if (!ed || !sel) return false; ed.executeEdits('caret', [{ range: sel, text }]); ed.focus(); return true } },
    { id: 'editor.getText', title: 'Get Text', hidden: true, run: async (a) => { const p = (a as { path: string }).path; const m = getModel(p); if (m) return m.getValue(); try { return (await api.fs.readFile(p)).content } catch { return null } } },
    { id: 'editor.applyEdit', title: 'Apply Edit', hidden: true, run: async (a) => {
      const o = a as { path: string; startLine?: number; endLine?: number; text: string; open?: boolean }
      const model = getModel(o.path) ?? (await ensureModel(o.path).catch(() => null))
      if (!model) return null
      const before = model.getValue()
      const range = o.startLine && o.endLine ? new monaco.Range(o.startLine, 1, o.endLine, model.getLineMaxColumn(o.endLine)) : model.getFullModelRange()
      model.pushEditOperations([], [{ range, text: o.text }], () => null)
      if (o.open !== false) await st().openFile(o.path, { line: o.startLine, preview: false })
      return { before, after: model.getValue() }
    } },
    { id: 'editor.reloadFile', title: 'Reload File from Disk', hidden: true, run: (a) => { const p = (a as { path: string }).path; return st().revert(p) } },
  ])
}


// ───────── bus topics consumed by the shell (status bar, activity bar, explorer) ─────────
export interface OpenEditorInfo { path: string; dirty: boolean; active: boolean; preview: boolean; title: string; groupId: string; tabId: string }
export function openEditorInfos(): OpenEditorInfo[] {
  const s = useEditor.getState()
  const out: OpenEditorInfo[] = []
  for (const g of s.groups) for (const t of g.tabs) if (t.path && t.kind === 'file') out.push({ path: t.path, dirty: t.dirty, active: g.id === s.activeGroupId && g.activeTabId === t.id, preview: !!t.preview, title: t.title, groupId: g.id, tabId: t.id })
  return out
}
let busWired = false
function wireBusTopics(): void {
  if (busWired) return
  busWired = true
  let lastTabsKey = ''
  let lastActive = ''
  let lastCursor = ''
  let lastTabStatus = ''
  const publish = () => {
    const s = useEditor.getState()
    const tabsKey = s.groups.map((g) => g.id + ':' + g.activeTabId + ':' + g.tabs.map((t) => t.id + (t.dirty ? '*' : '') + (t.preview ? '?' : '')).join(',')).join('|')
    if (tabsKey !== lastTabsKey) { lastTabsKey = tabsKey; busEmit('editor.tabs', openEditorInfos()) }
    const g = s.groups.find((x) => x.id === s.activeGroupId)
    const tab = g?.tabs.find((t) => t.id === g.activeTabId)
    const activeKey = (tab?.path ?? '') + '#' + (tab?.language ?? '') + '#' + (tab?.kind ?? '')
    if (activeKey !== lastActive) {
      lastActive = activeKey
      busEmit('editor.active', tab?.path ? { path: tab.path, language: tab.language, kind: tab.kind } : null)
      const ui = useSettings.getState().settings?.ui
      busEmit('editor.status', tab?.path ? { language: tab.language ?? 'plaintext', indent: ui ? (ui.insertSpaces ? `Spaces: ${ui.tabSize}` : 'Tabs') : undefined, encoding: 'UTF-8' } : null)
    }
    const cursorKey = `${s.cursor.line}:${s.cursor.column}:${s.cursor.selected}`
    if (cursorKey !== lastCursor) { lastCursor = cursorKey; busEmit('editor.cursor', tab?.path ? { line: s.cursor.line, column: s.cursor.column, selection: s.cursor.selected || undefined } : null) }
    const enabled = useSettings.getState().settings?.tab.enabled ?? true
    const tabKey = `${enabled}:${s.tabStatus}`
    if (tabKey !== lastTabStatus) { lastTabStatus = tabKey; busEmit('tab.status', { enabled: enabled && s.tabStatus !== 'off', busy: s.tabStatus === 'loading' }) }
  }
  useEditor.subscribe(publish)
  useSettings.subscribe(publish)
  let lastDiag = ''
  const publishDiag = () => { const p = useProblems.getState(); const k = `${p.errors}:${p.warnings}`; if (k !== lastDiag) { lastDiag = k; busEmit('diagnostics.summary', { errors: p.errors, warnings: p.warnings }) } }
  useProblems.subscribe(publishDiag)
  publish(); publishDiag()
}

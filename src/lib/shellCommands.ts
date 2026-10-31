/** Shell-owned commands: workspace, views, palette, app, help, settings shortcuts. */
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { createElement } from 'react'
import { api } from '@/lib/api'
import { registerCommands, runCommand, hasCommand, listCommands } from '@/lib/commands'
import { listBindings, formatKeybinding, getKeybindingFor } from '@/lib/keybindings'
import { THEMES } from '@/lib/theme'
import { useUi } from '@/store/ui'
import { useWorkspace } from '@/store/workspace'
import { useSettings } from '@/store/settings'
import { notify } from '@/store/notifications'
import { pick, showDialog, confirm } from '@/components/dialogs'
import { registerExplorerCommands } from '@/components/explorer'
import { registerSearchCommands } from '@/components/search'
import { registerTerminalCommands } from '@/components/terminal'
import { registerTerminalHistoryCommands } from '@/components/terminal/historyCommands'
import type { AutoRunMode } from '@/types'

export function registerShellCommands(): () => void {
  const ui = () => useUi.getState()
  const offs = [registerExplorerCommands(), registerSearchCommands(), registerTerminalCommands()]
  offs.push(registerTerminalHistoryCommands())
  offs.push(registerCommands([
    // ── workspace / files ──
    { id: 'workspace.openFolder', title: 'Open Folder…', category: 'File', keybinding: 'mod+o', run: async (args) => {
      let path = (args as { path?: string } | undefined)?.path
      if (!path) { const p = await openDialog({ directory: true, multiple: false, title: 'Open Folder' }); if (typeof p !== 'string') return; path = p }
      try { await useWorkspace.getState().open(path); ui().set({ layout: 'editor', settingsOpen: false }); if (!ui().sidebarVisible) ui().showSidebar('explorer') } catch (e) { notify('error', 'Could not open folder', String((e as Error).message ?? e)) }
    } },
    { id: 'workspace.closeFolder', title: 'Close Folder', category: 'File', run: async () => { await useWorkspace.getState().close() } },
    { id: 'workspace.openRecent', title: 'Open Recent…', category: 'File', run: async () => {
      const recent = useWorkspace.getState().recent.filter((r) => r.exists)
      if (!recent.length) { notify('info', 'No recent folders'); return }
      const id = await pick({ title: 'Open Recent', items: recent.map((r) => ({ id: r.path, label: r.name, description: r.path })) })
      if (id) await runCommand('workspace.openFolder', { path: id })
    } },
    { id: 'file.revealInFinder', title: 'Reveal Active File in Finder', category: 'File', run: async () => { const r = await runCommand<{ path?: string } | string | undefined>('editor.getActiveFile'); const p = typeof r === 'string' ? r : r?.path ?? useWorkspace.getState().workspace?.root; if (p) await api.app.showInFolder(p) } },
    { id: 'file.newWindow', title: 'New Window', category: 'File', run: () => notify('info', 'Multiple windows are not available yet', 'Open another folder in this window via File → Open Folder.') },
    // ── views ──
    { id: 'view.explorer', title: 'Show Explorer', category: 'View', keybinding: 'mod+shift+e', run: () => { ui().set({ layout: 'editor' }); ui().showSidebar('explorer'); setTimeout(() => document.querySelector<HTMLElement>('[data-keyctx=explorer]')?.focus(), 30) } },
    { id: 'view.search', title: 'Show Search', category: 'View', run: () => { ui().set({ layout: 'editor' }); ui().showSidebar('search') } },
    { id: 'view.scm', title: 'Show Source Control', category: 'View', keybinding: 'ctrl+shift+g', run: () => { ui().set({ layout: 'editor' }); ui().showSidebar('scm') } },
    { id: 'view.agents', title: 'Show Agents', category: 'View', keybinding: 'mod+shift+a', run: () => { ui().set({ layout: 'editor' }); ui().showSidebar('agents') } },
    { id: 'view.rules', title: 'Show Rules & Context', category: 'View', run: () => ui().showSidebar('rules') },
    { id: 'view.outline', title: 'Show Outline', category: 'View', run: () => ui().showSidebar('outline') },
    { id: 'view.toggleSidebar', title: 'Toggle Primary Sidebar', category: 'View', keybinding: 'mod+b', run: () => ui().toggleSidebar() },
    { id: 'view.togglePanel', title: 'Toggle Panel', category: 'View', keybinding: 'mod+j', run: () => ui().togglePanel() },
    { id: 'view.toggleChat', title: 'Toggle AI Pane', category: 'View', keybinding: 'mod+shift+i', run: () => ui().toggleChat() },
    { id: 'view.problems', title: 'Show Problems', category: 'View', keybinding: 'mod+shift+m', run: () => ui().togglePanel('problems') },
    { id: 'view.output', title: 'Show Output', category: 'View', keybinding: 'mod+shift+u', run: () => ui().togglePanel('output') },
    { id: 'view.review', title: 'Show Review Panel', category: 'View', run: () => ui().showPanel('review') },
    { id: 'view.ports', title: 'Show Ports', category: 'View', run: () => ui().showPanel('ports') },
    { id: 'view.debugConsole', title: 'Show Debug Console', category: 'View', run: () => ui().showPanel('debugConsole') },
    { id: 'view.toggleAgentsLayout', title: 'Toggle Agents Layout', category: 'View', keybinding: 'mod+alt+a', run: () => ui().set({ layout: ui().layout === 'agents' ? 'editor' : 'agents' }) },
    { id: 'view.zen', title: 'Toggle Zen Mode', category: 'View', keybinding: 'mod+k z', run: () => ui().set({ zenMode: !ui().zenMode }) },
    { id: 'view.zoomIn', title: 'Zoom In', category: 'View', keybinding: 'mod+=', run: () => ui().setZoom(ui().zoom + 1) },
    { id: 'view.zoomOut', title: 'Zoom Out', category: 'View', keybinding: 'mod+-', run: () => ui().setZoom(ui().zoom - 1) },
    { id: 'view.zoomReset', title: 'Reset Zoom', category: 'View', keybinding: 'mod+0', run: () => ui().setZoom(0) },
    { id: 'view.focusSidebar', title: 'Focus into Primary Sidebar', category: 'View', run: () => { if (!ui().sidebarVisible) ui().set({ sidebarVisible: true }); setTimeout(() => (document.querySelector<HTMLElement>('[data-keyctx=sidebar] [tabindex], [data-keyctx=sidebar] input'))?.focus(), 30) } },
    { id: 'view.focusEditor', title: 'Focus Editor', category: 'View', run: () => { if (hasCommand('editor.focus')) void runCommand('editor.focus'); else document.querySelector<HTMLElement>('.monaco-editor textarea')?.focus() } },
    { id: 'view.focusPanel', title: 'Focus Panel', category: 'View', run: () => { if (!ui().panelVisible) ui().set({ panelVisible: true }); if (ui().panelView === 'terminal') void runCommand('terminal.focus') } },
    // ── palette ──
    { id: 'palette.commands', title: 'Show All Commands', category: 'View', keybinding: 'mod+shift+p', run: () => ui().openPalette('commands', '>') },
    { id: 'palette.files', title: 'Go to File…', category: 'Go', keybinding: 'mod+p', run: () => ui().openPalette('files') },
    { id: 'palette.recent', title: 'Go to Recent File…', category: 'Go', run: () => ui().openPalette('recent') },
    // ── settings shortcuts ──
    { id: 'settings.open', title: 'Open Settings', category: 'Preferences', keybinding: 'mod+,', run: (args) => { const a = (args ?? {}) as { page?: string; query?: string }; ui().openSettings(a.page, a.query) } },
    { id: 'settings.keybindings', title: 'Open Keyboard Shortcuts', category: 'Preferences', keybinding: 'mod+k mod+s', run: () => ui().openSettings('keybindings') },
    { id: 'settings.mcp', title: 'MCP Servers', category: 'AI', run: () => ui().openSettings('mcp') },
    { id: 'settings.rules', title: 'Rules', category: 'AI', run: () => ui().openSettings('rules') },
    { id: 'settings.indexing', title: 'Codebase Indexing & Docs', category: 'AI', run: () => ui().openSettings('indexing') },
    { id: 'settings.models', title: 'Models & Providers', category: 'AI', run: () => ui().openSettings('models') },
    { id: 'settings.openJson', title: 'Open User Settings (JSON)', category: 'Preferences', run: async () => { const p = await api.settings.path('user'); await runCommand('editor.openFile', { path: p, focus: true }) } },
    { id: 'settings.openWorkspaceJson', title: 'Open Workspace Settings (JSON)', category: 'Preferences', run: async () => { try { const p = await api.settings.path('workspace'); await runCommand('editor.openFile', { path: p, focus: true }) } catch (e) { notify('warn', 'No workspace open', String((e as Error).message ?? e)) } } },
    // ── app ──
    { id: 'app.notify', title: 'Show Notification', category: 'Developer', hidden: true, run: (args) => { const a = args as { level: 'info' | 'warn' | 'error' | 'success'; title: string; message?: string; timeoutMs?: number }; notify(a.level, a.title, a.message, { timeoutMs: a.timeoutMs }) } },
    { id: 'app.reload', title: 'Reload Window', category: 'Developer', run: () => location.reload() },
    { id: 'app.devtools', title: 'Toggle Developer Tools', category: 'Developer', run: () => void api.app.openDevtools() },
    { id: 'app.openConfigFolder', title: 'Open Caret Config Folder', category: 'Developer', run: async () => { const i = await api.app.info(); await api.app.showInFolder(i.configDir) } },
    { id: 'app.toggleTheme', title: 'Toggle Color Theme', category: 'Preferences', keybinding: 'mod+k mod+t', run: async () => {
      const cur = useSettings.getState().settings?.ui.theme ?? 'caret-dark'
      const id = await pick({ title: 'Select Color Theme', items: THEMES.map((t) => ({ id: t.id, label: t.label, description: t.kind, detail: t.id === cur ? 'current' : undefined })) })
      if (id) await useSettings.getState().update({ ui: { theme: id as typeof cur } })
    } },
    { id: 'agent.toggleAutoRun', title: 'Agent: Cycle Auto-Run Mode', category: 'AI', run: async () => {
      const cur = useSettings.getState().settings?.agent.autoRun ?? 'allowlist'
      const next: AutoRunMode = cur === 'off' ? 'allowlist' : cur === 'allowlist' ? 'all' : 'off'
      await useSettings.getState().update({ agent: { autoRun: next } })
      notify('info', `Auto-run: ${next === 'all' ? 'all commands (YOLO)' : next === 'allowlist' ? 'allowlisted commands only' : 'off (ask every time)'}`)
    } },
    // ── help ──
    { id: 'help.shortcuts', title: 'Keyboard Shortcuts Reference', category: 'Help', run: () => {
      const rows = listBindings().map((b) => ({ key: b.key, command: b.command, title: listCommands().find((c) => c.id === b.command)?.title ?? b.command })).sort((a, b) => a.title.localeCompare(b.title))
      void showDialog('Keyboard Shortcuts', () => createElement('div', { className: 'text-xs' },
        createElement('div', { className: 'text-muted mb-2' }, 'Customize in Settings → Keyboard Shortcuts (⌘K ⌘S) or ~/.caret/keybindings.json'),
        createElement('table', { className: 'w-full' }, createElement('tbody', null, ...rows.map((r) => createElement('tr', { key: r.key + r.command, className: 'border-b border-border h-6' }, createElement('td', { className: 'pr-3' }, r.title), createElement('td', { className: 'text-faint mono pr-3' }, r.command), createElement('td', { className: 'text-right text-muted' }, formatKeybinding(r.key))))))), { wide: true })
    } },
    { id: 'help.about', title: 'About Caret', category: 'Help', run: async () => {
      const [info, stats] = await Promise.all([api.app.info(), api.app.processStats().catch(() => null)])
      void showDialog('About Caret', () => createElement('div', { className: 'text-xs flex flex-col gap-1' },
        createElement('div', { className: 'text-base font-semibold' }, `Caret ${info.version}`),
        createElement('div', { className: 'text-muted' }, 'A memory-efficient, Rust-powered AI code editor (Tauri 2 + Monaco).'),
        createElement('div', null, `Platform: ${info.os}/${info.arch} · WebView ${info.webview}`),
        createElement('div', null, `Config: ${info.configDir}`),
        stats ? createElement('div', null, `Core process: ${(stats.rssBytes / 1048576).toFixed(1)} MB RSS · CPU ${stats.cpuPercent.toFixed(1)}% · up ${Math.round(stats.uptimeSecs / 60)} min`) : null))
    } },
    { id: 'help.welcome', title: 'Welcome', category: 'Help', run: () => { if (hasCommand('editor.openWelcome')) void runCommand('editor.openWelcome'); else notify('info', 'Welcome', 'Open a folder to get started.') } },
    { id: 'help.docs', title: 'Documentation', category: 'Help', run: () => void api.app.openExternal('https://github.com/Fast-Editor/Caret/tree/main/docs#readme') },
    { id: 'help.reportIssue', title: 'Report Issue', category: 'Help', run: () => void api.app.openExternal('https://github.com/Fast-Editor/Caret/issues/new') },
    { id: 'app.checkForUpdates', title: 'Check for Updates…', category: 'Help', run: async () => {
      const { checkForUpdates } = await import('@/lib/updater')
      const st = await checkForUpdates()
      if (st.kind === 'available') { notify('info', `Caret ${st.version} is available`, 'Install it from Settings → About.'); void runCommand('settings.open', { page: 'about' }) }
      else if (st.kind === 'none') notify('success', 'You are on the latest version')
      else if (st.kind === 'error') notify('warn', 'Could not check for updates', st.message)
    } },
    { id: 'app.quitConfirm', title: 'Quit', category: 'File', hidden: true, run: async () => { if (await confirm({ title: 'Quit Caret?' })) { const { exit } = await import('@tauri-apps/plugin-process'); await exit(0) } } },
  ]))
  // Expose default keybinding on palette rows for commands registered without one.
  for (const c of listCommands()) if (!c.keybinding) { const k = getKeybindingFor(c.id); if (k) c.keybinding = k }
  return () => offs.forEach((o) => o())
}

/** Native menu id → command id/args. */
export const MENU_MAP: Record<string, { id: string; args?: unknown }> = {
  'settings.open': { id: 'settings.open' }, 'settings.caret': { id: 'settings.open', args: { page: 'agent' } },
  'file.newFile': { id: 'file.new' }, 'file.newWindow': { id: 'file.newWindow' }, 'file.openFolder': { id: 'workspace.openFolder' }, 'file.openRecent': { id: 'workspace.openRecent' }, 'file.closeFolder': { id: 'workspace.closeFolder' },
  'file.save': { id: 'file.save' }, 'file.saveAll': { id: 'file.saveAll' }, 'file.revealInFinder': { id: 'file.revealInFinder' },
  'edit.find': { id: 'editor.find' }, 'edit.replace': { id: 'editor.replace' }, 'edit.findInFiles': { id: 'search.findInFiles' },
  'view.commandPalette': { id: 'palette.commands' }, 'view.quickOpen': { id: 'palette.files' }, 'view.explorer': { id: 'view.explorer' }, 'view.search': { id: 'view.search' }, 'view.scm': { id: 'view.scm' }, 'view.agents': { id: 'view.agents' },
  'view.toggleSidebar': { id: 'view.toggleSidebar' }, 'view.toggleChat': { id: 'view.toggleChat' }, 'view.togglePanel': { id: 'view.togglePanel' }, 'view.toggleAgentsLayout': { id: 'view.toggleAgentsLayout' },
  'view.zoomIn': { id: 'view.zoomIn' }, 'view.zoomOut': { id: 'view.zoomOut' }, 'view.zoomReset': { id: 'view.zoomReset' }, 'view.devtools': { id: 'app.devtools' },
  'go.back': { id: 'editor.navigateBack' }, 'go.forward': { id: 'editor.navigateForward' }, 'go.symbol': { id: 'editor.goToSymbol' }, 'go.line': { id: 'editor.goToLine' }, 'go.definition': { id: 'editor.goToDefinition' }, 'go.references': { id: 'editor.findReferences' },
  'ai.newChat': { id: 'chat.new' }, 'ai.toggleChat': { id: 'chat.open' }, 'ai.inlineEdit': { id: 'ai.inlineEdit' }, 'ai.addSelectionToChat': { id: 'chat.addSelection' }, 'ai.review': { id: 'ai.review' }, 'ai.commitMessage': { id: 'ai.commitMessage' },
  'ai.toggleTab': { id: 'ai.tab.toggle' }, 'ai.rules': { id: 'settings.rules' }, 'ai.mcp': { id: 'settings.mcp' }, 'ai.indexing': { id: 'settings.indexing' },
  'terminal.new': { id: 'terminal.new' }, 'terminal.split': { id: 'terminal.split' }, 'terminal.kill': { id: 'terminal.kill' }, 'terminal.cmdk': { id: 'ai.terminalCmdK' },
  'window.agents': { id: 'view.toggleAgentsLayout' },
  'help.welcome': { id: 'help.welcome' }, 'help.shortcuts': { id: 'help.shortcuts' }, 'help.docs': { id: 'help.docs' }, 'help.openConfig': { id: 'app.openConfigFolder' }, 'help.about': { id: 'help.about' },
}

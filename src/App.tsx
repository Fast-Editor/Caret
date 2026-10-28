import { useEffect, useState } from 'react'
import { AppLayout } from '@/components/layout/AppLayout'
import { TooltipProvider, ErrorBoundary } from '@/components/common'
import { DialogHost } from '@/components/dialogs'
import { Toasts } from '@/components/notifications'
import { PaletteHost } from '@/components/palette'
import * as editorModule from '@/components/editor'
import * as chatModule from '@/components/chat'
import { registerShellCommands, MENU_MAP } from '@/lib/shellCommands'
import { runCommand, hasCommand } from '@/lib/commands'
import { startKeybindings, setUserOverrides, setFileBindings, setPersistHandler } from '@/lib/keybindings'
import { initTheme } from '@/lib/theme'
import { initCliOpen } from '@/lib/cliOpen'
import { initDebugModule } from '@/components/debug'
import { Events, on, isTauri } from '@/lib/tauri'
import { api } from '@/lib/api'
import { initSettingsStore, useSettings } from '@/store/settings'
import { initWorkspaceStore, useWorkspace } from '@/store/workspace'
import { useUi } from '@/store/ui'
import { useChat } from '@/store/chat'
import { hookConsole } from '@/store/notifications'
import type { WorkspaceInfo } from '@/types'

let booted = false
function boot(): () => void {
  if (booted) return () => {}
  booted = true
  hookConsole()
  // Test/automation hook (used by scripts/e2e.mjs through the dev bridge).
  ;(window as unknown as { __caret?: unknown }).__caret = { runCommand, hasCommand, api, stores: { settings: useSettings, workspace: useWorkspace, ui: useUi, chat: useChat } }
  initSettingsStore()
  initWorkspaceStore()
  initTheme()
  const offs: (() => void)[] = []
  offs.push(registerShellCommands())
  const ed = editorModule as unknown as { initEditorModule?: () => void | (() => void) }
  const ch = chatModule as unknown as { initChatModule?: () => void | (() => void) }
  try { const r = ed.initEditorModule?.(); if (typeof r === 'function') offs.push(r) } catch (e) { console.error('editor module init failed', e) }
  try { const r = ch.initChatModule?.(); if (typeof r === 'function') offs.push(r) } catch (e) { console.error('chat module init failed', e) }
  // `caret <path>` launcher: drain queued opens (argv / Finder / caret:// URLs) and register install commands
  offs.push(initCliOpen())
  try { offs.push(initDebugModule()) } catch (e) { console.error('debug module init failed', e) }
  if (!import.meta.env.DEV) { const t = window.setTimeout(() => { void import('@/lib/updater').then((m) => m.checkForUpdatesSilently()) }, 15_000); offs.push(() => window.clearTimeout(t)) }
  // keybindings: settings overrides + optional ~/.caret/keybindings.json
  setPersistHandler((overrides) => useSettings.getState().update({ keybindings: overrides }))
  const applyOverrides = () => setUserOverrides(useSettings.getState().settings?.keybindings)
  applyOverrides()
  offs.push(useSettings.subscribe((s, p) => { if (s.settings?.keybindings !== p.settings?.keybindings) applyOverrides() }))
  void (async () => { try { const p = await api.settings.path('keybindings'); const f = await api.fs.readFile(p); const parsed = JSON.parse(f.content); if (Array.isArray(parsed)) setFileBindings(parsed) } catch { /* optional */ } })()
  offs.push(startKeybindings())
  // platform
  void import('@tauri-apps/plugin-os').then(({ platform }) => { try { const p = platform(); useUi.setState({ platform: p === 'macos' ? 'macos' : p === 'windows' ? 'windows' : p === 'linux' ? 'linux' : 'other' }) } catch { /* ignore */ } }).catch(() => {})
  // native menu routing
  offs.push(on<string>(Events.menu, (id) => { const m = MENU_MAP[id]; if (m && hasCommand(m.id)) void runCommand(m.id, m.args); else if (m) console.warn(`[menu] command not available: ${m.id}`) }))
  // workspace restore / welcome
  offs.push(on<WorkspaceInfo | null>(Events.workspaceChanged, (ws) => { if (ws && hasCommand('editor.onWorkspaceChanged')) void runCommand('editor.onWorkspaceChanged', ws) }))
  void (async () => {
    const wsState = useWorkspace.getState()
    await wsState.load().catch(() => {})
    const settings = useSettings.getState().settings ?? (await new Promise<ReturnType<typeof useSettings.getState>['settings']>((res) => { const u = useSettings.subscribe((s) => { if (s.loaded) { u(); res(s.settings) } }) }))
    if (!useWorkspace.getState().workspace && settings?.general.openLastWorkspace) {
      const r = useWorkspace.getState().recent.find((x) => x.exists)
      if (r) { try { await useWorkspace.getState().open(r.path) } catch { /* ignore */ } }
    }
    if (!useWorkspace.getState().workspace && hasCommand('editor.openWelcome')) void runCommand('editor.openWelcome')
  })()
  // window focus state (for blur styling)
  const onFocus = () => useUi.setState({ windowFocused: true }); const onBlur = () => useUi.setState({ windowFocused: false })
  window.addEventListener('focus', onFocus); window.addEventListener('blur', onBlur)
  offs.push(() => { window.removeEventListener('focus', onFocus); window.removeEventListener('blur', onBlur) })
  // prevent WebView default drag/drop navigation and browser zoom shortcuts
  const prevent = (e: DragEvent) => { if (!(e.target as HTMLElement | null)?.closest?.('[data-allow-drop]')) e.preventDefault() }
  window.addEventListener('dragover', prevent); window.addEventListener('drop', prevent)
  offs.push(() => { window.removeEventListener('dragover', prevent); window.removeEventListener('drop', prevent) })
  const ctxGuard = (e: MouseEvent) => { if (!(e.target as HTMLElement | null)?.closest?.('input, textarea, .monaco-editor, .xterm, [data-native-menu]')) e.preventDefault() }
  window.addEventListener('contextmenu', ctxGuard)
  offs.push(() => window.removeEventListener('contextmenu', ctxGuard))
  return () => { offs.forEach((o) => o()); booted = false }
}

export default function App() {
  const [ready, setReady] = useState(false)
  useEffect(() => { const off = boot(); setReady(true); return off }, [])
  if (!isTauri()) {
    return <div className="h-full flex items-center justify-center text-muted text-sm">Caret must run inside the Tauri shell (<code className="mono mx-1">npm run dev</code>).</div>
  }
  return (
    <TooltipProvider>
      {ready && <ErrorBoundary name="Workbench"><AppLayout /></ErrorBoundary>}
      <PaletteHost />
      <DialogHost />
      <Toasts />
    </TooltipProvider>
  )
}

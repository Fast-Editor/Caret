/**
 * Closing a window that has unsaved files (docs/MULTI_WINDOW.md, "Behaviour"). Tauri lets the frontend veto a close
 * request; we always veto, ask when something is dirty, and destroy the window ourselves: Save All / Don't Save →
 * the window goes away, Cancel → it stays. `resolveWindowClose` is the pure decision (unit-tested); `initWindowClose`
 * wires it to the current window and the stores.
 */
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isTauri } from './tauri'

export type CloseChoice = 'save' | 'discard' | 'cancel'
export type WindowCloseOutcome = 'closed' | 'kept'

export interface WindowCloseDeps {
  /** Titles of the tabs with unsaved changes, one per file. */
  dirtyTitles: () => string[]
  /** Ask the user; only called when something is dirty. */
  confirm: (titles: string[]) => Promise<CloseChoice>
  /** Save every dirty tab; `false` when a save failed or was cancelled (the window then stays open). */
  saveAll: () => Promise<boolean>
  /** Best-effort cleanup before the webview goes away (kill the window's PTYs); errors are ignored. */
  disposeTerminals?: () => Promise<void> | void
  /** Actually close the window (bypasses the close-requested hook). */
  destroy: () => Promise<void> | void
}

/** No dirty tabs → close. Dirty: Save → save then close (stay if a save fails), Don't Save → close, Cancel → stay. */
export async function resolveWindowClose(deps: WindowCloseDeps): Promise<WindowCloseOutcome> {
  const titles = deps.dirtyTitles()
  if (titles.length) {
    const choice = await deps.confirm(titles)
    if (choice === 'cancel') return 'kept'
    if (choice === 'save' && !(await deps.saveAll())) return 'kept'
  }
  try { await deps.disposeTerminals?.() } catch { /* the window is going away regardless */ }
  await deps.destroy()
  return 'closed'
}

/** Unsaved tabs across all groups, one entry per file (a file open in two groups counts once). */
export function dirtyTabTitles(groups: { tabs: { id: string; title: string; dirty: boolean; path?: string }[] }[]): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const g of groups) {
    for (const t of g.tabs) {
      if (!t.dirty) continue
      const key = t.path ?? t.id
      if (seen.has(key)) continue
      seen.add(key)
      out.push(t.title)
    }
  }
  return out
}

/**
 * Hook the current window's close request (the red button, File → Close Window, `window_close` from another window).
 * No-op outside Tauri (vitest, the remote browser shim). Returns the unsubscribe.
 */
export function initWindowClose(): () => void {
  if (!isTauri()) return () => {}
  let unlisten: (() => void) | null = null
  let disposed = false
  let busy = false
  void getCurrentWindow().onCloseRequested(async (e) => {
    e.preventDefault() // we destroy the window ourselves once the user has decided
    if (busy) return // a second close request while the dialog is up
    busy = true
    try { await resolveWindowClose(await appDeps()) } catch (err) { console.error('[window] close failed', err) } finally { busy = false }
  }).then((u) => { if (disposed) u(); else unlisten = u }).catch((err) => console.error('[window] close hook failed', err))
  return () => { disposed = true; unlisten?.(); unlisten = null }
}

/** Store-backed deps, loaded lazily so this module stays importable in Node (tests). */
async function appDeps(): Promise<WindowCloseDeps> {
  const [{ useEditor }, { useTerminals }, { api }] = await Promise.all([import('@/store/editor'), import('@/store/terminal'), import('@/lib/api')])
  const dirty = () => useEditor.getState().groups.flatMap((g) => g.tabs.filter((t) => t.dirty).map((tab) => ({ groupId: g.id, tab })))
  return {
    dirtyTitles: () => dirtyTabTitles(useEditor.getState().groups),
    // Reuses the tab-close dialog (ConfirmCloseDialog) with every dirty file listed and a Save All button.
    confirm: (titles) => new Promise<CloseChoice>((resolve) => {
      const title = titles.length === 1 ? titles[0] : `${titles.length} files`
      useEditor.setState({ confirmClose: { groupId: '', tabId: '', title, titles, saveLabel: titles.length > 1 ? 'Save All' : 'Save', resolve } })
    }).finally(() => useEditor.setState({ confirmClose: null })),
    saveAll: async () => {
      const st = useEditor.getState()
      await st.saveAll() // every dirty file model; failures notify and leave the tab dirty
      // Untitled buffers each need a Save As dialog; `save()` with no path targets the active tab.
      for (const { groupId, tab } of dirty()) {
        if (tab.kind !== 'untitled') continue
        st.setActive(groupId, tab.id)
        if (!(await useEditor.getState().save())) return false
      }
      return dirty().length === 0
    },
    disposeTerminals: async () => {
      const ids = Object.values(useTerminals.getState().sessions).map((s) => s.ptyId).filter((id): id is string => !!id)
      if (!ids.length) return
      await Promise.race([Promise.allSettled(ids.map((id) => api.pty.kill(id))), new Promise((r) => setTimeout(r, 1000))])
    },
    destroy: () => getCurrentWindow().destroy(),
  }
}

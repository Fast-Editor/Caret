/**
 * Per-window helpers (docs/MULTI_WINDOW.md): the label of the window this bundle runs in, editor-session keys that
 * keep two windows on one folder apart, and the Open Folder routing rule.
 */
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isTauri } from './tauri'

/** Label of the current window (`main`, `w2`, …); `main` outside Tauri (tests, the remote browser shim). */
export function currentWindowLabel(): string {
  if (!isTauri()) return 'main'
  try { return getCurrentWindow().label || 'main' } catch { return 'main' }
}

const SESSION_PREFIX = 'caret.editor.session.'
/** Per-window editor session key: two windows on the same folder keep separate tab sets. */
export function sessionKey(root: string, label: string): string { return `${SESSION_PREFIX}${root}::${label}` }
/** The key written before multiple windows existed (one tab set per folder); still read as a fallback. */
export function legacySessionKey(root: string): string { return `${SESSION_PREFIX}${root}` }
/** The saved session for `root` in window `label`, falling back to the pre-multi-window entry; `null` when neither parses. */
export function readSession<T>(storage: Pick<Storage, 'getItem'>, root: string, label: string): T | null {
  for (const key of [sessionKey(root, label), legacySessionKey(root)]) {
    try {
      const raw = storage.getItem(key)
      if (raw) { const v = JSON.parse(raw) as T | null; if (v != null) return v }
    } catch { /* corrupt entry: try the next key */ }
  }
  return null
}

export type OpenFoldersIn = 'new' | 'current'
/**
 * Where a folder opened from this window goes: in place when the window is empty; otherwise a new window unless
 * `window.openFoldersIn` is `"current"`. `invert` flips the setting (the Alt gesture).
 */
export function opensInNewWindow(hasWorkspace: boolean, setting: OpenFoldersIn | undefined, invert = false): boolean {
  if (!hasWorkspace) return false
  const wantsNew = (setting ?? 'new') !== 'current'
  return invert ? !wantsNew : wantsNew
}

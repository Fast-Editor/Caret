/**
 * Per-window helpers (docs/MULTI_WINDOW.md): the label of the window this bundle runs in, editor-session keys that
 * keep two windows on one folder apart, and the Open Folder routing rule.
 */
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isTauri } from './tauri'
import type { WindowSummary } from '@/types'

/** Label of the current window (`main`, `w2`, …); `main` outside Tauri (tests, the remote browser shim). */
export function currentWindowLabel(): string {
  if (!isTauri()) return 'main'
  try { return getCurrentWindow().label || 'main' } catch { return 'main' }
}

const SESSION_PREFIX = 'caret.editor.session.'
/** Per-window editor session key: two windows on the same folder keep separate tab sets. */
/** The first window of a cold start: the only one that restores the last folder and drains argv opens. */
export function isMainWindow(): boolean { return currentWindowLabel() === 'main' }

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

/** `<folder> — Caret`, or just `Caret` for an empty window; mirrors `window_title` in core/windows.rs. */
export function windowTitle(folderName: string | null | undefined): string {
  return folderName ? `${folderName} — Caret` : 'Caret'
}

/** `/Users/<me>/x` → `~/x` for display. */
export function displayPath(path: string): string { return path.replace(/^\/Users\/[^/]+/, '~') }

export interface WindowPickItem { id: string; label: string; description: string; detail: string; keywords: string }
/**
 * Rows for "Window: Switch Window…": folder name (or "Empty window"), the folder path, and the label with a "current"
 * marker. The current window goes last so Enter switches to another window straight away; the others keep creation
 * order (same as the native Window menu).
 */
export function windowPickItems(windows: WindowSummary[], currentLabel: string): WindowPickItem[] {
  const row = (w: WindowSummary): WindowPickItem => {
    const current = w.label === currentLabel || w.focused
    return {
      id: w.label,
      label: w.name ?? 'Empty window',
      description: w.root ? displayPath(w.root) : 'No folder open',
      detail: current ? `${w.label} · current` : w.label,
      keywords: [w.label, w.root ?? ''].join(' ').trim(),
    }
  }
  const others = windows.filter((w) => w.label !== currentLabel)
  const current = windows.filter((w) => w.label === currentLabel)
  return [...others, ...current].map(row)
}

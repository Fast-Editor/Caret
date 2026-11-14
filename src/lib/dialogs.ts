/**
 * Native pickers that degrade instead of crashing. The folder picker goes through `dialog_pick_folder` (nil-safe on
 * macOS); when the system panel is unavailable the user can type a path instead.
 */
import { api } from '@/lib/api'
import { isCaretError } from '@/lib/tauri'

export const DIALOG_UNAVAILABLE = 'dialog_unavailable'

export function isDialogUnavailable(e: unknown): boolean { return isCaretError(e) && e.code === DIALOG_UNAVAILABLE }

export interface PickFolderOptions { title?: string; defaultPath?: string }

/** Pick a folder; `null` when cancelled. Falls back to a typed path when the system dialog cannot be shown. */
export async function pickFolder(opts: PickFolderOptions = {}): Promise<string | null> {
  try {
    return await api.dialog.pickFolder(opts)
  } catch (e) {
    if (!isDialogUnavailable(e)) throw e
    const { prompt } = await import('@/components/dialogs/store')
    const typed = await prompt({
      title: opts.title ?? 'Open Folder',
      message: 'The system file dialog is not available right now. Enter the folder path instead:',
      placeholder: '/path/to/project', defaultValue: opts.defaultPath ?? '', okLabel: 'Open',
      validate: (v) => (v.trim().startsWith('/') || v.trim().startsWith('~') ? null : 'Enter an absolute path'),
    })
    const path = typed?.trim()
    if (!path) return null
    const st = await api.fs.stat(path).catch(() => null)
    if (!st?.exists || st.kind !== 'dir') { const { notify } = await import('@/store/notifications'); notify('error', 'Not a folder', `${path} does not exist or is not a folder.`); return null }
    return st.path ?? path
  }
}

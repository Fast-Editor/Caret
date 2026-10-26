import { useCallback } from 'react'
import { useSettings } from '@/store/settings'
import { useWorkspace } from '@/store/workspace'
import { getPath, isModified, patchFor, defaultFor, type SettingField } from '@/lib/settingsSchema'
import { create } from 'zustand'
import { notify, errorMessage } from './primitives'

export type SettingsScope = 'user' | 'workspace'
interface ScopeStore { scope: SettingsScope; setScope: (s: SettingsScope) => void }
export const useSettingsScope = create<ScopeStore>((set) => ({ scope: 'user', setScope: (scope) => set({ scope }) }))

/** Bind a settings path to the store, honouring the active scope (user/workspace). */
export function useField<T = unknown>(path: string | SettingField) {
  const id = typeof path === 'string' ? path : path.id
  const value = useSettings((s) => getPath(s.settings, id)) as T | undefined
  const settings = useSettings((s) => s.settings)
  const update = useSettings((s) => s.update)
  const scope = useSettingsScope((s) => s.scope)
  const hasWorkspace = useWorkspace((s) => !!s.workspace)
  const effectiveScope: SettingsScope = scope === 'workspace' && hasWorkspace ? 'workspace' : 'user'
  const set = useCallback(async (v: T | undefined) => {
    try { await update(patchFor(id, v), effectiveScope) } catch (e) { notify('error', 'Failed to save setting', errorMessage(e)) }
  }, [id, update, effectiveScope])
  const reset = useCallback(() => set(defaultFor(id) as T | undefined), [id, set])
  return { value, set, reset, modified: isModified(settings, id), scope: effectiveScope }
}

import { create } from 'zustand'
import type { Settings, SettingsPatch } from '@/types'
import { api } from '@/lib/api'
import { Events, on } from '@/lib/tauri'

interface SettingsStore {
  settings: Settings | null
  loaded: boolean
  userPath: string
  workspacePath: string | null
  load: () => Promise<void>
  update: (patch: SettingsPatch, scope?: 'user' | 'workspace') => Promise<void>
}

export const useSettings = create<SettingsStore>((set) => ({
  settings: null,
  loaded: false,
  userPath: '',
  workspacePath: null,
  load: async () => {
    const s = await api.settings.get()
    set({ settings: s.effective, loaded: true, userPath: s.userPath, workspacePath: s.workspacePath })
  },
  update: async (patch, scope = 'user') => {
    const eff = await api.settings.update(patch, scope)
    set({ settings: eff })
  },
}))

let subscribed = false
export function initSettingsStore(): void {
  if (subscribed) return
  subscribed = true
  useSettings.getState().load().catch((e) => console.error('settings load failed', e))
  on<Settings>(Events.settingsChanged, (s) => useSettings.setState({ settings: s }))
}

/** Convenience selector with defaults so components never deal with null settings. */
export function useSetting<K extends keyof Settings>(key: K): Settings[K] | undefined {
  return useSettings((s) => s.settings?.[key])
}

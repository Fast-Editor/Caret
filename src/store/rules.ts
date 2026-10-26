import { create } from 'zustand'
import type { RuleInfo, MemoryItem, SkillInfo, SlashCommandInfo, SubagentInfo, HookInfo, PlanInfo } from '@/types'
import { api } from '@/lib/api'
import { Events, on } from '@/lib/tauri'

interface RulesStore {
  rules: RuleInfo[]
  memories: MemoryItem[]
  skills: SkillInfo[]
  commands: SlashCommandInfo[]
  subagents: SubagentInfo[]
  hooks: HookInfo[]
  plans: PlanInfo[]
  loaded: boolean
  loadRules: () => Promise<void>
  loadMemories: () => Promise<void>
  loadAutomation: () => Promise<void>
  loadAll: () => Promise<void>
}

const safe = async <T,>(p: Promise<T>, fallback: T): Promise<T> => { try { return await p } catch { return fallback } }

export const useRules = create<RulesStore>((set, get) => ({
  rules: [], memories: [], skills: [], commands: [], subagents: [], hooks: [], plans: [], loaded: false,
  loadRules: async () => set({ rules: await safe(api.rules.list(), []) }),
  loadMemories: async () => set({ memories: await safe(api.memories.list(), []) }),
  loadAutomation: async () => {
    const [skills, commands, subagents, hooks, plans] = await Promise.all([
      safe(api.skills.list(), []), safe(api.commands.list(), []), safe(api.subagents.list(), []), safe(api.hooks.list(), []), safe(api.plans.list(), []),
    ])
    set({ skills, commands, subagents, hooks, plans })
  },
  loadAll: async () => { const s = get(); await Promise.all([s.loadRules(), s.loadMemories(), s.loadAutomation()]); set({ loaded: true }) },
}))

let subscribed = false
export function initRulesStore(): void {
  if (subscribed) return
  subscribed = true
  useRules.getState().loadAll().catch(() => {})
  on(Events.rulesChanged, () => useRules.getState().loadAll().catch(() => {}))
  on(Events.workspaceChanged, () => useRules.getState().loadAll().catch(() => {}))
}

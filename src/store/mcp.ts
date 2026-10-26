import { create } from 'zustand'
import type { McpServerStatus, McpServerConfig, McpToolInfo, McpResourceInfo, McpPromptInfo } from '@/types'
import { api } from '@/lib/api'
import { Events, on } from '@/lib/tauri'

interface McpStore {
  servers: McpServerStatus[]
  configs: McpServerConfig[]
  tools: McpToolInfo[]
  resources: McpResourceInfo[]
  prompts: McpPromptInfo[]
  loading: boolean
  loaded: boolean
  error: string | null
  load: () => Promise<void>
  toggleServer: (id: string, enabled: boolean) => Promise<void>
  toggleTool: (serverId: string, tool: string, enabled: boolean) => Promise<void>
  restart: (id: string) => Promise<void>
  remove: (id: string) => Promise<void>
  add: (config: McpServerConfig, scope: 'project' | 'user') => Promise<void>
  update: (config: McpServerConfig) => Promise<void>
}

export const useMcp = create<McpStore>((set, get) => ({
  servers: [], configs: [], tools: [], resources: [], prompts: [], loading: false, loaded: false, error: null,
  load: async () => {
    set({ loading: true })
    try {
      const [servers, configs, tools, resources, prompts] = await Promise.all([
        api.mcp.servers(), api.mcp.configs(), api.mcp.tools(), api.mcp.resources().catch(() => [] as McpResourceInfo[]), api.mcp.prompts().catch(() => [] as McpPromptInfo[]),
      ])
      set({ servers, configs, tools, resources, prompts, loaded: true, error: null })
    } catch (e) { set({ error: String((e as Error).message ?? e), loaded: true }) } finally { set({ loading: false }) }
  },
  toggleServer: async (id, enabled) => { await api.mcp.toggle(id, enabled); await get().load() },
  toggleTool: async (serverId, tool, enabled) => { await api.mcp.toggleTool(serverId, tool, enabled); await get().load() },
  restart: async (id) => { await api.mcp.restart(id); await get().load() },
  remove: async (id) => { await api.mcp.remove(id); await get().load() },
  add: async (config, scope) => { await api.mcp.add(config, scope); await get().load() },
  update: async (config) => { await api.mcp.update(config); await get().load() },
}))

let subscribed = false
export function initMcpStore(): void {
  if (subscribed) return
  subscribed = true
  useMcp.getState().load().catch(() => {})
  on(Events.mcpChanged, () => useMcp.getState().load().catch(() => {}))
  on(Events.workspaceChanged, () => useMcp.getState().load().catch(() => {}))
}

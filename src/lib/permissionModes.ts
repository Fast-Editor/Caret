/**
 * Permission modes (mirror of `policy::PermissionMode` in src-tauri): what the agent may do without asking.
 * Pure helpers shared by the composer picker, the approval card and settings.
 */
import type { AgentSettings, AutoRunMode, PermissionMode } from '@/types'

export const PERMISSION_MODE_ORDER: PermissionMode[] = ['supervised', 'auto-accept-edits', 'auto', 'full-access']

export interface PermissionModeInfo { id: PermissionMode; label: string; short: string; description: string }

export const PERMISSION_MODES: PermissionModeInfo[] = [
  { id: 'supervised', label: 'Supervised', short: 'Supervised', description: 'Every command, file edit and MCP tool asks first. Session allows are remembered.' },
  { id: 'auto-accept-edits', label: 'Auto-accept edits', short: 'Edits', description: 'Edits inside the workspace apply automatically; only allowlisted, non-destructive commands run on their own.' },
  { id: 'auto', label: 'Auto', short: 'Auto', description: 'Edits and safe or cautious commands run; destructive commands ask, blocked ones never run.' },
  { id: 'full-access', label: 'Full access', short: 'Full', description: 'Everything runs without asking, destructive commands included. Blocked and denylisted commands stay denied; the sandbox still applies.' },
]

export function permissionModeInfo(mode: PermissionMode): PermissionModeInfo { return PERMISSION_MODES.find((m) => m.id === mode) ?? PERMISSION_MODES[0] }
export function permissionModeLabel(mode: PermissionMode): string { return permissionModeInfo(mode).label }
export function modeRank(mode: PermissionMode): number { const i = PERMISSION_MODE_ORDER.indexOf(mode); return i < 0 ? 0 : i }

/** Legacy `agent.autoRun` to a mode (off = supervised, allowlist = auto-accept edits, all = auto). */
export function legacyToPermissionMode(autoRun: AutoRunMode | string | undefined): PermissionMode {
  if (autoRun === 'off') return 'supervised'
  if (autoRun === 'all') return 'auto'
  return 'auto-accept-edits'
}

export function clampPermissionMode(requested: PermissionMode, cap: PermissionMode | undefined | null): PermissionMode {
  if (!cap) return requested
  return modeRank(requested) <= modeRank(cap) ? requested : cap
}

type AgentModeSettings = Pick<AgentSettings, 'permissionMode' | 'maxPermissionMode' | 'autoRun'>

/** The mode new threads get from settings (before the cap). */
export function settingsPermissionMode(agent: Partial<AgentModeSettings> | undefined | null): PermissionMode {
  return agent?.permissionMode ?? legacyToPermissionMode(agent?.autoRun)
}

/** Thread override, else the settings default, clamped by `maxPermissionMode` (same rule as the backend). */
export function effectivePermissionMode(agent: Partial<AgentModeSettings> | undefined | null, override?: PermissionMode | null): PermissionMode {
  return clampPermissionMode(override ?? settingsPermissionMode(agent), agent?.maxPermissionMode)
}

/** Whether a mode can be picked under the cap. */
export function isWithinCap(mode: PermissionMode, cap: PermissionMode | undefined | null): boolean { return !cap || modeRank(mode) <= modeRank(cap) }

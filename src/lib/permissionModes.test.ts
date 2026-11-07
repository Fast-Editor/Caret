import { describe, it, expect } from 'vitest'
import { PERMISSION_MODES, PERMISSION_MODE_ORDER, clampPermissionMode, effectivePermissionMode, isWithinCap, legacyToPermissionMode, modeRank, permissionModeLabel, settingsPermissionMode } from './permissionModes'

describe('permission modes', () => {
  it('lists the four modes in cap order', () => {
    expect(PERMISSION_MODES.map((m) => m.id)).toEqual(PERMISSION_MODE_ORDER)
    expect(modeRank('supervised')).toBeLessThan(modeRank('auto-accept-edits'))
    expect(modeRank('auto')).toBeLessThan(modeRank('full-access'))
    expect(permissionModeLabel('auto-accept-edits')).toBe('Auto-accept edits')
  })
  it('derives the legacy auto-run setting', () => {
    expect(legacyToPermissionMode('off')).toBe('supervised')
    expect(legacyToPermissionMode('allowlist')).toBe('auto-accept-edits')
    expect(legacyToPermissionMode('all')).toBe('auto')
    expect(legacyToPermissionMode(undefined)).toBe('auto-accept-edits')
  })
  it('clamps by the cap', () => {
    expect(clampPermissionMode('full-access', 'auto')).toBe('auto')
    expect(clampPermissionMode('supervised', 'auto')).toBe('supervised')
    expect(clampPermissionMode('full-access', undefined)).toBe('full-access')
    expect(isWithinCap('full-access', 'auto')).toBe(false)
    expect(isWithinCap('auto', 'auto')).toBe(true)
    expect(isWithinCap('full-access', null)).toBe(true)
  })
  it('resolves the effective mode like the backend', () => {
    expect(settingsPermissionMode({ autoRun: 'off' })).toBe('supervised')
    expect(settingsPermissionMode({ autoRun: 'off', permissionMode: 'auto' })).toBe('auto')
    expect(effectivePermissionMode({ autoRun: 'allowlist' }, null)).toBe('auto-accept-edits')
    expect(effectivePermissionMode({ autoRun: 'allowlist' }, 'full-access')).toBe('full-access')
    expect(effectivePermissionMode({ autoRun: 'allowlist', maxPermissionMode: 'auto' }, 'full-access')).toBe('auto')
    expect(effectivePermissionMode({ permissionMode: 'full-access', maxPermissionMode: 'supervised' })).toBe('supervised')
    expect(effectivePermissionMode(undefined)).toBe('auto-accept-edits')
  })
})

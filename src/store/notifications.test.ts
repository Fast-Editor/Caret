import { describe, it, expect, vi } from 'vitest'
import { leaveDelayMs, useNotifications } from './notifications'

describe('leaveDelayMs', () => {
  it('is 0 with no document (SSR/tests), motion off, or reduce-motion', () => {
    expect(leaveDelayMs(undefined)).toBe(0)
    expect(leaveDelayMs({ dataset: { motion: 'off' } })).toBe(0)
    expect(leaveDelayMs({ dataset: { motion: 'subtle' }, className: 'dark reduce-motion' })).toBe(0)
  })
  it('scales with the motion level', () => {
    expect(leaveDelayMs({ dataset: { motion: 'subtle' } })).toBe(136)
    expect(leaveDelayMs({ dataset: { motion: 'expressive' } })).toBeGreaterThan(leaveDelayMs({ dataset: { motion: 'subtle' } }))
    expect(leaveDelayMs({ dataset: {} })).toBe(136)
  })
})

describe('toast dismiss', () => {
  it('removes immediately when there is no motion (node env has no document)', () => {
    vi.useFakeTimers()
    const id = useNotifications.getState().add({ kind: 'info', title: 'hi', sticky: true } as never)
    expect(useNotifications.getState().toasts.some((t) => t.id === id)).toBe(true)
    useNotifications.getState().dismiss(id)
    expect(useNotifications.getState().toasts.some((t) => t.id === id)).toBe(false)
    useNotifications.getState().dismissAll()
    expect(useNotifications.getState().toasts).toEqual([])
    vi.useRealTimers()
  })
})

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

describe('timed action toasts (thread archive / settle undo)', () => {
  it('auto-dismisses a toast with actions when an explicit timeout is given, keeps action-only toasts', () => {
    vi.useFakeTimers()
    vi.stubGlobal('window', globalThis) // the store schedules with window.setTimeout
    const onAction = vi.fn()
    const store = useNotifications.getState()
    const id = store.add({ level: 'info', title: 'Thread archived', actions: [{ id: 'undo', label: 'Undo' }], timeoutMs: 5000, onAction })
    expect(useNotifications.getState().toasts.some((t) => t.id === id)).toBe(true)
    expect(useNotifications.getState().actionHandlers.get(id)).toBe(onAction)
    vi.advanceTimersByTime(5001)
    expect(useNotifications.getState().toasts.some((t) => t.id === id)).toBe(false)
    expect(useNotifications.getState().actionHandlers.has(id)).toBe(false)
    const sticky = store.add({ level: 'info', title: 'Needs a click', actions: [{ id: 'ok', label: 'OK' }] })
    vi.advanceTimersByTime(60_000)
    expect(useNotifications.getState().toasts.some((t) => t.id === sticky)).toBe(true)
    useNotifications.getState().dismiss(sticky)
    vi.unstubAllGlobals()
    vi.useRealTimers()
  })
})

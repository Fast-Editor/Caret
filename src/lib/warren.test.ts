import { describe, expect, it } from 'vitest'
import type { SandboxBinding, WarrenInstance } from '@/types'
import {
  availableActions, bindingStatusLine, executorLabel, formatMicros, isTransitional, parseCommaList, pickerLabel, relativeTime,
  shapeLabel, sortInstances, statusTone,
} from './warren'

const inst = (id: string, status: WarrenInstance['status'], createdAt = 0): WarrenInstance => ({
  id, name: id, template: 'warren-base', status, resources: { cpu: 2, memory: 4, disk: 4 }, createdAt,
})

describe('formatMicros', () => {
  it('renders micro-dollars with two decimals', () => {
    expect(formatMicros(5_000_000)).toBe('$5.00')
    expect(formatMicros(4_760_000)).toBe('$4.76')
    expect(formatMicros(0)).toBe('$0.00')
    expect(formatMicros(undefined)).toBe('$0.00')
  })
  it('groups thousands and keeps the sign in front of the dollar', () => {
    expect(formatMicros(1_234_500_000)).toBe('$1,234.50')
    expect(formatMicros(-1_000_000)).toBe('-$1.00')
  })
})

describe('shapeLabel', () => {
  it('formats cpu, memory and disk with middle dots', () => {
    expect(shapeLabel({ cpu: 2, memory: 4, disk: 4 })).toBe('2 vCPU · 4 GB · 4 GB')
    expect(shapeLabel({ cpu: 8, memory: 16, disk: 20 })).toBe('8 vCPU · 16 GB · 20 GB')
    expect(shapeLabel(undefined)).toBe('—')
  })
})

describe('statusTone / isTransitional', () => {
  it('maps every Warren status to a tone', () => {
    expect(statusTone('running')).toBe('success')
    for (const s of ['provisioning', 'starting', 'restarting', 'updating', 'stopping', 'waking', 'deleting'] as const) expect(statusTone(s), s).toBe('warning')
    expect(statusTone('failed')).toBe('danger')
    expect(statusTone('sleeping')).toBe('muted')
    expect(statusTone('stopped')).toBe('muted')
    expect(statusTone('deleted')).toBe('muted')
    expect(statusTone('something-new')).toBe('muted')
    expect(statusTone(undefined)).toBe('muted')
  })
  it('knows which states are transitions', () => {
    expect(isTransitional('waking')).toBe(true)
    expect(isTransitional('running')).toBe(false)
    expect(isTransitional(undefined)).toBe(false)
  })
})

describe('sortInstances', () => {
  it('orders running, waking/starting, sleeping, stopped, then the rest', () => {
    const list = [inst('f', 'failed'), inst('st', 'stopped'), inst('sl', 'sleeping'), inst('w', 'waking'), inst('r', 'running'), inst('p', 'provisioning'), inst('s', 'starting')]
    expect(sortInstances(list).map((i) => i.id)).toEqual(['r', 's', 'w', 'sl', 'st', 'f', 'p'])
  })
  it('puts the newest first inside a group and does not mutate the input', () => {
    const list = [inst('old', 'running', 100), inst('new', 'running', 300), inst('mid', 'running', 200)]
    const sorted = sortInstances(list)
    expect(sorted.map((i) => i.id)).toEqual(['new', 'mid', 'old'])
    expect(list.map((i) => i.id)).toEqual(['old', 'new', 'mid'])
  })
  it('falls back to the id when status and age tie', () => {
    expect(sortInstances([inst('b', 'stopped'), inst('a', 'stopped')]).map((i) => i.id)).toEqual(['a', 'b'])
  })
})

describe('pickerLabel', () => {
  const binding: SandboxBinding = { provider: 'warren', instanceId: 'ab12cd34ef', instanceName: 'caret', remoteRoot: '/home/agent/work', pushedFiles: 0, status: 'ready' }
  it('says Local without a binding and names the instance with one', () => {
    expect(pickerLabel(null)).toBe('Local')
    expect(pickerLabel(undefined)).toBe('Local')
    expect(pickerLabel(binding)).toBe('Warren · ab12cd34ef')
  })
})

describe('relativeTime', () => {
  const now = 1_763_000_000_000 // ms
  it('accepts unix seconds and milliseconds', () => {
    expect(relativeTime(1_763_000_000 - 120, now)).toBe('2m ago')
    expect(relativeTime(now - 120_000, now)).toBe('2m ago')
  })
  it('scales from seconds to years', () => {
    expect(relativeTime(1_763_000_000 - 10, now)).toBe('just now')
    expect(relativeTime(1_763_000_000 - 3 * 3600, now)).toBe('3h ago')
    expect(relativeTime(1_763_000_000 - 5 * 86_400, now)).toBe('5d ago')
    expect(relativeTime(1_763_000_000 - 70 * 86_400, now)).toBe('2mo ago')
    expect(relativeTime(1_763_000_000 - 800 * 86_400, now)).toBe('2y ago')
  })
  it('treats missing or zero timestamps as never and the future as in …', () => {
    expect(relativeTime(undefined, now)).toBe('never')
    expect(relativeTime(0, now)).toBe('never')
    expect(relativeTime(1_763_000_000 + 600, now)).toBe('in 10m')
    expect(relativeTime(1_763_000_000 + 5, now)).toBe('in a moment')
  })
})

describe('availableActions', () => {
  it('offers start for asleep, stopped and failed instances', () => {
    expect(availableActions('stopped')).toEqual(['start', 'delete'])
    expect(availableActions('sleeping')).toEqual(['start', 'stop', 'delete'])
    expect(availableActions('failed')).toEqual(['start', 'delete'])
  })
  it('offers stop, sleep and restart while running', () => {
    expect(availableActions('running')).toEqual(['stop', 'sleep', 'restart', 'delete'])
  })
  it('only offers delete during transitions and nothing once deleting', () => {
    expect(availableActions('provisioning')).toEqual(['delete'])
    expect(availableActions('deleting')).toEqual([])
    expect(availableActions('deleted')).toEqual([])
  })
})

describe('parseCommaList', () => {
  it('splits, trims and dedupes', () => {
    expect(parseCommaList('node_modules, target ,dist,,node_modules')).toEqual(['node_modules', 'target', 'dist'])
    expect(parseCommaList('')).toEqual([])
    expect(parseCommaList(' , , ')).toEqual([])
  })
})

describe('executorLabel', () => {
  it('normalises the Rust executor id into "warren <id>"', () => {
    expect(executorLabel('warren:ab12cd34ef')).toBe('warren ab12cd34ef')
    expect(executorLabel('warren ab12cd34ef')).toBe('warren ab12cd34ef')
    expect(executorLabel('warren/ab12cd34ef')).toBe('warren ab12cd34ef')
    expect(executorLabel('Warren: ab12cd34ef')).toBe('warren ab12cd34ef')
  })
  it('shows nothing for the local executor or garbage', () => {
    expect(executorLabel('local')).toBeNull()
    expect(executorLabel('')).toBeNull()
    expect(executorLabel(undefined)).toBeNull()
    expect(executorLabel('!!!')).toBeNull()
  })
  it('keeps a bare provider name', () => { expect(executorLabel('docker')).toBe('docker') })
})

describe('bindingStatusLine', () => {
  const now = 1_763_000_000_000
  const base: SandboxBinding = { provider: 'warren', instanceId: 'ab12cd34ef', instanceName: 'caret', remoteRoot: '/home/agent/work', pushedFiles: 12, status: 'ready', lastPushAt: 1_763_000_000 - 120 }
  it('joins status and both sync times', () => {
    expect(bindingStatusLine(base, now)).toBe('ready · pushed 2m ago · pulled never')
    expect(bindingStatusLine({ ...base, status: 'syncing', lastPullAt: 1_763_000_000 - 3600 }, now)).toBe('syncing · pushed 2m ago · pulled 1h ago')
  })
  it('surfaces the error message', () => {
    expect(bindingStatusLine({ ...base, status: 'error', error: 'instance stopped' }, now)).toBe('error: instance stopped · pushed 2m ago · pulled never')
    expect(bindingStatusLine({ ...base, status: 'error' }, now)).toMatch(/^error · /)
  })
})

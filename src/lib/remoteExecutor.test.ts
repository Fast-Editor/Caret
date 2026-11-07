import { describe, it, expect, vi } from 'vitest'
import { replaceChannelPlaceholders, createExecutorCore, serializeError, BLOCKED_COMMANDS, type OutFrame, type TauriEventLike } from './remoteExecutor'

describe('replaceChannelPlaceholders', () => {
  it('replaces placeholder strings anywhere in the argument tree and leaves the rest alone', () => {
    const make = (id: number) => ({ channel: id })
    const input = { path: '/x', onEvent: '__CHANNEL__:42', nested: { list: ['a', '__CHANNEL__:7', 3, null], keep: '__CHANNEL__:x' }, n: 1, b: false, u: undefined }
    const out = replaceChannelPlaceholders(input, make) as Record<string, unknown>
    expect(out.path).toBe('/x')
    expect(out.onEvent).toEqual({ channel: 42 })
    expect((out.nested as { list: unknown[] }).list).toEqual(['a', { channel: 7 }, 3, null])
    expect((out.nested as { keep: string }).keep).toBe('__CHANNEL__:x')
    expect(out.n).toBe(1); expect(out.b).toBe(false); expect(out.u).toBeUndefined()
    // input is not mutated
    expect(input.onEvent).toBe('__CHANNEL__:42')
    expect(replaceChannelPlaceholders('__CHANNEL__:1', make)).toEqual({ channel: 1 })
    expect(replaceChannelPlaceholders(5, make)).toBe(5)
    expect(replaceChannelPlaceholders(null, make)).toBeNull()
  })
})

describe('serializeError', () => {
  it('keeps CaretError shape, strings and Error messages', () => {
    expect(serializeError({ code: 'io', message: 'nope', extra: 1 })).toEqual({ code: 'io', message: 'nope' })
    expect(serializeError('plain')).toBe('plain')
    expect(serializeError(new Error('boom'))).toBe('boom')
    expect(serializeError(42)).toBe('42')
  })
})

function harness(overrides: Partial<Parameters<typeof createExecutorCore>[0]> = {}) {
  const sent: OutFrame[] = []
  const unlisten = vi.fn()
  const handlers = new Map<string, (ev: TauriEventLike) => void>()
  const channels: { onmessage: (m: unknown) => void; onEnd: () => void }[] = []
  const deps = {
    invoke: vi.fn(async (cmd: string, args: Record<string, unknown>) => ({ cmd, args })),
    listen: vi.fn(async (event: string, handler: (ev: TauriEventLike) => void) => { handlers.set(event, handler); return unlisten }),
    makeChannel: vi.fn((onmessage: (m: unknown) => void, onEnd: () => void) => { const ch = { onmessage, onEnd, kind: 'fake-channel' }; channels.push(ch); return ch }),
    send: (f: OutFrame) => { sent.push(f) },
    ...overrides,
  }
  return { core: createExecutorCore(deps), sent, deps, unlisten, handlers, channels }
}

describe('executor core', () => {
  it('invokes commands and replies with the value', async () => {
    const { core, sent, deps } = harness()
    await core.handle({ t: 'invoke', client: 3, id: 'r1', cmd: 'fs_stat', args: { path: '/a' } })
    expect(deps.invoke).toHaveBeenCalledWith('fs_stat', { path: '/a' })
    expect(sent).toEqual([{ t: 'reply', client: 3, id: 'r1', ok: true, value: { cmd: 'fs_stat', args: { path: '/a' } } }])
  })

  it('maps undefined results to null and errors to error replies', async () => {
    const { core, sent } = harness({ invoke: vi.fn(async (cmd: string) => { if (cmd === 'boom') throw { code: 'denied', message: 'no' }; return undefined }) })
    await core.handle({ t: 'invoke', client: 1, id: 1, cmd: 'void_cmd' })
    await core.handle({ t: 'invoke', client: 1, id: 2, cmd: 'boom', args: null })
    expect(sent[0]).toEqual({ t: 'reply', client: 1, id: 1, ok: true, value: null })
    expect(sent[1]).toEqual({ t: 'reply', client: 1, id: 2, ok: false, error: { code: 'denied', message: 'no' } })
  })

  it('refuses commands that could escalate a remote session', async () => {
    const { core, sent, deps } = harness()
    for (const cmd of BLOCKED_COMMANDS) await core.handle({ t: 'invoke', client: 1, id: cmd, cmd })
    expect(deps.invoke).not.toHaveBeenCalled()
    expect(sent).toHaveLength(BLOCKED_COMMANDS.size)
    for (const f of sent) { const r = f as { ok: boolean; error: { code: string } }; expect(r.ok).toBe(false); expect(r.error.code).toBe('denied') }
    await core.handle({ t: 'invoke', client: 1, id: 9, cmd: '' })
    expect((sent.at(-1) as { error: { message: string } }).error.message).toBe('missing cmd')
  })

  it('turns channel placeholders into channels that stream cb frames with ordered indices and an end marker', async () => {
    const { core, sent, deps, channels } = harness()
    await core.handle({ t: 'invoke', client: 5, id: 'c', cmd: 'search_text', args: { query: { q: 'x' }, onEvent: '__CHANNEL__:900' } })
    const passed = vi.mocked(deps.invoke).mock.calls[0][1] as { onEvent: { kind: string } }
    expect(passed.onEvent.kind).toBe('fake-channel')
    expect(sent[0]).toMatchObject({ t: 'reply', id: 'c', ok: true })
    channels[0].onmessage({ hit: 1 })
    channels[0].onmessage({ hit: 2 })
    channels[0].onEnd()
    expect(sent.slice(1)).toEqual([
      { t: 'cb', client: 5, callback: 900, payload: { index: 0, message: { hit: 1 } } },
      { t: 'cb', client: 5, callback: 900, payload: { index: 1, message: { hit: 2 } } },
      { t: 'cb', client: 5, callback: 900, payload: { index: 2, end: true } },
    ])
  })

  it('implements plugin:event listen/unlisten per client and disposes on client_gone', async () => {
    const { core, sent, deps, unlisten, handlers } = harness()
    await core.handle({ t: 'invoke', client: 7, id: 'l1', cmd: 'plugin:event|listen', args: { event: 'caret://fs/changed', target: { kind: 'Any' }, handler: 123 } })
    expect(deps.listen).toHaveBeenCalledWith('caret://fs/changed', expect.any(Function))
    expect(deps.invoke).not.toHaveBeenCalled()
    const eventId = (sent[0] as { value: number }).value
    expect(typeof eventId).toBe('number')
    expect(core.listenerCount()).toBe(1)
    // an event arrives: delivered to the client's callback with Tauri's { event, id, payload } shape
    handlers.get('caret://fs/changed')!({ event: 'caret://fs/changed', id: 999, payload: { paths: ['/a'] } })
    expect(sent[1]).toEqual({ t: 'cb', client: 7, callback: 123, payload: { event: 'caret://fs/changed', id: eventId, payload: { paths: ['/a'] } } })
    // unlisten by the returned id
    await core.handle({ t: 'invoke', client: 7, id: 'u1', cmd: 'plugin:event|unlisten', args: { event: 'caret://fs/changed', eventId } })
    expect(unlisten).toHaveBeenCalledTimes(1)
    expect(sent[2]).toEqual({ t: 'reply', client: 7, id: 'u1', ok: true, value: null })
    expect(core.listenerCount()).toBe(0)
    // listeners of a client that disappears are released; other clients keep theirs
    await core.handle({ t: 'invoke', client: 7, id: 'l2', cmd: 'plugin:event|listen', args: { event: 'a', handler: 1 } })
    await core.handle({ t: 'invoke', client: 8, id: 'l3', cmd: 'plugin:event|listen', args: { event: 'b', handler: 2 } })
    expect(core.listenerCount()).toBe(2)
    await core.handle({ t: 'client_gone', client: 7 })
    expect(unlisten).toHaveBeenCalledTimes(2)
    expect(core.listenerCount()).toBe(1)
    core.disposeAll()
    expect(unlisten).toHaveBeenCalledTimes(3)
    expect(core.listenerCount()).toBe(0)
    // invalid listen args
    await core.handle({ t: 'invoke', client: 1, id: 'bad', cmd: 'plugin:event|listen', args: { event: '' } })
    expect((sent.at(-1) as { ok: boolean }).ok).toBe(false)
  })

  it('ignores unknown frames', async () => {
    const { core, sent } = harness()
    await core.handle({ t: 'ping' })
    await core.handle(null as unknown as { t: string })
    expect(sent).toHaveLength(0)
  })
})

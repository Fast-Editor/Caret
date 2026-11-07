/**
 * Desktop-side executor for `caret serve` (docs/REMOTE.md).
 *
 * The Rust server relays every `invoke()` a remote browser makes as `{ t: 'invoke', client, id, cmd, args }` to the
 * desktop webview over a loopback WebSocket. This module runs the real Tauri command here and answers with
 * `{ t: 'reply', client, id, ok, value | error }`. Streams are preserved: `"__CHANNEL__:<id>"` placeholders in the
 * arguments become real `Channel`s whose messages go back as `{ t: 'cb', client, callback, payload }` frames, and
 * `plugin:event|listen` / `unlisten` are mapped onto `listen()` so `on()`/`listen()` work in the browser too.
 *
 * Security: commands that could let a remote client pair more devices or read the executor secret are refused.
 */
import { invoke, Channel } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { api } from './api'
import { on } from './tauri'
import type { RemoteStatus } from '@/types'

export const CHANNEL_RE = /^__CHANNEL__:(\d+)$/
/** Never relayed: a remote client must not be able to escalate its access. */
export const BLOCKED_COMMANDS: ReadonlySet<string> = new Set(['remote_executor_secret', 'remote_revoke_device', 'remote_pairing_code', 'remote_set_enabled', 'remote_restart'])
export const STATUS_EVENT = 'caret://remote/status'

/** Deep-replace `"__CHANNEL__:<id>"` strings with whatever `make(id)` returns (a real `Channel` in production). */
export function replaceChannelPlaceholders(value: unknown, make: (callbackId: number) => unknown): unknown {
  if (typeof value === 'string') { const m = CHANNEL_RE.exec(value); return m ? make(Number(m[1])) : value }
  if (Array.isArray(value)) return value.map((v) => replaceChannelPlaceholders(v, make))
  if (value && typeof value === 'object') {
    const out: Record<string, unknown> = {}
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) out[k] = replaceChannelPlaceholders(v, make)
    return out
  }
  return value
}

/** Errors travel as `{ code, message }` (CaretError) or a string, like Tauri's own rejections. */
export function serializeError(e: unknown): unknown {
  if (e && typeof e === 'object' && 'code' in e && 'message' in e) { const x = e as { code: unknown; message: unknown }; return { code: String(x.code), message: String(x.message) } }
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return String(e)
}

export interface InvokeFrame { t: 'invoke'; client: number; id: unknown; cmd: string; args?: Record<string, unknown> | null; options?: unknown }
export interface ClientGoneFrame { t: 'client_gone'; client: number }
export type ExecutorFrame = InvokeFrame | ClientGoneFrame | { t: string; [k: string]: unknown }
export type OutFrame =
  | { t: 'reply'; client: number; id: unknown; ok: true; value: unknown }
  | { t: 'reply'; client: number; id: unknown; ok: false; error: unknown }
  | { t: 'cb'; client: number; callback: number; payload: unknown }

export interface TauriEventLike { event: string; id: number; payload: unknown }
export interface ExecutorDeps {
  invoke: (cmd: string, args: Record<string, unknown>) => Promise<unknown>
  listen: (event: string, handler: (ev: TauriEventLike) => void) => Promise<UnlistenFn>
  /** Build a channel that calls `onmessage` per message and `onEnd` when the Rust side drops it (optional). */
  makeChannel: (onmessage: (message: unknown) => void, onEnd: () => void) => unknown
  send: (frame: OutFrame) => void
}

export interface ExecutorCore {
  handle: (frame: ExecutorFrame) => Promise<void>
  /** Drop one client's listeners (it disconnected). */
  disposeClient: (client: number) => void
  /** Drop everything (socket closed). */
  disposeAll: () => void
  listenerCount: () => number
}

/** Transport-free executor: feed it frames, it calls `deps.send` with replies and callbacks. */
export function createExecutorCore(deps: ExecutorDeps): ExecutorCore {
  const listeners = new Map<number, Map<number, UnlistenFn>>()
  let nextEventId = 1
  const disposeClient = (client: number) => { const m = listeners.get(client); listeners.delete(client); m?.forEach((un) => { try { un() } catch { /* ignore */ } }) }
  const disposeAll = () => { for (const c of [...listeners.keys()]) disposeClient(c) }

  const handle = async (frame: ExecutorFrame): Promise<void> => {
    if (!frame || typeof frame !== 'object') return
    if (frame.t === 'client_gone') { disposeClient(Number((frame as ClientGoneFrame).client)); return }
    if (frame.t !== 'invoke') return
    const { client, id, cmd } = frame as InvokeFrame
    const args = ((frame as InvokeFrame).args ?? {}) as Record<string, unknown>
    const ok = (value: unknown) => deps.send({ t: 'reply', client, id, ok: true, value: value === undefined ? null : value })
    const fail = (error: unknown) => deps.send({ t: 'reply', client, id, ok: false, error })
    if (typeof cmd !== 'string' || !cmd) { fail({ code: 'invalid', message: 'missing cmd' }); return }
    if (BLOCKED_COMMANDS.has(cmd)) { fail({ code: 'denied', message: `${cmd} is not available in a remote session` }); return }
    try {
      if (cmd === 'plugin:event|listen') {
        const event = String(args.event ?? '')
        const handler = Number(args.handler)
        if (!event || !Number.isFinite(handler)) { fail({ code: 'invalid', message: 'listen needs event and handler' }); return }
        const eventId = nextEventId++
        const un = await deps.listen(event, (ev) => deps.send({ t: 'cb', client, callback: handler, payload: { event: ev.event, id: eventId, payload: ev.payload } }))
        if (!listeners.has(client)) listeners.set(client, new Map())
        listeners.get(client)!.set(eventId, un)
        ok(eventId)
        return
      }
      if (cmd === 'plugin:event|unlisten') {
        const eventId = Number(args.eventId)
        const m = listeners.get(client)
        const un = m?.get(eventId)
        if (un) { m!.delete(eventId); un() }
        ok(null)
        return
      }
      const indices = new Map<number, number>()
      const real = replaceChannelPlaceholders(args, (callbackId) => deps.makeChannel(
        (message) => { const index = indices.get(callbackId) ?? 0; indices.set(callbackId, index + 1); deps.send({ t: 'cb', client, callback: callbackId, payload: { index, message } }) },
        () => { const index = indices.get(callbackId) ?? 0; deps.send({ t: 'cb', client, callback: callbackId, payload: { index, end: true } }) },
      )) as Record<string, unknown>
      ok(await deps.invoke(cmd, real))
    } catch (e) { fail(serializeError(e)) }
  }
  return { handle, disposeClient, disposeAll, listenerCount: () => [...listeners.values()].reduce((n, m) => n + m.size, 0) }
}

// ───────────────────────── production wiring ─────────────────────────

type Internals = { unregisterCallback?: (id: number) => void }
const endHooks = new Map<number, () => void>()
let patchedUnregister = false
/** A real `Channel`; when Tauri cleans its callback up (Rust dropped the channel) the remote side is told too. */
function makeRealChannel(onmessage: (m: unknown) => void, onEnd: () => void): unknown {
  const ch = new Channel<unknown>()
  ch.onmessage = onmessage
  endHooks.set(ch.id, onEnd)
  const internals = (window as unknown as { __TAURI_INTERNALS__?: Internals }).__TAURI_INTERNALS__
  if (!patchedUnregister && internals?.unregisterCallback) {
    patchedUnregister = true
    const orig = internals.unregisterCallback
    internals.unregisterCallback = (id: number) => { const hook = endHooks.get(id); if (hook) { endHooks.delete(id); try { hook() } catch { /* ignore */ } } return orig.call(internals, id) }
  }
  return ch
}

let active: (() => void) | null = null

/**
 * Connect this desktop webview to the local relay as the executor. Safe to call when remote access is off: it checks
 * `remote_status` once and otherwise waits for the `caret://remote/status` event. Returns a disposer.
 */
export function startRemoteExecutor(): () => void {
  if (typeof window === 'undefined' || (window as unknown as { __CARET_REMOTE__?: unknown }).__CARET_REMOTE__) return () => {}
  if (active) return active
  let disposed = false
  let ws: WebSocket | null = null
  let attempts = 0
  let timer: number | undefined
  let connecting = false
  const core = createExecutorCore({
    invoke: (cmd, args) => invoke(cmd, args),
    listen: (event, handler) => listen(event, (ev) => handler({ event: ev.event, id: ev.id, payload: ev.payload })),
    makeChannel: makeRealChannel,
    send: (frame) => { if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(frame)) },
  })
  const schedule = () => {
    if (disposed || timer !== undefined) return
    attempts += 1
    timer = window.setTimeout(() => { timer = undefined; void connect() }, Math.min(30_000, 1000 * 2 ** Math.min(attempts, 5)))
  }
  const connect = async () => {
    if (disposed || ws || connecting) return
    connecting = true
    try {
      const st = await api.remote.status()
      if (!st.running) return // wait for the status event
      const secret = await api.remote.executorSecret()
      const sock = new WebSocket(`ws://127.0.0.1:${st.port}/ws?role=executor&secret=${encodeURIComponent(secret)}`)
      ws = sock
      sock.onopen = () => { attempts = 0; console.info('[remote] executor connected') }
      sock.onmessage = (ev) => { let f: ExecutorFrame; try { f = JSON.parse(String(ev.data)) as ExecutorFrame } catch { return } void core.handle(f) }
      sock.onclose = () => { if (ws === sock) ws = null; core.disposeAll(); if (!disposed) schedule() }
      sock.onerror = () => { /* onclose follows */ }
    } catch (e) { console.warn('[remote] executor could not connect', e); schedule() } finally { connecting = false }
  }
  void connect()
  const off = on<RemoteStatus>(STATUS_EVENT, (st) => { if (st.running && !ws) { attempts = 0; if (timer !== undefined) { window.clearTimeout(timer); timer = undefined } void connect() } })
  const dispose = () => { disposed = true; off(); if (timer !== undefined) window.clearTimeout(timer); ws?.close(); ws = null; core.disposeAll(); active = null }
  active = dispose
  return dispose
}

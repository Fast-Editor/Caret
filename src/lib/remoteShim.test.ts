/**
 * Loads src-tauri/remote/shim.js (plain JS served to browsers by `caret serve`) into a fake window with a fake
 * WebSocket and checks the Tauri internals it provides: invoke → frame, reply → promise, transformCallback → cb.
 */
import { describe, it, expect, beforeEach } from 'vitest'
import { readFileSync } from 'node:fs'
import path from 'node:path'

const SHIM = readFileSync(path.resolve(__dirname, '../../src-tauri/remote/shim.js'), 'utf8')

class FakeSocket {
  static instances: FakeSocket[] = []
  url: string
  readyState = 0
  sent: string[] = []
  onopen: (() => void) | null = null
  onmessage: ((ev: { data: string }) => void) | null = null
  onclose: ((ev: { code: number }) => void) | null = null
  onerror: (() => void) | null = null
  constructor(url: string) { this.url = url; FakeSocket.instances.push(this) }
  send(data: string) { this.sent.push(data) }
  close() { this.readyState = 3; this.onclose?.({ code: 1000 }) }
  open() { this.readyState = 1; this.onopen?.() }
  receive(frame: unknown) { this.onmessage?.({ data: JSON.stringify(frame) }) }
}

type Win = Record<string, unknown> & {
  __TAURI_INTERNALS__?: { invoke: (cmd: string, args?: unknown, options?: unknown) => Promise<unknown>; transformCallback: (cb: (r: unknown) => void, once?: boolean) => number; unregisterCallback: (id: number) => void; convertFileSrc: (p: string) => string; metadata: { currentWindow: { label: string }; currentWebview: { label: string; windowLabel: string } }; plugins: { path: { sep: string } } }
  __CARET_REMOTE__?: { connected: boolean; token: string | null; url: string }
  __TAURI_OS_PLUGIN_INTERNALS__?: { platform: string }
  isTauri?: boolean
}

function load(opts: { cookie?: string; search?: string; stored?: string | null } = {}) {
  FakeSocket.instances = []
  const storage = new Map<string, string>()
  if (opts.stored) storage.set('caret_token', opts.stored)
  const attrs: Record<string, string> = {}
  const listeners: Record<string, ((e: unknown) => void)[]> = {}
  const location = { protocol: 'http:', host: '192.168.1.5:17430', origin: 'http://192.168.1.5:17430', pathname: '/', search: opts.search ?? '', hash: '', replaced: '' as string, replace(u: string) { this.replaced = u }, reload() { /* noop */ } }
  const win: Win = {
    location,
    document: { cookie: opts.cookie ?? '', documentElement: { setAttribute: (k: string, v: string) => { attrs[k] = v } }, body: { setAttribute: (k: string, v: string) => { attrs['body:' + k] = v } }, addEventListener: () => {} },
    localStorage: { getItem: (k: string) => storage.get(k) ?? null, setItem: (k: string, v: string) => { storage.set(k, v) }, removeItem: (k: string) => { storage.delete(k) } },
    navigator: { platform: 'MacIntel', userAgent: 'test', language: 'en-US' },
    history: { replaceState: () => {} },
    crypto: { getRandomValues: (a: Uint32Array) => { a[0] = Math.floor(Math.random() * 0xffffffff) + 1; return a } },
    addEventListener: (k: string, fn: (e: unknown) => void) => { (listeners[k] ??= []).push(fn) },
    dispatchEvent: (e: { type: string }) => { (listeners[e.type] ?? []).forEach((fn) => fn(e)); return true },
    setTimeout: (fn: () => void, ms: number) => setTimeout(fn, ms),
    CustomEvent: class { type: string; detail: unknown; constructor(type: string, init?: { detail?: unknown }) { this.type = type; this.detail = init?.detail } },
    WebSocket: FakeSocket,
    URLSearchParams,
    Promise, JSON, Math, Object, String, Number, Array,
  }
  // the shim references these as free identifiers: run it with `window` and the globals bound to the fake
  const fn = new Function('window', 'document', 'location', 'localStorage', 'navigator', 'history', 'crypto', 'WebSocket', 'setTimeout', 'CustomEvent', SHIM)
  fn(win, win.document, win.location, win.localStorage, win.navigator, win.history, win.crypto, FakeSocket, win.setTimeout, win.CustomEvent)
  return { win, attrs, location, storage }
}

describe('remote shim', () => {
  beforeEach(() => { FakeSocket.instances = [] })

  it('redirects to the pairing page when there is no token', () => {
    const { win, location } = load()
    expect(location.replaced).toBe('/__remote/pair.html')
    expect(FakeSocket.instances).toHaveLength(0)
    expect(win.__TAURI_INTERNALS__).toBeDefined()
  })

  it('defines the Tauri internals and connects with the cookie token', () => {
    const { win, attrs, storage } = load({ cookie: 'x=1; caret_token=abc123' })
    expect(win.isTauri).toBe(true)
    expect(win.__CARET_REMOTE__).toMatchObject({ connected: false, token: 'abc123', url: 'http://192.168.1.5:17430' })
    expect(win.__TAURI_INTERNALS__!.metadata.currentWindow.label).toBe('main')
    expect(win.__TAURI_INTERNALS__!.metadata.currentWebview.windowLabel).toBe('main')
    expect(win.__TAURI_INTERNALS__!.plugins.path.sep).toBe('/')
    expect(win.__TAURI_INTERNALS__!.convertFileSrc('/a/b.png')).toBe('/a/b.png')
    expect(win.__TAURI_OS_PLUGIN_INTERNALS__!.platform).toBe('macos')
    expect(attrs['data-remote']).toBe('1')
    expect(storage.get('caret_token')).toBe('abc123')
    expect(FakeSocket.instances).toHaveLength(1)
    expect(FakeSocket.instances[0].url).toBe('ws://192.168.1.5:17430/ws?role=client&token=abc123')
  })

  it('queues invokes until open, serialises channels, resolves replies and routes callbacks', async () => {
    const { win } = load({ cookie: 'caret_token=tok' })
    const internals = win.__TAURI_INTERNALS__!
    const sock = FakeSocket.instances[0]
    const got: unknown[] = []
    const cbId = internals.transformCallback((r) => got.push(r))
    expect(typeof cbId).toBe('number')
    expect(typeof (win as Record<string, unknown>)['_' + cbId]).toBe('function')
    const channelLike = { toJSON: () => `__CHANNEL__:${cbId}` }
    const p = internals.invoke('search_text', { query: 'x', onEvent: channelLike })
    expect(sock.sent).toHaveLength(0) // not open yet
    sock.open()
    expect(sock.sent).toHaveLength(1)
    const frame = JSON.parse(sock.sent[0])
    expect(frame).toEqual({ t: 'invoke', id: 1, cmd: 'search_text', args: { query: 'x', onEvent: `__CHANNEL__:${cbId}` }, options: null })
    expect(win.__CARET_REMOTE__!.connected).toBe(true)
    // callback frame → registered callback
    sock.receive({ t: 'cb', callback: cbId, payload: { index: 0, message: 'hit' } })
    expect(got).toEqual([{ index: 0, message: 'hit' }])
    // reply resolves the promise
    sock.receive({ t: 'reply', id: 1, ok: true, value: 'search-1' })
    await expect(p).resolves.toBe('search-1')
    // error reply rejects with the error payload
    const p2 = internals.invoke('x', {})
    sock.receive({ t: 'reply', id: 2, ok: false, error: { code: 'io', message: 'bad' } })
    await expect(p2).rejects.toEqual({ code: 'io', message: 'bad' })
    // unregister removes the callback
    internals.unregisterCallback(cbId)
    sock.receive({ t: 'cb', callback: cbId, payload: 'late' })
    expect(got).toHaveLength(1)
    // once callbacks self-unregister
    const onceGot: unknown[] = []
    const onceId = internals.transformCallback((r) => onceGot.push(r), true)
    sock.receive({ t: 'cb', callback: onceId, payload: 1 })
    sock.receive({ t: 'cb', callback: onceId, payload: 2 })
    expect(onceGot).toEqual([1])
  })

  it('keeps window chrome local, rejects zoom so the app falls back to CSS zoom, and fails pending calls on close', async () => {
    const { win } = load({ search: '?token=fromquery' })
    const internals = win.__TAURI_INTERNALS__!
    expect(win.__CARET_REMOTE__!.token).toBe('fromquery')
    const sock = FakeSocket.instances[0]
    sock.open()
    await expect(internals.invoke('plugin:window|close', { label: 'main' })).resolves.toBeNull()
    await expect(internals.invoke('plugin:window|toggle_maximize', { label: 'main' })).resolves.toBeNull()
    await expect(internals.invoke('plugin:webview|set_webview_zoom', { value: 1.1 })).rejects.toMatch(/zoom/)
    await expect(internals.invoke('plugin:process|exit', { code: 0 })).rejects.toMatch(/not available/)
    expect(sock.sent).toHaveLength(0)
    const p = internals.invoke('app_info')
    expect(sock.sent).toHaveLength(1)
    sock.close()
    await expect(p).rejects.toMatch(/connection lost/)
    expect(win.__CARET_REMOTE__!.connected).toBe(false)
  })
})

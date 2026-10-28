/**
 * Thin typed bridge over Tauri IPC. All UI code goes through `api.ts`; this file
 * normalises errors and wraps Channels / events.
 */
import { invoke as tauriInvoke, Channel } from '@tauri-apps/api/core'
import { listen, type UnlistenFn, type Event as TauriEvent } from '@tauri-apps/api/event'
import type { CaretError } from '@/types'

export class CaretApiError extends Error {
  code: string
  constructor(err: CaretError) {
    super(err.message)
    this.name = 'CaretApiError'
    this.code = err.code
  }
}

export function isCaretError(e: unknown): e is CaretApiError {
  return e instanceof CaretApiError
}

/** Call a Rust command. Rust errors (`{code,message}`) surface as `CaretApiError`. */
export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await tauriInvoke<T>(cmd, args)
  } catch (e: unknown) {
    if (e && typeof e === 'object' && 'code' in e && 'message' in e) {
      throw new CaretApiError(e as CaretError)
    }
    if (typeof e === 'string') throw new CaretApiError({ code: 'ipc', message: e })
    throw e
  }
}

/** Create a streaming channel; Rust side receives `tauri::ipc::Channel<T>`. */
export function channel<T>(onMessage: (msg: T) => void): Channel<T> {
  const ch = new Channel<T>()
  ch.onmessage = onMessage
  return ch
}

export type { Channel }

/** Subscribe to a backend broadcast. Returns an unsubscribe function. */
export function on<T>(event: string, handler: (payload: T) => void): () => void {
  let unlisten: UnlistenFn | null = null
  let disposed = false
  listen<T>(event, (e: TauriEvent<T>) => handler(e.payload)).then((fn) => {
    if (disposed) fn()
    else unlisten = fn
  })
  return () => {
    disposed = true
    unlisten?.()
  }
}

export const Events = {
  fsChanged: 'caret://fs/changed',
  gitChanged: 'caret://git/changed',
  settingsChanged: 'caret://settings/changed',
  routerStatus: 'caret://router/status',
  indexStatus: 'caret://index/status',
  mcpChanged: 'caret://mcp/changed',
  notify: 'caret://notify',
  workspaceChanged: 'caret://workspace/changed',
  lspStatus: 'caret://lsp/status',
  lspDiagnostics: 'caret://lsp/diagnostics',
  agentEvent: 'caret://agent/event',
  reviewChanged: 'caret://review/changed',
  rulesChanged: 'caret://rules/changed',
  menu: 'caret://menu',
  docsChanged: 'caret://docs/changed',
} as const

export const isTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

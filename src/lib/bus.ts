/** Tiny typed event bus for intra-UI notifications (not for backend events — see tauri.ts `on`). */
type Handler<T> = (payload: T) => void
const handlers = new Map<string, Set<Handler<unknown>>>()

export function emit<T = unknown>(topic: string, payload: T): void {
  handlers.get(topic)?.forEach((h) => { try { h(payload) } catch (e) { console.error(`[bus:${topic}]`, e) } })
}
export function subscribe<T = unknown>(topic: string, handler: Handler<T>): () => void {
  let set = handlers.get(topic)
  if (!set) { set = new Set(); handlers.set(topic, set) }
  set.add(handler as Handler<unknown>)
  return () => { set!.delete(handler as Handler<unknown>) }
}

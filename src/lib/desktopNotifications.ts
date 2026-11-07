/**
 * Desktop notifications for agent threads. `observeAgentEvent` sits on the raw AgentEvent stream (chatBridge) and
 * turns session transitions into at most one OS notification each:
 *   running → idle                       finished (skipped when the turn was cancelled)
 *   → error / message-end error / error  failed
 *   approval-request                     needs approval
 *   question                             asked a question
 * Subagent sessions never notify. `AgentNotifier` holds the pure decision logic with injectable deps so it is
 * unit-testable; the module-level singleton wires it to the stores and `api.app.notifyDesktop`.
 */
import type { AgentEvent, AgentNotificationKind, NotificationsSettings, SessionStatus } from '@/types'
import { api } from '@/lib/api'
import { useSettings } from '@/store/settings'
import { useUi } from '@/store/ui'
import { useChat } from '@/store/chat'

export interface SessionSnapshot { title: string; isSubagent?: boolean; status?: SessionStatus; lastAssistantText?: string; lastMessageStatus?: 'streaming' | 'done' | 'error' | 'cancelled' }

export interface NotifierDeps {
  settings: () => { notifications: NotificationsSettings; chimeOnFinish: boolean } | null
  windowFocused: () => boolean
  session: (id: string) => SessionSnapshot | null
  notify: (title: string, body: string, kind: AgentNotificationKind, sessionId: string) => void
  playSound: () => void
  now?: () => number
}

const BUSY: readonly SessionStatus[] = ['running', 'awaiting-approval', 'awaiting-input', 'summarizing']
/** The backend emits several events per transition (message-end + session-status, approval-request + session-status); one notification per kind per session inside this window. */
export const COALESCE_MS = 1500

const KIND_SETTING: Record<AgentNotificationKind, keyof NotificationsSettings> = { finished: 'onFinish', failed: 'onError', approval: 'onApproval', question: 'onQuestion' }

function oneLine(s: string | undefined, max = 140): string {
  const t = (s ?? '').replace(/\s+/g, ' ').trim()
  return t.length > max ? `${t.slice(0, max - 1)}…` : t
}

export function bodyFor(kind: AgentNotificationKind, detail: string | undefined, session: SessionSnapshot | null): string {
  switch (kind) {
    case 'finished': { const p = oneLine(detail ?? session?.lastAssistantText); return p ? `Finished: ${p}` : 'Finished.' }
    case 'failed': { const p = oneLine(detail); return p ? `Failed: ${p}` : 'The agent stopped with an error.' }
    case 'approval': { const p = oneLine(detail); return p ? `Needs approval: ${p}` : 'Waiting for your approval.' }
    case 'question': { const p = oneLine(detail); return p || 'The agent asked a question.' }
  }
}

export class AgentNotifier {
  private last = new Map<string, SessionStatus>()
  private recent = new Map<string, number>()
  constructor(private deps: NotifierDeps) {}

  observe(e: AgentEvent): void {
    switch (e.type) {
      case 'session-status': {
        const prev = this.last.get(e.sessionId) ?? this.deps.session(e.sessionId)?.status
        this.last.set(e.sessionId, e.status)
        if (e.status === 'idle') {
          if (!prev || !BUSY.includes(prev)) return
          if (this.deps.session(e.sessionId)?.lastMessageStatus === 'cancelled') return
          this.fire(e.sessionId, 'finished')
        } else if (e.status === 'error') this.fire(e.sessionId, 'failed')
        // awaiting-approval / awaiting-input are reported through approval-request / question, which carry the detail.
        return
      }
      case 'message-end': if (e.status === 'error') this.fire(e.sessionId, 'failed', e.error); return
      case 'error': this.fire(e.sessionId, 'failed', e.message); return
      case 'approval-request': this.fire(e.sessionId, 'approval', e.request.title); return
      case 'question': this.fire(e.sessionId, 'question', e.question.question); return
      case 'session-deleted': this.last.delete(e.sessionId); return
      default: return
    }
  }

  private fire(sessionId: string, kind: AgentNotificationKind, detail?: string): void {
    const session = this.deps.session(sessionId)
    if (session?.isSubagent) return
    const now = (this.deps.now ?? Date.now)()
    const key = `${sessionId}:${kind}`
    const prev = this.recent.get(key)
    if (prev !== undefined && now - prev < COALESCE_MS) return
    this.recent.set(key, now)
    const s = this.deps.settings()
    if (!s) return
    const n = s.notifications
    if (kind === 'finished' && (s.chimeOnFinish || n.sound)) this.deps.playSound()
    if (!n.desktop || !n[KIND_SETTING[kind]]) return
    if (n.onlyWhenUnfocused && this.deps.windowFocused()) return
    this.deps.notify(session?.title || 'Agent', bodyFor(kind, detail, session), kind, sessionId)
  }
}

// ───────────── wiring ─────────────

let audio: AudioContext | null = null
/** Two-tone Web Audio beep (no asset, ~0.35 s). Silently does nothing without an audio device. */
export function playChime(): void {
  try {
    const Ctx = (globalThis as { AudioContext?: typeof AudioContext; webkitAudioContext?: typeof AudioContext }).AudioContext ?? (globalThis as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
    if (!Ctx) return
    audio ??= new Ctx()
    if (audio.state === 'suspended') void audio.resume()
    const t = audio.currentTime
    const gain = audio.createGain(); gain.connect(audio.destination)
    gain.gain.setValueAtTime(0.0001, t); gain.gain.exponentialRampToValueAtTime(0.12, t + 0.015); gain.gain.exponentialRampToValueAtTime(0.0001, t + 0.35)
    const osc = audio.createOscillator(); osc.type = 'sine'; osc.frequency.setValueAtTime(880, t); osc.frequency.setValueAtTime(1174.66, t + 0.12)
    osc.connect(gain); osc.start(t); osc.stop(t + 0.4)
  } catch { /* no audio */ }
}

const storeDeps: NotifierDeps = {
  settings: () => { const s = useSettings.getState().settings; return s ? { notifications: s.notifications, chimeOnFinish: s.agent.chimeOnFinish } : null },
  windowFocused: () => useUi.getState().windowFocused,
  session: (id) => {
    const c = useChat.getState()
    const loaded = c.loaded[id]
    if (loaded) {
      const lastAssistant = [...loaded.messages].reverse().find((m) => m.role === 'assistant')
      const text = lastAssistant?.parts.map((p) => (p.type === 'text' ? p.text : '')).join(' ')
      return { title: loaded.title, isSubagent: loaded.isSubagent, status: loaded.status, lastAssistantText: text, lastMessageStatus: loaded.messages[loaded.messages.length - 1]?.status }
    }
    const sum = c.sessions.find((x) => x.id === id)
    return sum ? { title: sum.title, isSubagent: sum.isSubagent, status: sum.status, lastAssistantText: sum.lastMessagePreview } : null
  },
  notify: (title, body) => { api.app.notifyDesktop(title, body).catch((e: unknown) => console.warn('[notifications] desktop notification failed', e)) },
  playSound: playChime,
}

let singleton: AgentNotifier | null = null
/** Feed one raw agent event (called by chatBridge for every event, before batching). */
export function observeAgentEvent(e: AgentEvent): void { (singleton ??= new AgentNotifier(storeDeps)).observe(e) }

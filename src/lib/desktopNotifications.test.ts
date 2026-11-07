import { describe, expect, it } from 'vitest'
import { AgentNotifier, COALESCE_MS, bodyFor, type NotifierDeps, type SessionSnapshot } from './desktopNotifications'
import type { AgentEvent, NotificationsSettings, SessionStatus } from '@/types'

const allOn: NotificationsSettings = { desktop: true, onlyWhenUnfocused: true, onFinish: true, onError: true, onApproval: true, onQuestion: true, sound: false }

function harness(over: { notifications?: Partial<NotificationsSettings>; chime?: boolean; focused?: boolean; session?: Partial<SessionSnapshot> | null } = {}) {
  const sent: { title: string; body: string; kind: string; sessionId: string }[] = []
  let sounds = 0
  let now = 1_000_000
  const session: SessionSnapshot | null = over.session === null ? null : { title: 'Fix discount bug', status: 'running', lastAssistantText: 'Done, all tests pass.', ...over.session }
  const deps: NotifierDeps = {
    settings: () => ({ notifications: { ...allOn, ...over.notifications }, chimeOnFinish: over.chime ?? false }),
    windowFocused: () => over.focused ?? false,
    session: () => session,
    notify: (title, body, kind, sessionId) => { sent.push({ title, body, kind, sessionId }) },
    playSound: () => { sounds += 1 },
    now: () => now,
  }
  const n = new AgentNotifier(deps)
  return { n, sent, sounds: () => sounds, advance: (ms: number) => { now += ms } }
}
const status = (s: SessionStatus, sessionId = 's1'): AgentEvent => ({ type: 'session-status', sessionId, status: s })

describe('AgentNotifier', () => {
  it('notifies once when a running thread goes idle, with the thread title and a one-line preview', () => {
    const h = harness()
    h.n.observe(status('running'))
    h.n.observe(status('idle'))
    expect(h.sent).toEqual([{ title: 'Fix discount bug', body: 'Finished: Done, all tests pass.', kind: 'finished', sessionId: 's1' }])
    // a second idle (e.g. duplicate event) inside the window is coalesced
    h.n.observe(status('running')); h.n.observe(status('idle'))
    expect(h.sent).toHaveLength(1)
    h.advance(COALESCE_MS + 1)
    h.n.observe(status('running')); h.n.observe(status('idle'))
    expect(h.sent).toHaveLength(2)
  })
  it('does not treat idle-from-idle, idle-after-error or a cancelled turn as finished', () => {
    const h = harness({ session: { status: 'idle' } })
    h.n.observe(status('idle'))
    expect(h.sent).toHaveLength(0)
    h.n.observe(status('running')); h.n.observe(status('error')); h.n.observe(status('idle'))
    expect(h.sent.map((s) => s.kind)).toEqual(['failed'])
    const c = harness({ session: { lastMessageStatus: 'cancelled' } })
    c.n.observe(status('running')); c.n.observe(status('idle'))
    expect(c.sent).toHaveLength(0)
  })
  it('seeds the previous status from the store when it has not seen the session yet', () => {
    const h = harness({ session: { status: 'running' } })
    h.n.observe(status('idle'))
    expect(h.sent.map((s) => s.kind)).toEqual(['finished'])
  })
  it('coalesces message-end error, the error event and the error status into one failure with the message', () => {
    const h = harness()
    h.n.observe(status('running'))
    h.n.observe({ type: 'message-end', sessionId: 's1', messageId: 'm1', status: 'error', error: 'Rate limited (429)', durationMs: 5 })
    h.n.observe({ type: 'error', sessionId: 's1', message: 'Rate limited (429)' })
    h.n.observe(status('error'))
    expect(h.sent).toEqual([{ title: 'Fix discount bug', body: 'Failed: Rate limited (429)', kind: 'failed', sessionId: 's1' }])
  })
  it('uses the approval title and the question text; the matching status changes do not double up', () => {
    const h = harness()
    h.n.observe(status('running'))
    h.n.observe(status('awaiting-approval'))
    h.n.observe({ type: 'approval-request', sessionId: 's1', request: { approvalId: 'a', sessionId: 's1', toolCallId: 't', kind: 'command', title: 'Run npm test', detail: '', options: ['allow', 'deny'] } })
    h.n.observe(status('running'))
    h.n.observe(status('awaiting-input'))
    h.n.observe({ type: 'question', sessionId: 's1', messageId: 'm1', question: { questionId: 'q', question: 'Which database should I use?', allowFreeText: true } })
    expect(h.sent.map((s) => [s.kind, s.body])).toEqual([['approval', 'Needs approval: Run npm test'], ['question', 'Which database should I use?']])
  })
  it('respects the master switch, per-kind toggles and the focus rule', () => {
    const off = harness({ notifications: { desktop: false } })
    off.n.observe(status('running')); off.n.observe(status('idle'))
    expect(off.sent).toHaveLength(0)
    const noFinish = harness({ notifications: { onFinish: false } })
    noFinish.n.observe(status('running')); noFinish.n.observe(status('idle'))
    noFinish.n.observe({ type: 'error', sessionId: 's1', message: 'boom' })
    expect(noFinish.sent.map((s) => s.kind)).toEqual(['failed'])
    const focused = harness({ focused: true })
    focused.n.observe(status('running')); focused.n.observe(status('idle'))
    expect(focused.sent).toHaveLength(0)
    const focusedButAllowed = harness({ focused: true, notifications: { onlyWhenUnfocused: false } })
    focusedButAllowed.n.observe(status('running')); focusedButAllowed.n.observe(status('idle'))
    expect(focusedButAllowed.sent).toHaveLength(1)
  })
  it('skips subagent sessions entirely', () => {
    const h = harness({ session: { isSubagent: true } })
    h.n.observe(status('running')); h.n.observe(status('idle'))
    h.n.observe({ type: 'error', sessionId: 's1', message: 'boom' })
    expect(h.sent).toHaveLength(0)
  })
  it('plays the chime on finish when agent.chimeOnFinish or notifications.sound is on, even while focused or with desktop off', () => {
    const chime = harness({ chime: true, focused: true, notifications: { desktop: false } })
    chime.n.observe(status('running')); chime.n.observe(status('idle'))
    expect(chime.sounds()).toBe(1); expect(chime.sent).toHaveLength(0)
    const sound = harness({ notifications: { sound: true } })
    sound.n.observe(status('running')); sound.n.observe(status('idle'))
    sound.n.observe({ type: 'error', sessionId: 's1', message: 'boom' })
    expect(sound.sounds()).toBe(1)
    const silent = harness()
    silent.n.observe(status('running')); silent.n.observe(status('idle'))
    expect(silent.sounds()).toBe(0)
  })
  it('falls back to a generic title and body when the session is unknown', () => {
    const h = harness({ session: null })
    h.n.observe(status('running')); h.n.observe(status('idle'))
    expect(h.sent[0]).toMatchObject({ title: 'Agent', body: 'Finished.' })
    expect(bodyFor('failed', undefined, null)).toBe('The agent stopped with an error.')
    expect(bodyFor('approval', '  ', null)).toBe('Waiting for your approval.')
    expect(bodyFor('finished', 'x'.repeat(200), null).length).toBeLessThanOrEqual('Finished: '.length + 140)
  })
})

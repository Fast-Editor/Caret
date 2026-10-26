import { create } from 'zustand'
import type { AgentEvent, PendingChange, ReviewComment, ReviewFinding, ReviewGroup, ReviewRequest, ReviewResult, ReviewStreamEvent, VerificationReport, VerifyEvent, VerifyScope } from '@/types'
import { api, apiWave1 } from '@/lib/api'
import { Events, on } from '@/lib/tauri'
import { emit as busEmit } from '@/lib/bus'
import { computeGroups, overallFailing, type GroupInput } from '@/lib/review'

export interface VerifySummary { sessionId: string; riskTier: VerificationReport['riskTier'] | null; status: VerificationReport['status'] | null; failing: boolean; reportId: string | null }

interface ReviewState {
  // ── Bugbot (model review) ──
  reviews: ReviewResult[]
  current: ReviewResult | null
  streamingFindings: ReviewFinding[]
  running: boolean
  status: string
  error: string | null
  dismissed: Set<string>
  scope: ReviewRequest['scope']
  base: string
  sha: string
  focus: string
  loaded: boolean
  load: () => Promise<void>
  run: (req?: Partial<ReviewRequest>) => Promise<void>
  select: (id: string) => void
  dismissReview: (id: string) => Promise<void>
  dismissFinding: (reviewId: string, findingId: string) => Promise<void>
  set: (p: Partial<Pick<ReviewState, 'scope' | 'base' | 'sha' | 'focus'>>) => void

  // ── Verification (Verified Diff) ──
  reportsById: Record<string, VerificationReport>
  reportOrder: string[]
  latestBySession: Record<string, string>
  verifying: Record<string, boolean>
  verifyError: Record<string, string | undefined>
  selectedReportId: string | null
  runVerification: (sessionId: string, scope?: VerifyScope) => Promise<string | null>
  cancelVerification: (reportId: string) => Promise<void>
  loadLatest: (sessionId: string) => Promise<VerificationReport | null>
  loadReports: (sessionId?: string) => Promise<void>
  selectReport: (id: string | null) => void
  applyVerifyEvent: (e: VerifyEvent, sessionId?: string) => void
  applyFix: (reportId: string, findingId: string) => Promise<PendingChange | null>

  // ── Review comments & groups ──
  comments: Record<string, ReviewComment[]>
  groups: Record<string, ReviewGroup[]>
  loadComments: (sessionId: string) => Promise<void>
  addComment: (sessionId: string, c: Omit<ReviewComment, 'id' | 'createdAt' | 'author'>) => Promise<ReviewComment>
  resolveComment: (sessionId: string, commentId: string, resolved?: boolean) => Promise<void>
  deleteComment: (sessionId: string, commentId: string) => Promise<void>
  sendComments: (sessionId: string, text?: string) => Promise<void>
  loadGroups: (sessionId: string, fallback?: GroupInput) => Promise<ReviewGroup[]>
  ingestAgentEvent: (e: AgentEvent) => void
}

function uid(prefix: string): string { return `${prefix}_${Math.random().toString(36).slice(2, 10)}${Date.now().toString(36)}` }
const notImpl = (e: unknown) => /not implemented|not found|unknown command|command .* not found/i.test(String((e as Error)?.message ?? e))

function publishSummary(get: () => ReviewState, sessionId: string) {
  const s = get()
  const id = s.latestBySession[sessionId]
  const r = id ? s.reportsById[id] : undefined
  const summary: VerifySummary = { sessionId, riskTier: r?.riskTier ?? null, status: r?.status ?? null, failing: overallFailing(r), reportId: id ?? null }
  busEmit('verify.summary', summary)
}

export const useReview = create<ReviewState>((set, get) => ({
  reviews: [], current: null, streamingFindings: [], running: false, status: '', error: null, dismissed: new Set(), scope: 'working-tree', base: '', sha: '', focus: '', loaded: false,
  load: async () => {
    try { const reviews = await api.ai.reviews(); set({ reviews, loaded: true, current: get().current ?? reviews[0] ?? null }) } catch { set({ loaded: true }) }
  },
  run: async (req) => {
    const s = get()
    const request: ReviewRequest = { scope: req?.scope ?? s.scope, base: req?.base ?? (s.base || undefined), sha: req?.sha ?? (s.sha || undefined), files: req?.files, focus: req?.focus ?? (s.focus || undefined), modelId: req?.modelId }
    set({ running: true, status: 'Starting review…', error: null, streamingFindings: [], current: null })
    try {
      await api.ai.review(request, (e: ReviewStreamEvent) => {
        if (e.type === 'status') set({ status: e.message })
        else if (e.type === 'finding') set((st) => ({ streamingFindings: [...st.streamingFindings, e.finding] }))
        else if (e.type === 'done') set((st) => ({ running: false, status: '', current: e.result, reviews: [e.result, ...st.reviews.filter((r) => r.id !== e.result.id)], streamingFindings: [] }))
        else if (e.type === 'error') set({ running: false, error: e.message, status: '' })
      })
    } catch (e) {
      set({ running: false, error: e instanceof Error ? e.message : String(e), status: '' })
    }
  },
  select: (id) => set((s) => ({ current: s.reviews.find((r) => r.id === id) ?? s.current })),
  dismissReview: async (id) => { await api.ai.dismissReview(id); set((s) => { const reviews = s.reviews.filter((r) => r.id !== id); return { reviews, current: s.current?.id === id ? reviews[0] ?? null : s.current } }) },
  dismissFinding: async (reviewId, findingId) => {
    await api.ai.dismissFinding(reviewId, findingId).catch(() => {})
    set((s) => { const d = new Set(s.dismissed); d.add(findingId); return { dismissed: d } })
  },
  set: (p) => set(p),

  // ── verification ──
  reportsById: {}, reportOrder: [], latestBySession: {}, verifying: {}, verifyError: {}, selectedReportId: null,
  applyVerifyEvent: (e, sessionId) => {
    const upsert = (r: VerificationReport) => set((s) => {
      const order = s.reportOrder.includes(r.id) ? s.reportOrder : [r.id, ...s.reportOrder]
      const sid = r.sessionId ?? sessionId
      const latest = sid ? { ...s.latestBySession, [sid]: r.id } : s.latestBySession
      return { reportsById: { ...s.reportsById, [r.id]: r }, reportOrder: order, latestBySession: latest }
    })
    switch (e.type) {
      case 'report': upsert(e.report); break
      case 'check': set((s) => { const r = s.reportsById[e.reportId]; if (!r) return {}; const idx = r.checks.findIndex((c) => c.id === e.check.id || c.kind === e.check.kind); const checks = idx >= 0 ? r.checks.map((c, i) => (i === idx ? e.check : c)) : [...r.checks, e.check]; return { reportsById: { ...s.reportsById, [r.id]: { ...r, checks } } } }); break
      case 'done': { upsert(e.report); const sid = e.report.sessionId ?? sessionId; if (sid) { set((s) => ({ verifying: { ...s.verifying, [sid]: false } })); publishSummary(get, sid) } break }
      case 'error': { const sid = sessionId ?? (e.reportId ? get().reportsById[e.reportId]?.sessionId : undefined); if (sid) set((s) => ({ verifying: { ...s.verifying, [sid]: false }, verifyError: { ...s.verifyError, [sid]: e.message } })); if (e.reportId) set((s) => { const r = s.reportsById[e.reportId!]; return r ? { reportsById: { ...s.reportsById, [r.id]: { ...r, status: 'error', summary: e.message, finishedAt: Date.now() } } } : {} }); break }
    }
  },
  runVerification: async (sessionId, scope) => {
    set((s) => ({ verifying: { ...s.verifying, [sessionId]: true }, verifyError: { ...s.verifyError, [sessionId]: undefined } }))
    try {
      const id = await apiWave1.verify.run(scope ?? { kind: 'session', sessionId }, (e) => get().applyVerifyEvent(e, sessionId))
      return id
    } catch (e) {
      const msg = String((e as Error)?.message ?? e)
      set((s) => ({ verifying: { ...s.verifying, [sessionId]: false }, verifyError: { ...s.verifyError, [sessionId]: notImpl(e) ? 'Verification engine is not available yet.' : msg } }))
      return null
    }
  },
  cancelVerification: async (reportId) => { try { await apiWave1.verify.cancel(reportId) } catch { /* ignore */ } },
  loadLatest: async (sessionId) => {
    try {
      const r = await apiWave1.verify.latestForSession(sessionId)
      if (r) { get().applyVerifyEvent({ type: 'report', report: r }, sessionId); publishSummary(get, sessionId) }
      return r
    } catch { return null }
  },
  loadReports: async (sessionId) => {
    try {
      const list = await apiWave1.verify.list(sessionId, 30)
      set((s) => { const byId = { ...s.reportsById }; const order = [...s.reportOrder]; for (const r of list) { byId[r.id] = r; if (!order.includes(r.id)) order.push(r.id) } order.sort((a, b) => (byId[b]?.startedAt ?? 0) - (byId[a]?.startedAt ?? 0)); return { reportsById: byId, reportOrder: order } })
    } catch { /* backend not ready */ }
  },
  selectReport: (id) => set({ selectedReportId: id }),
  applyFix: async (reportId, findingId) => {
    try { return (await apiWave1.verify.applyFix(reportId, findingId)) ?? null } catch { return null }
  },

  // ── comments & groups ──
  comments: {}, groups: {},
  loadComments: async (sessionId) => { try { const c = await apiWave1.review.comments(sessionId); set((s) => ({ comments: { ...s.comments, [sessionId]: c } })) } catch { /* keep local */ } },
  addComment: async (sessionId, c) => {
    const optimistic: ReviewComment = { ...c, id: uid('rc'), createdAt: Date.now(), author: 'user' }
    set((s) => ({ comments: { ...s.comments, [sessionId]: [...(s.comments[sessionId] ?? []), optimistic] } }))
    try {
      const saved = await apiWave1.review.addComment(sessionId, c)
      set((s) => ({ comments: { ...s.comments, [sessionId]: (s.comments[sessionId] ?? []).map((x) => (x.id === optimistic.id ? saved : x)) } }))
      return saved
    } catch { return optimistic }
  },
  resolveComment: async (sessionId, commentId, resolved = true) => {
    set((s) => ({ comments: { ...s.comments, [sessionId]: (s.comments[sessionId] ?? []).map((x) => (x.id === commentId ? { ...x, resolved } : x)) } }))
    try { await apiWave1.review.resolveComment(sessionId, commentId, resolved) } catch { /* local only */ }
  },
  deleteComment: async (sessionId, commentId) => {
    set((s) => ({ comments: { ...s.comments, [sessionId]: (s.comments[sessionId] ?? []).filter((x) => x.id !== commentId) } }))
    try { await apiWave1.review.deleteComment(sessionId, commentId) } catch { /* local only */ }
  },
  sendComments: async (sessionId, text) => {
    try { await apiWave1.review.sendComments(sessionId, text) }
    catch (e) {
      // Fallback: send as a normal prompt with the comments inlined.
      const open = (get().comments[sessionId] ?? []).filter((c) => !c.resolved)
      if (!open.length) throw e
      const body = open.map((c) => `- ${c.path.split('/').slice(-2).join('/')}:${c.line}${c.endLine && c.endLine !== c.line ? `-${c.endLine}` : ''} (${c.side}): ${c.text}`).join('\n')
      const { runCommand } = await import('@/lib/commands')
      await runCommand('chat.sendPrompt', { text: `${text ? text + '\n\n' : ''}Please address these review comments on your changes:\n${body}`, submit: true })
    }
    set((s) => ({ comments: { ...s.comments, [sessionId]: (s.comments[sessionId] ?? []).map((c) => ({ ...c, resolved: true })) } }))
  },
  loadGroups: async (sessionId, fallback) => {
    let groups: ReviewGroup[] | null = null
    try { groups = await apiWave1.review.groups(sessionId) } catch { groups = null }
    if (!groups || groups.length === 0) groups = fallback ? computeGroups(fallback) : []
    set((s) => ({ groups: { ...s.groups, [sessionId]: groups! } }))
    return groups
  },
  ingestAgentEvent: (e) => {
    if (e.type === 'verification') { get().applyVerifyEvent({ type: e.report.status === 'running' || e.report.status === 'pending' ? 'report' : 'done', report: e.report }, e.sessionId); if (e.report.status === 'running' || e.report.status === 'pending') set((s) => ({ verifying: { ...s.verifying, [e.sessionId]: true } })) }
    else if (e.type === 'review-comments') set((s) => ({ comments: { ...s.comments, [e.sessionId]: e.comments } }))
  },
}))

/** Latest verification report for a session (or null). */
export function useLatestReport(sessionId: string | null | undefined): VerificationReport | null {
  return useReview((s) => (sessionId ? s.reportsById[s.latestBySession[sessionId] ?? ''] ?? null : null))
}
export function useSessionComments(sessionId: string | null | undefined): ReviewComment[] {
  return useReview((s) => (sessionId ? s.comments[sessionId] ?? EMPTY_COMMENTS : EMPTY_COMMENTS))
}
const EMPTY_COMMENTS: ReviewComment[] = []

let subscribed = false
export function initReviewStore(): void {
  if (subscribed) return
  subscribed = true
  on(Events.reviewChanged, () => { useReview.getState().load().catch(() => {}) })
  on('caret://verify/changed', () => { useReview.getState().loadReports().catch(() => {}) })
}

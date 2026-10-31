import { create } from 'zustand'
import type { PtyBlockEvent, TerminalBlock } from '@/types'
import { reduceBlockState, type TerminalStatusInfo } from '@/lib/terminalBlocks'

/** Default tab names: "Terminal", then the smallest free "Terminal N" (so closing #2 and opening again reuses it). */
export function nextTerminalTitle(existing: string[]): string {
  const used = new Set(existing)
  if (!used.has('Terminal')) return 'Terminal'
  for (let n = 2; ; n++) { const t = `Terminal ${n}`; if (!used.has(t)) return t }
}

export interface TerminalSession {
  /** Local id (stable across the component lifecycle); `ptyId` is assigned after spawn. */
  id: string
  ptyId: string | null
  title: string
  customTitle?: string
  /** Title reported by the program via OSC 0/2 (shown as a hint, never replaces the tab name). */
  osTitle?: string
  cwd?: string
  shell?: string
  initialCommand?: string
  exitCode: number | null | undefined
  groupId: string
  createdAt: number
  /** Semantic command blocks mirrored from the backend (OSC 133). */
  blocks: TerminalBlock[]
  /** Explicit program status (OSC 7501) — undefined when idle. */
  status?: TerminalStatusInfo
  selectedBlockId?: string | null
}
export interface TerminalGroup { id: string; terminalIds: string[] }

interface TerminalStore {
  sessions: Record<string, TerminalSession>
  groups: TerminalGroup[]
  activeGroupId: string | null
  activeTerminalId: string | null
  cmdkFor: string | null
  findFor: string | null
  create: (opts?: { cwd?: string; shell?: string; command?: string; split?: boolean }) => TerminalSession
  remove: (id: string) => void
  update: (id: string, patch: Partial<TerminalSession>) => void
  setActive: (id: string) => void
  setActiveGroup: (groupId: string) => void
  /** Apply a block/status event from the PTY stream. */
  applyPtyEvent: (id: string, ev: PtyBlockEvent, maxBlocks?: number) => void
  selectBlock: (id: string, blockId: string | null) => void
  set: (patch: Partial<TerminalStore>) => void
}

let n = 0
const uid = (p: string) => `${p}${++n}-${Date.now().toString(36)}`

export const useTerminals = create<TerminalStore>((set, get) => ({
  sessions: {}, groups: [], activeGroupId: null, activeTerminalId: null, cmdkFor: null, findFor: null,
  create: (opts = {}) => {
    const s = get()
    const id = uid('t')
    let groupId: string
    let groups = s.groups.slice()
    if (opts.split && s.activeGroupId) {
      groupId = s.activeGroupId
      groups = groups.map((g) => (g.id === groupId ? { ...g, terminalIds: [...g.terminalIds, id] } : g))
    } else {
      groupId = uid('g')
      groups.push({ id: groupId, terminalIds: [id] })
    }
    const session: TerminalSession = { id, ptyId: null, title: nextTerminalTitle(Object.values(s.sessions).map((x) => x.customTitle ?? x.title)), cwd: opts.cwd, shell: opts.shell, initialCommand: opts.command, exitCode: undefined, groupId, createdAt: Date.now(), blocks: [] }
    set({ sessions: { ...s.sessions, [id]: session }, groups, activeGroupId: groupId, activeTerminalId: id })
    return session
  },
  remove: (id) => {
    const s = get()
    const sess = s.sessions[id]
    if (!sess) return
    const sessions = { ...s.sessions }
    delete sessions[id]
    const groups = s.groups.map((g) => (g.id === sess.groupId ? { ...g, terminalIds: g.terminalIds.filter((t) => t !== id) } : g)).filter((g) => g.terminalIds.length > 0)
    let activeGroupId = s.activeGroupId
    let activeTerminalId = s.activeTerminalId
    if (!groups.some((g) => g.id === activeGroupId)) activeGroupId = groups[groups.length - 1]?.id ?? null
    if (activeTerminalId === id) activeTerminalId = groups.find((g) => g.id === activeGroupId)?.terminalIds[0] ?? null
    set({ sessions, groups, activeGroupId, activeTerminalId })
  },
  update: (id, patch) => set((s) => (s.sessions[id] ? { sessions: { ...s.sessions, [id]: { ...s.sessions[id], ...patch } } } : {})),
  setActive: (id) => { const sess = get().sessions[id]; if (sess) set({ activeTerminalId: id, activeGroupId: sess.groupId }) },
  setActiveGroup: (groupId) => { const g = get().groups.find((x) => x.id === groupId); if (g) set({ activeGroupId: groupId, activeTerminalId: g.terminalIds[0] ?? null }) },
  applyPtyEvent: (id, ev, maxBlocks = 200) => set((s) => {
    const sess = s.sessions[id]
    if (!sess) return {}
    const next = reduceBlockState({ blocks: sess.blocks, status: sess.status }, ev, maxBlocks)
    if (ev.type === 'block-end') recordHistory(sess, ev)
    const selectedBlockId = ev.type === 'block-start' ? null : sess.selectedBlockId
    return { sessions: { ...s.sessions, [id]: { ...sess, blocks: next.blocks, status: next.status, selectedBlockId } } }
  }),
  selectBlock: (id, blockId) => set((s) => (s.sessions[id] && s.sessions[id].selectedBlockId !== blockId ? { sessions: { ...s.sessions, [id]: { ...s.sessions[id], selectedBlockId: blockId } } } : {})),
  set: (patch) => set(patch),
}))

/** Persist a finished command to the cross-session history (`~/.caret/terminal/history.jsonl`). Fire-and-forget; never throws. */
function recordHistory(sess: TerminalSession, ev: Extract<PtyBlockEvent, { type: 'block-end' }>): void {
  try {
    const block = sess.blocks.find((b) => b.id === ev.blockId)
    const command = block?.command?.trim()
    const ptyId = sess.ptyId
    if (!block || !command || !ptyId) return
    void (async () => {
      const [{ api }, { useWorkspace }] = await Promise.all([import('@/lib/api'), import('@/store/workspace')])
      let outputHead = ''
      try { outputHead = await api.pty.blockOutput(ptyId, block.id, 2000) } catch { /* output capture is optional */ }
      await api.terminalHistory.append({ id: block.id, ts: block.startedAt, cwd: block.cwd ?? sess.cwd ?? '', shell: sess.shell ?? '', command, exitCode: ev.exitCode, durationMs: Math.max(0, ev.endedAt - block.startedAt), outputHead, workspace: useWorkspace.getState().workspace?.root ?? null })
    })().catch(() => { /* history is best-effort */ })
  } catch { /* never throw from the reducer */ }
}

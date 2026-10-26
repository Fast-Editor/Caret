import { describe, it, expect } from 'vitest'
import { applyBlockEvent, reduceBlockState, effectiveStatus, summarizeStatus, neighborIndex, formatBlockText, blockContextItem, blockTitle, formatDuration } from '@/lib/terminalBlocks'
import type { TerminalBlock } from '@/types'

const mk = (i: number, extra: Partial<TerminalBlock> = {}): TerminalBlock => ({ id: `b${i}`, index: i, startedAt: 1000 + i, startLine: i * 3, running: false, ...extra })

describe('applyBlockEvent', () => {
  it('starts, labels, ends blocks in order and evicts beyond max', () => {
    let blocks = applyBlockEvent([], { type: 'block-start', block: mk(0, { running: true }) }, 2)
    expect(blocks).toHaveLength(1); expect(blocks[0]!.running).toBe(true)
    blocks = applyBlockEvent(blocks, { type: 'block-command', blockId: 'b0', command: 'ls' })
    expect(blocks[0]!.command).toBe('ls')
    blocks = applyBlockEvent(blocks, { type: 'block-end', blockId: 'b0', exitCode: 2, endedAt: 2000, endLine: 7 })
    expect(blocks[0]).toMatchObject({ running: false, exitCode: 2, endedAt: 2000, endLine: 7 })
    blocks = applyBlockEvent(blocks, { type: 'block-start', block: mk(1, { running: true }) }, 2)
    blocks = applyBlockEvent(blocks, { type: 'block-start', block: mk(2, { running: true }) }, 2)
    expect(blocks.map((b) => b.id)).toEqual(['b1', 'b2'])
    expect(blocks[0]!.running).toBe(false)
  })
  it('ignores events for unknown blocks', () => {
    const blocks = [mk(0)]
    expect(applyBlockEvent(blocks, { type: 'block-end', blockId: 'nope', exitCode: 0, endedAt: 1, endLine: 1 })).toEqual(blocks)
  })
})

describe('reduceBlockState / effectiveStatus', () => {
  it('explicit status is cleared by the next command start, working/waiting cleared on end', () => {
    let s = reduceBlockState({ blocks: [] }, { type: 'block-start', block: mk(0, { running: true }) }, 200, 5)
    s = reduceBlockState(s, { type: 'status', status: 'waiting', title: 'claude', detail: 'approve?' }, 200, 6)
    expect(s.status).toMatchObject({ status: 'waiting', title: 'claude', detail: 'approve?', at: 6 })
    expect(effectiveStatus(s.blocks, s.status)).toBe('waiting')
    s = reduceBlockState(s, { type: 'block-end', blockId: 'b0', exitCode: 0, endedAt: 7, endLine: 1 })
    expect(s.status).toBeUndefined()
    expect(effectiveStatus(s.blocks, s.status)).toBe('idle')
    s = reduceBlockState(s, { type: 'block-start', block: mk(1, { running: true }) })
    s = reduceBlockState(s, { type: 'status', status: 'done', title: 'cargo' })
    s = reduceBlockState(s, { type: 'block-end', blockId: 'b1', exitCode: 0, endedAt: 9, endLine: 2 })
    expect(s.status?.status).toBe('done')
    s = reduceBlockState(s, { type: 'block-start', block: mk(2, { running: true }) })
    expect(s.status).toBeUndefined()
    expect(effectiveStatus(s.blocks, s.status)).toBe('working')
    s = reduceBlockState(s, { type: 'status', status: 'idle' })
    expect(s.status).toBeUndefined()
  })
  it('derives failed from a non-zero exit and idle for exited terminals', () => {
    const blocks = [mk(0, { exitCode: 1 })]
    expect(effectiveStatus(blocks, undefined)).toBe('failed')
    expect(effectiveStatus(blocks, undefined, true)).toBe('idle')
    expect(effectiveStatus([mk(0, { exitCode: 0 })], undefined)).toBe('idle')
    expect(effectiveStatus([], undefined)).toBe('idle')
  })
})

describe('summarizeStatus / neighborIndex', () => {
  it('counts and phrases statuses', () => {
    const sum = summarizeStatus([
      { sessionId: 'a', ptyId: '1', title: 'zsh', status: 'waiting' },
      { sessionId: 'b', ptyId: '2', title: 'zsh', status: 'working' },
      { sessionId: 'c', ptyId: '3', title: 'zsh', status: 'working' },
      { sessionId: 'd', ptyId: null, title: 'zsh', status: 'idle' },
    ])
    expect(sum).toMatchObject({ waiting: 1, working: 2, failed: 0, text: '1 waiting for input · 2 running' })
    expect(summarizeStatus([]).text).toBe('')
  })
  it('navigates blocks with ⌘↑/⌘↓ semantics', () => {
    expect(neighborIndex([3, 1, 2], null, -1)).toBe(3)
    expect(neighborIndex([3, 1, 2], null, 1)).toBe(3)
    expect(neighborIndex([1, 2, 3], 2, -1)).toBe(1)
    expect(neighborIndex([1, 2, 3], 2, 1)).toBe(3)
    expect(neighborIndex([1, 2, 3], 3, 1)).toBe(3)
    expect(neighborIndex([1, 2, 3], 1, -1)).toBe(1)
    expect(neighborIndex([1, 2, 3], 9, 1)).toBe(3)
    expect(neighborIndex([], null, 1)).toBeNull()
  })
})

describe('formatting', () => {
  it('renders block text with cwd, command, exit and trimming', () => {
    const b = mk(0, { command: 'npm test', cwd: '/w', exitCode: 1, endedAt: 2500 })
    const text = formatBlockText(b, 'FAIL x\r\nerr\n\n')
    expect(text).toBe('# cwd: /w\n$ npm test\nFAIL x\nerr\n[exit 1]')
    const long = formatBlockText(mk(1, { command: 'x' }), 'a'.repeat(100), { maxChars: 30 })
    expect(long).toContain('chars omitted')
    expect(formatBlockText(mk(2, { running: true }), '')).toContain('[still running]')
  })
  it('builds a note context item that carries the output inline', () => {
    const item = blockContextItem(mk(0, { command: 'cargo build --release --all-features --workspace --locked' }), 'ok', 'pty-1', 'zsh')
    expect(item.kind).toBe('note'); expect(item.terminalId).toBe('pty-1')
    expect(item.label.startsWith('Terminal (zsh): $ cargo build')).toBe(false)
    expect(item.label).toMatch(/^Terminal \(zsh\): cargo build .*…$/)
    expect(item.data).toContain('$ cargo build'); expect(item.data).toContain('ok')
  })
  it('titles and durations', () => {
    expect(blockTitle(mk(0, { command: 'ls', exitCode: 0, startedAt: 0, endedAt: 1500 }))).toBe('$ ls — exit 0 in 1.5s')
    expect(blockTitle(mk(0, { running: true }))).toBe('command — running')
    expect(formatDuration(250)).toBe('250ms'); expect(formatDuration(65_000)).toBe('1m 5s'); expect(formatDuration(12_000)).toBe('12s')
  })
})

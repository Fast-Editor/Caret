import { describe, it, expect, vi } from 'vitest'
import { dirtyTabTitles, resolveWindowClose, type CloseChoice, type WindowCloseDeps } from './windowClose'

function harness(dirty: string[], choice: CloseChoice = 'cancel', saveOk = true) {
  const calls: string[] = []
  const deps: WindowCloseDeps = {
    dirtyTitles: () => dirty,
    confirm: vi.fn(async () => { calls.push('confirm'); return choice }),
    saveAll: vi.fn(async () => { calls.push('save'); return saveOk }),
    disposeTerminals: vi.fn(() => { calls.push('terminals') }),
    destroy: vi.fn(() => { calls.push('destroy') }),
  }
  return { deps, calls }
}

describe('resolveWindowClose', () => {
  it('closes straight away when nothing is dirty (terminals first, no prompt)', async () => {
    const { deps, calls } = harness([])
    expect(await resolveWindowClose(deps)).toBe('closed')
    expect(calls).toEqual(['terminals', 'destroy'])
    expect(deps.confirm).not.toHaveBeenCalled()
    expect(deps.saveAll).not.toHaveBeenCalled()
  })
  it('dirty + Save: saves, then closes', async () => {
    const { deps, calls } = harness(['a.ts', 'b.ts'], 'save')
    expect(await resolveWindowClose(deps)).toBe('closed')
    expect(deps.confirm).toHaveBeenCalledWith(['a.ts', 'b.ts'])
    expect(calls).toEqual(['confirm', 'save', 'terminals', 'destroy'])
  })
  it('dirty + Save with a failed or cancelled save keeps the window', async () => {
    const { deps, calls } = harness(['a.ts'], 'save', false)
    expect(await resolveWindowClose(deps)).toBe('kept')
    expect(calls).toEqual(['confirm', 'save'])
    expect(deps.destroy).not.toHaveBeenCalled()
  })
  it("dirty + Don't Save: closes without saving", async () => {
    const { deps, calls } = harness(['a.ts'], 'discard')
    expect(await resolveWindowClose(deps)).toBe('closed')
    expect(calls).toEqual(['confirm', 'terminals', 'destroy'])
    expect(deps.saveAll).not.toHaveBeenCalled()
  })
  it('dirty + Cancel: stays, touching nothing', async () => {
    const { deps, calls } = harness(['a.ts'], 'cancel')
    expect(await resolveWindowClose(deps)).toBe('kept')
    expect(calls).toEqual(['confirm'])
    expect(deps.destroy).not.toHaveBeenCalled()
    expect(deps.disposeTerminals).not.toHaveBeenCalled()
  })
  it('a failing terminal cleanup does not block the close', async () => {
    const { deps } = harness([])
    deps.disposeTerminals = vi.fn(() => { throw new Error('pty gone') })
    expect(await resolveWindowClose(deps)).toBe('closed')
    expect(deps.destroy).toHaveBeenCalledTimes(1)
  })
  it('works without a terminal cleanup hook', async () => {
    const { deps } = harness([])
    delete deps.disposeTerminals
    expect(await resolveWindowClose(deps)).toBe('closed')
  })
})

describe('dirtyTabTitles', () => {
  it('lists dirty tabs only, once per file across groups', () => {
    const groups = [
      { tabs: [{ id: 't1', title: 'a.ts', dirty: true, path: '/r/a.ts' }, { id: 't2', title: 'b.ts', dirty: false, path: '/r/b.ts' }] },
      { tabs: [{ id: 't3', title: 'a.ts', dirty: true, path: '/r/a.ts' }, { id: 't4', title: 'Untitled-1', dirty: true }] },
    ]
    expect(dirtyTabTitles(groups)).toEqual(['a.ts', 'Untitled-1'])
    expect(dirtyTabTitles([{ tabs: [] }])).toEqual([])
  })
})

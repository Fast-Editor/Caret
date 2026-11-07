import { describe, it, expect } from 'vitest'
import { fmtUsd, fmtPct, dayLabel, fmtResetIn, windowTone, barHeights, bucketDaily, csvFilename, priceRowsFromMap, priceMapFromRows, pricePatch, emptyPriceRow, RANGES, GROUPS } from './usageFormat'

describe('usage formatting', () => {
  it('formats dollars with sub-cent precision and grouping', () => {
    expect(fmtUsd(null)).toBe('--')
    expect(fmtUsd(undefined)).toBe('--')
    expect(fmtUsd(0)).toBe('$0.00')
    expect(fmtUsd(0.0042)).toBe('$0.0042')
    expect(fmtUsd(0.42)).toBe('$0.42')
    expect(fmtUsd(1234.5)).toBe('$1,234.50')
  })
  it('formats cache hit ratios', () => {
    expect(fmtPct(0)).toBe('0%')
    expect(fmtPct(0.004)).toBe('<1%')
    expect(fmtPct(0.126)).toBe('13%')
    expect(fmtPct(1)).toBe('100%')
  })
  it('labels days and reset countdowns', () => {
    expect(dayLabel('2026-03-10')).toBe('Mar 10')
    expect(dayLabel('2026-12-01')).toBe('Dec 1')
    expect(dayLabel('weird')).toBe('weird')
    const now = 1_000_000_000_000
    expect(fmtResetIn(undefined, now)).toBe('')
    expect(fmtResetIn(now - 1, now)).toBe('resets soon')
    expect(fmtResetIn(now + 5 * 60_000, now)).toBe('resets in 5 min')
    expect(fmtResetIn(now + (2 * 60 + 5) * 60_000, now)).toBe('resets in 2h 05m')
    expect(fmtResetIn(now + (3 * 24 + 4) * 3_600_000, now)).toBe('resets in 3d 4h')
    expect(windowTone(10)).toBe('success')
    expect(windowTone(75)).toBe('warning')
    expect(windowTone(95)).toBe('danger')
  })
  it('exposes the range and group selectors in order', () => {
    expect(RANGES.map((r) => r.id)).toEqual(['today', '7d', '30d', 'all'])
    expect(GROUPS.map((g) => g.id)).toEqual(['model', 'provider', 'agent', 'source', 'day', 'workspace'])
  })
  it('names the CSV after the range and local date', () => {
    expect(csvFilename('30d', new Date(2026, 2, 5))).toBe('caret-usage-30d-2026-03-05.csv')
  })
})

describe('usage chart scaling', () => {
  const pts = (...costs: number[]) => costs.map((c, i) => ({ day: `2026-03-${String(i + 1).padStart(2, '0')}`, costUsd: c, tokens: c * 1000 }))
  it('scales bars to the tallest day', () => {
    expect(barHeights(pts(0, 1, 4))).toEqual([0, 25, 100])
    expect(barHeights(pts(0, 0))).toEqual([0, 0])
    expect(barHeights([])).toEqual([])
  })
  it('buckets long series without losing totals', () => {
    const p = pts(1, 2, 3, 4, 5, 6, 7)
    expect(bucketDaily(p, 10)).toBe(p)
    const b = bucketDaily(p, 3)
    expect(b.map((x) => x.day)).toEqual(['2026-03-01', '2026-03-04', '2026-03-07'])
    expect(b.map((x) => x.costUsd)).toEqual([6, 15, 7])
    expect(b.reduce((a, x) => a + x.tokens, 0)).toBe(28_000)
    expect(bucketDaily(p, 0)).toBe(p)
  })
})

describe('custom price editor', () => {
  it('round-trips rows and maps, dropping blank optional prices', () => {
    const rows = priceRowsFromMap({ 'zeta-1': { input: 1, output: 2 }, 'alpha-9': { input: 0.5, output: 1.5, cacheRead: 0.05, cacheWrite: 0.6 } })
    expect(rows.map((r) => r.id)).toEqual(['alpha-9', 'zeta-1'])
    expect(rows[0]).toEqual({ id: 'alpha-9', input: '0.5', output: '1.5', cacheRead: '0.05', cacheWrite: '0.6' })
    expect(rows[1].cacheRead).toBe('')
    const back = priceMapFromRows([...rows, emptyPriceRow()])
    expect(back.errors).toEqual([])
    expect(back.prices).toEqual({ 'zeta-1': { input: 1, output: 2 }, 'alpha-9': { input: 0.5, output: 1.5, cacheRead: 0.05, cacheWrite: 0.6 } })
  })
  it('reports validation problems per row', () => {
    const r = priceMapFromRows([
      { id: '', input: '1', output: '2', cacheRead: '', cacheWrite: '' },
      { id: 'm', input: 'abc', output: '2', cacheRead: '', cacheWrite: '' },
      { id: 'n', input: '1', output: '', cacheRead: '-1', cacheWrite: '' },
      { id: 'ok', input: '1', output: '2', cacheRead: '', cacheWrite: '' },
      { id: 'ok', input: '3', output: '4', cacheRead: '', cacheWrite: '' },
    ])
    expect(r.errors).toEqual(['A row is missing its model id', 'm: input price must be a number >= 0', 'n: output price is required', 'n: cache read price must be a number >= 0', 'ok: listed twice'])
    expect(Object.keys(r.prices)).toEqual(['ok'])
    expect(r.prices.ok).toEqual({ input: 1, output: 2 })
  })
  it('builds a merge patch that nulls removed ids and skips unchanged ones', () => {
    const prev = { a: { input: 1, output: 2 }, b: { input: 3, output: 4, cacheRead: 0.3 }, c: { input: 5, output: 6 } }
    const next = { a: { input: 1, output: 2 }, b: { input: 3, output: 4 }, d: { input: 7, output: 8 } }
    expect(pricePatch(prev, next)).toEqual({ c: null, b: { input: 3, output: 4 }, d: { input: 7, output: 8 } })
    expect(pricePatch({}, {})).toEqual({})
  })
})

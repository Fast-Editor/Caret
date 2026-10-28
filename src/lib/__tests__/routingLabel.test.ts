import { describe, expect, it } from 'vitest'
import { modelLabel, routingLabel, routingTooltip } from '../routingLabel'

describe('routingLabel', () => {
  it('formats served model and tier', () => {
    expect(routingLabel({ via: 'auto', servedModelId: 'claude-sonnet-5-5', servedTier: 'COMPLEX', fallback: false }, 'Claude Sonnet 5.5')).toBe('Auto → Claude Sonnet 5.5 · COMPLEX')
  })
  it('falls back to the id and marks fallbacks', () => {
    expect(routingLabel({ via: 'auto', servedModelId: 'x', requestedTier: 'SIMPLE', fallback: true })).toBe('Auto → x · SIMPLE ↷')
    expect(routingLabel({ via: 'auto', servedModelId: 'x', fallback: false })).toBe('Auto → x')
  })
  it('tooltip explains tier changes and score', () => {
    const t = routingTooltip({ via: 'auto', servedModelId: 'sonnet', requestedTier: 'SIMPLE', servedTier: 'MEDIUM', score: 33.4, method: 'native-passthrough-stream', fallback: true })
    expect(t).toContain('requested SIMPLE, served MEDIUM'); expect(t).toContain('complexity 33'); expect(t).toContain('fell back')
  })
  it('modelLabel prefers display names and humanises agent ids', () => {
    expect(modelLabel('claude-opus-5-5', 'Claude Opus 5.5')).toBe('Claude Opus 5.5')
    expect(modelLabel('acp:claude-code')).toBe('Claude Code')
    expect(modelLabel('gpt-5')).toBe('gpt-5'); expect(modelLabel(undefined)).toBeUndefined()
  })
})

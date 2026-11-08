import { describe, it, expect } from 'vitest'
import { maskToken } from './orchestrator'

describe('maskToken', () => {
  const token = 'abcdef0123456789abcdef0123456789'
  it('masks every occurrence but keeps a recognisable prefix', () => {
    const out = maskToken(`export CARET_MCP_TOKEN=${token}\nBearer ${token}`, token, false)
    expect(out).not.toContain(token)
    expect(out.match(/abcdef\u2022{12}/g)).toHaveLength(2)
  })
  it('returns the text untouched when showing or when there is no token', () => {
    expect(maskToken(`x ${token}`, token, true)).toBe(`x ${token}`)
    expect(maskToken('plain', '', false)).toBe('plain')
  })
})

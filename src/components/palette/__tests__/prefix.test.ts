import { describe, it, expect } from 'vitest'
import { stripPalettePrefix } from '../parse'

describe('stripPalettePrefix', () => {
  it('drops the mode prefix and the whitespace after it', () => {
    expect(stripPalettePrefix('>')).toBe('')
    expect(stripPalettePrefix('> toggle')).toBe('toggle')
    expect(stripPalettePrefix('@sym')).toBe('sym')
    expect(stripPalettePrefix('#needle')).toBe('needle')
  })
  it('leaves plain queries alone', () => {
    expect(stripPalettePrefix('open file')).toBe('open file')
    expect(stripPalettePrefix('a>b')).toBe('a>b')
  })
})

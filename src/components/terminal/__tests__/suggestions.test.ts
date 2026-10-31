import { describe, it, expect } from 'vitest'
import { pickHistorySuggestion, cleanAiSuggestion } from '../suggestions'

describe('pickHistorySuggestion', () => {
  it('returns the remainder of the first longer match, most recent first', () => {
    expect(pickHistorySuggestion('git ch', ['git checkout main', 'git status', 'git checkout -b x'])).toBe('eckout main')
    expect(pickHistorySuggestion('npm', ['npm', 'npm run build'])).toBe(' run build')
  })
  it('never suggests the prefix itself or anything for blank input', () => {
    expect(pickHistorySuggestion('ls', ['ls'])).toBeNull()
    expect(pickHistorySuggestion('   ', ['ls -la'])).toBeNull()
    expect(pickHistorySuggestion('cargo', ['npm test'])).toBeNull()
  })
})

describe('cleanAiSuggestion', () => {
  it('keeps one line, strips an echoed prefix and trailing whitespace', () => {
    expect(cleanAiSuggestion('git ch', 'git checkout main\n# switches branch')).toBe('eckout main')
    expect(cleanAiSuggestion('ls', ' -la  \n')).toBe(' -la')
    expect(cleanAiSuggestion('x', '')).toBeNull()
    expect(cleanAiSuggestion('x', null)).toBeNull()
  })
})

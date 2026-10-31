import { describe, it, expect } from 'vitest'
import { isNaturalLanguage, queryTokens, matchIndices, timeAgo, relativeCwd, exitBadge } from '../terminalHistory'

describe('terminal history helpers', () => {
  it('treats 3+ words as natural language', () => {
    expect(isNaturalLanguage('docker')).toBe(false)
    expect(isNaturalLanguage('docker ps')).toBe(false)
    expect(isNaturalLanguage('  the docker  thing ')).toBe(true)
    expect(isNaturalLanguage('')).toBe(false)
  })

  it('tokenises queries like the backend', () => {
    expect(queryTokens('Where did I run Docker?')).toEqual(['where', 'did', 'i', 'run', 'docker'])
    expect(queryTokens('"npm" npm,  TEST.')).toEqual(['npm', 'test'])
    expect(queryTokens('./run.sh --flag')).toEqual(['./run.sh', '--flag'])
    expect(queryTokens('')).toEqual([])
  })

  it('computes highlight indices for every occurrence', () => {
    expect(matchIndices('npm test && npm run build', ['npm'])).toEqual([0, 1, 2, 12, 13, 14])
    expect(matchIndices('Cargo Test', ['test'])).toEqual([6, 7, 8, 9])
    expect(matchIndices('ls', ['zzz', ''])).toEqual([])
  })

  it('formats relative time', () => {
    const now = 1_700_000_000_000
    expect(timeAgo(now - 5_000, now)).toBe('just now')
    expect(timeAgo(now - 5 * 60_000, now)).toBe('5m ago')
    expect(timeAgo(now - 3 * 3_600_000, now)).toBe('3h ago')
    expect(timeAgo(now - 2 * 86_400_000, now)).toBe('2d ago')
    expect(timeAgo(now - 45 * 86_400_000, now)).toBe('1mo ago')
    expect(timeAgo(now - 400 * 86_400_000, now)).toBe('1y ago')
    expect(timeAgo(now + 60_000, now)).toBe('just now')
  })

  it('shortens cwd against the workspace root, then home', () => {
    expect(relativeCwd('/w/caret/src', '/w/caret')).toBe('src')
    expect(relativeCwd('/w/caret', '/w/caret/')).toBe('.')
    expect(relativeCwd('/w/caret-other/src', '/w/caret')).toBe('/w/caret-other/src')
    expect(relativeCwd('/Users/me/proj', null, '/Users/me')).toBe('~/proj')
    expect(relativeCwd('/Users/me/proj', null)).toBe('~/proj')
    expect(relativeCwd('/home/me', null)).toBe('~')
    expect(relativeCwd('/opt/x', null)).toBe('/opt/x')
    expect(relativeCwd('', '/w')).toBe('')
  })

  it('renders an exit badge only for failures', () => {
    expect(exitBadge({ exitCode: 1 })).toBe('exit 1')
    expect(exitBadge({ exitCode: 130 })).toBe('exit 130')
    expect(exitBadge({ exitCode: 0 })).toBeNull()
    expect(exitBadge({ exitCode: null })).toBeNull()
    expect(exitBadge({})).toBeNull()
  })
})

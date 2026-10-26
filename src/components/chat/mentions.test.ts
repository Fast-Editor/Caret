import { describe, expect, it } from 'vitest'
import { detectTrigger, parseSlashCommand, replaceTrigger } from './mentions'

describe('detectTrigger', () => {
  it('detects @ at start and after whitespace', () => {
    expect(detectTrigger('@src', 4)).toEqual({ kind: '@', query: 'src', start: 0, end: 4 })
    expect(detectTrigger('fix @util', 9)).toEqual({ kind: '@', query: 'util', start: 4, end: 9 })
  })
  it('does not trigger inside words or emails', () => {
    expect(detectTrigger('mail me@host', 12)).toBeNull()
  })
  it('stops at whitespace between trigger and caret', () => {
    expect(detectTrigger('@src foo', 8)).toBeNull()
  })
  it('only triggers slash at the start of the input', () => {
    expect(detectTrigger('/pl', 3)).toEqual({ kind: '/', query: 'pl', start: 0, end: 3 })
    expect(detectTrigger('do /pl', 6)).toBeNull()
  })
  it('handles caret in the middle of text', () => {
    expect(detectTrigger('a @fi more', 5)).toEqual({ kind: '@', query: 'fi', start: 2, end: 5 })
  })
})

describe('replaceTrigger', () => {
  it('replaces the trigger span and positions the caret', () => {
    const t = detectTrigger('see @abc now', 8)!
    expect(replaceTrigger('see @abc now', t, '')).toEqual({ text: 'see  now', caret: 4 })
    expect(replaceTrigger('/pl', detectTrigger('/pl', 3)!, '/plan ')).toEqual({ text: '/plan ', caret: 6 })
  })
})

describe('parseSlashCommand', () => {
  it('parses name and args', () => {
    expect(parseSlashCommand('/agent')).toEqual({ name: 'agent', args: '' })
    expect(parseSlashCommand('/deploy staging --fast')).toEqual({ name: 'deploy', args: 'staging --fast' })
    expect(parseSlashCommand('not a command')).toBeNull()
  })
})

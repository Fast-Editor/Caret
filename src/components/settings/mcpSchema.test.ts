import { describe, it, expect } from 'vitest'
import { skeletonFromSchema, parseArgs } from './mcpSchema'

describe('skeletonFromSchema', () => {
  it('fills example values by type, default and enum', () => {
    expect(skeletonFromSchema({ type: 'object', properties: { q: { type: 'string' }, n: { type: 'integer' }, b: { type: 'boolean' }, arr: { type: 'array' }, o: { type: 'object' }, d: { type: 'string', default: 'x' }, e: { type: 'string', enum: ['one', 'two'] }, u: { type: ['null', 'string'] } } }))
      .toEqual({ q: '', n: 0, b: false, arr: [], o: {}, d: 'x', e: 'one', u: '' })
  })
  it('handles missing schemas', () => { expect(skeletonFromSchema(undefined)).toEqual({}) })
})

describe('parseArgs', () => {
  it('splits on whitespace and honours quotes', () => {
    expect(parseArgs('-y @modelcontextprotocol/server-github')).toEqual(['-y', '@modelcontextprotocol/server-github'])
    expect(parseArgs(`--path "/Users/me/My Dir" --flag 'a b'`)).toEqual(['--path', '/Users/me/My Dir', '--flag', 'a b'])
    expect(parseArgs('   ')).toEqual([])
  })
})

import { describe, it, expect } from 'vitest'
import { orderFiles, orderHunks, classifyHunk } from '../ordering'

describe('ordering', () => {
  it('orders files types → implementation → tests → docs → config → lock', () => {
    const files = ['package-lock.json', 'README.md', 'src/pricing.test.ts', 'src/pricing.ts', 'src/types.ts', 'package.json'].map((path) => ({ path }))
    expect(orderFiles(files).map((f) => f.path)).toEqual(['src/types.ts', 'src/pricing.ts', 'src/pricing.test.ts', 'README.md', 'package.json', 'package-lock.json'])
  })
  it('orders hunks definitions before functions before tests', () => {
    const hunks = [
      { index: 0, startLine: 40, endLine: 45, added: 6, deleted: 0, lines: ["describe('x', () => {", 'it(…)'] },
      { index: 1, startLine: 20, endLine: 25, added: 6, deleted: 0, lines: ['function helper(a) {', '  return a'] },
      { index: 2, startLine: 1, endLine: 5, added: 5, deleted: 0, lines: ['export interface CartItem {', '  name: string'] },
      { index: 3, startLine: 30, endLine: 32, added: 3, deleted: 1, lines: ['  total += x', '  foo()'] },
    ]
    expect(orderHunks(hunks, (h) => h.lines).map((h) => h.index)).toEqual([2, 1, 3, 0])
    expect(classifyHunk(['pub fn compute() -> u32 {'])).toBe('exports')
    expect(classifyHunk(['#[test]', 'fn it_works() {'])).toBe('tests')
    expect(classifyHunk(['def test_mean():'])).toBe('tests')
  })
})

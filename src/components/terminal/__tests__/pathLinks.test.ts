import { describe, it, expect } from 'vitest'
import { findPathCandidates, resolveCandidatePaths, normalizePath } from '../pathLinks'

describe('findPathCandidates', () => {
  it('finds git-style indented file names and nested paths', () => {
    expect(findPathCandidates('\tpackage-lock.json').map((c) => c.path)).toEqual(['package-lock.json'])
    expect(findPathCandidates('        src/server.js').map((c) => c.path)).toEqual(['src/server.js'])
  })
  it('parses :line:col and (line,col) suffixes', () => {
    const [c] = findPathCandidates('error at src/server.js:12:5 near token')
    expect(c).toMatchObject({ path: 'src/server.js', line: 12, column: 5, text: 'src/server.js:12:5' })
    const [d] = findPathCandidates('src/app.ts(7,3): error TS2304')
    expect(d).toMatchObject({ path: 'src/app.ts', line: 7, column: 3 })
  })
  it('ignores prose, versions, urls and trailing punctuation', () => {
    expect(findPathCandidates('error: Your local changes to the following files would be overwritten by checkout:')).toEqual([])
    expect(findPathCandidates('node v20.11.1 and npm 10.2.4')).toEqual([])
    expect(findPathCandidates('see https://example.com/docs/index.html for help')).toEqual([])
    expect(findPathCandidates('Wrote ./dist/bundle.js.').map((c) => c.path)).toEqual(['./dist/bundle.js'])
  })
  it('keeps dotfiles and absolute paths', () => {
    expect(findPathCandidates('.env .gitignore /etc/hosts ~/notes/todo.md').map((c) => c.path)).toEqual(['.env', '.gitignore', '/etc/hosts', '~/notes/todo.md'])
  })
})

describe('resolveCandidatePaths', () => {
  it('tries cwd then workspace root for relative paths', () => {
    expect(resolveCandidatePaths('src/a.ts', '/w/sub', '/w')).toEqual(['/w/sub/src/a.ts', '/w/src/a.ts'])
    expect(resolveCandidatePaths('../x.md', '/w/sub', '/w')).toEqual(['/w/x.md', '/x.md'])
    expect(resolveCandidatePaths('/abs/f.rs', '/w', '/w')).toEqual(['/abs/f.rs'])
    expect(resolveCandidatePaths('~/f.rs', '/w', '/w', '/Users/me')).toEqual(['/Users/me/f.rs'])
    expect(resolveCandidatePaths('~/f.rs', '/w', '/w')).toEqual([])
  })
  it('normalises dot segments', () => { expect(normalizePath('/a/./b/../c')).toBe('/a/c') })
})

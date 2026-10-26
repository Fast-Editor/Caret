import { describe, it, expect } from 'vitest'
import { languageForPath, lspLanguageId, lineCommentPrefix, isImagePath } from '../languages'

describe('languageForPath', () => {
  it('maps common extensions', () => {
    expect(languageForPath('/a/b.ts')).toBe('typescript')
    expect(languageForPath('/a/b.tsx')).toBe('typescript')
    expect(languageForPath('/a/b.rs')).toBe('rust')
    expect(languageForPath('/a/b.py')).toBe('python')
    expect(languageForPath('/a/Dockerfile')).toBe('dockerfile')
    expect(languageForPath('/a/Makefile')).toBe('makefile')
    expect(languageForPath('/a/.env.local')).toBe('ini')
    expect(languageForPath('/a/Cargo.toml')).toBe('ini')
    expect(languageForPath('/a/x.svelte')).toBe('html')
    expect(languageForPath('/a/x.unknownext')).toBe('plaintext')
    expect(languageForPath('/a/config.yaml.in')).toBe('yaml')
    expect(languageForPath('/a/README')).toBe('markdown')
  })
  it('maps lsp ids', () => {
    expect(lspLanguageId('typescript', '/a.tsx')).toBe('typescriptreact')
    expect(lspLanguageId('shell')).toBe('shellscript')
    expect(lspLanguageId('rust')).toBe('rust')
  })
  it('comment prefixes and images', () => {
    expect(lineCommentPrefix('python')).toBe('#'); expect(lineCommentPrefix('typescript')).toBe('//'); expect(lineCommentPrefix('html')).toBeNull()
    expect(isImagePath('/x/y.PNG')).toBe(true); expect(isImagePath('/x/y.ts')).toBe(false)
  })
})

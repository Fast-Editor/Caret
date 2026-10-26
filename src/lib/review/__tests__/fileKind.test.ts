import { describe, it, expect } from 'vitest'
import { fileKind, isTestPath, isTypesPath } from '../fileKind'

describe('fileKind', () => {
  it('classifies tests across languages', () => {
    for (const p of ['src/foo.test.ts', 'src/__tests__/bar.tsx', 'tests/pricing.test.mjs', 'pkg/util_test.go', 'tests/test_utils.py', 'spec/models/user_spec.rb', 'src/lib/a.spec.js', 'conftest.py', 'cypress/e2e/login.cy.ts']) expect(fileKind(p), p).toBe('test')
    expect(isTestPath('src/testing-helpers.ts')).toBe(false)
  })
  it('classifies types, docs, config, locks and source', () => {
    expect(fileKind('src/types.ts')).toBe('types')
    expect(fileKind('src/api/schema.graphql')).toBe('types')
    expect(fileKind('types/global.d.ts')).toBe('types')
    expect(isTypesPath('src/models/user.ts')).toBe(true)
    expect(fileKind('README.md')).toBe('doc')
    expect(fileKind('docs/guide.mdx')).toBe('doc')
    expect(fileKind('package.json')).toBe('config')
    expect(fileKind('.github/workflows/ci.yml')).toBe('config')
    expect(fileKind('Cargo.toml')).toBe('config')
    expect(fileKind('package-lock.json')).toBe('lock')
    expect(fileKind('Cargo.lock')).toBe('lock')
    expect(fileKind('src/pricing.ts')).toBe('source')
    expect(fileKind('src-tauri/src/main.rs')).toBe('source')
    expect(fileKind('assets/logo.png')).toBe('other')
  })
})

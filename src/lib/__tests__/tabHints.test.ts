import { describe, expect, it } from 'vitest'
import { tabDirHint } from '../tabHints'

describe('tabDirHint', () => {
  it('shows the parent directory only when the basename is duplicated', () => {
    expect(tabDirHint('/repo/skills/hint/SKILL.md', 2)).toBe('…/hint')
    expect(tabDirHint('/repo/skills/course/SKILL.md', 3)).toBe('…/course')
    expect(tabDirHint('/repo/skills/hint/SKILL.md', 1)).toBeNull()
  })
  it('uses the workspace-relative path and copes with root-level files', () => {
    expect(tabDirHint('/repo/src/index.ts', 2, (p) => p.replace('/repo/', ''))).toBe('…/src')
    expect(tabDirHint('/repo/README.md', 2, (p) => p.replace('/repo/', ''))).toBeNull()
    expect(tabDirHint(undefined, 2)).toBeNull()
  })
})

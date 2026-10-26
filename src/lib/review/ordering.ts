/** Logical ordering for files and hunks: definitions before uses, implementation before tests/docs/config. */
import { fileKind, KIND_RANK, type FileKind } from './fileKind'

export interface Orderable { path: string }

export function compareByKindThenPath(a: string, b: string): number {
  const ka = KIND_RANK[fileKind(a)], kb = KIND_RANK[fileKind(b)]
  if (ka !== kb) return ka - kb
  // shallower paths first, then alphabetical
  const da = a.split('/').length, db = b.split('/').length
  if (da !== db) return da - db
  return a.localeCompare(b)
}

export function orderFiles<T extends Orderable>(items: T[]): T[] { return [...items].sort((a, b) => compareByKindThenPath(a.path, b.path)) }

export interface HunkLike { index: number; startLine: number; endLine: number; added: number; deleted: number }
export type HunkClass = 'types' | 'exports' | 'functions' | 'tests' | 'other'

const TYPE_RE = /^\s*(export\s+)?(declare\s+)?(interface|type|enum|struct|trait|protocol|typedef|schema|record|data\s+class|sealed\s+class|abstract\s+class|class\s+\w+\s*(<[^>]*>)?\s*(implements|extends)?)\b/
const EXPORT_RE = /^\s*(export\s+(default\s+)?|pub(\(crate\))?\s+|public\s+|module\.exports|__all__)/
const FUNC_RE = /^\s*(export\s+)?(default\s+)?(async\s+)?(function\b|fn\b|def\b|func\b|proc\b|sub\b|static\s+|private\s+|protected\s+|public\s+|const\s+\w+\s*=\s*(async\s*)?\(|let\s+\w+\s*=\s*(async\s*)?\(|\w+\s*\([^)]*\)\s*(:\s*[\w<>\[\], ]+)?\s*\{)/
const TEST_RE = /^\s*(?:(?:describe|it|test|context|suite|it\.each|test\.each|it\.only|test\.only|xit|xtest)\s*\(|#\[(?:test|cfg\(test\))\]|def test_|func Test|@Test\b|#\[tokio::test\])/

/** Classify a hunk by scanning its first non-empty modified lines. */
export function classifyHunk(lines: string[]): HunkClass {
  const head = lines.filter((l) => l.trim().length > 0).slice(0, 12)
  if (head.some((l) => TEST_RE.test(l))) return 'tests'
  if (head.some((l) => TYPE_RE.test(l))) return 'types'
  if (head.some((l) => EXPORT_RE.test(l))) return 'exports'
  if (head.some((l) => FUNC_RE.test(l))) return 'functions'
  return 'other'
}
const HUNK_RANK: Record<HunkClass, number> = { types: 0, exports: 1, functions: 2, other: 3, tests: 4 }

/**
 * Order hunks logically: type/interface definitions → exported symbols → functions → other → tests,
 * ties broken by position. `getLines(h)` returns the modified-side text of a hunk.
 */
export function orderHunks<T extends HunkLike>(hunks: T[], getLines: (h: T) => string[]): T[] {
  const scored = hunks.map((h) => ({ h, cls: classifyHunk(getLines(h)) }))
  return scored.sort((a, b) => (HUNK_RANK[a.cls] - HUNK_RANK[b.cls]) || (a.h.startLine - b.h.startLine)).map((x) => x.h)
}

export function kindOf(path: string): FileKind { return fileKind(path) }

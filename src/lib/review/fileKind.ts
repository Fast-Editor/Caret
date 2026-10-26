/** Classify a path into a review bucket. Pure; used for grouping, ordering and collapse-by-default. */
export type FileKind = 'types' | 'source' | 'test' | 'doc' | 'config' | 'lock' | 'other'

const LOCK_FILES = new Set(['package-lock.json', 'yarn.lock', 'pnpm-lock.yaml', 'bun.lockb', 'bun.lock', 'cargo.lock', 'poetry.lock', 'pipfile.lock', 'gemfile.lock', 'composer.lock', 'go.sum', 'flake.lock', 'uv.lock', 'pubspec.lock', 'mix.lock', 'packages.lock.json'])
const CONFIG_FILES = new Set(['package.json', 'tsconfig.json', 'jsconfig.json', 'cargo.toml', 'pyproject.toml', 'setup.py', 'setup.cfg', 'requirements.txt', 'go.mod', 'gemfile', 'makefile', 'dockerfile', 'docker-compose.yml', 'docker-compose.yaml', 'procfile', '.gitignore', '.gitattributes', '.editorconfig', '.npmrc', '.nvmrc', '.prettierrc', '.eslintrc', 'eslint.config.js', 'eslint.config.mjs', 'vite.config.ts', 'vitest.config.ts', 'tauri.conf.json', 'babel.config.js', 'webpack.config.js', 'rollup.config.js', 'jest.config.js', 'jest.config.ts', 'tailwind.config.js', 'tailwind.config.ts', 'postcss.config.js', 'biome.json', 'deno.json', 'bunfig.toml'])
const CONFIG_EXT = new Set(['toml', 'ini', 'cfg', 'conf', 'env', 'properties', 'yml', 'yaml', 'json', 'json5', 'jsonc', 'xml', 'plist', 'nix', 'tf', 'tfvars', 'hcl'])
const DOC_EXT = new Set(['md', 'mdx', 'markdown', 'rst', 'txt', 'adoc', 'asciidoc', 'org', 'tex', 'rtf'])
const SOURCE_EXT = new Set(['ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs', 'mts', 'cts', 'rs', 'py', 'go', 'java', 'kt', 'kts', 'swift', 'rb', 'php', 'c', 'h', 'cc', 'cpp', 'hpp', 'cxx', 'cs', 'scala', 'dart', 'ex', 'exs', 'erl', 'hs', 'ml', 'clj', 'cljs', 'lua', 'zig', 'nim', 'vue', 'svelte', 'astro', 'sh', 'bash', 'zsh', 'fish', 'ps1', 'sql', 'graphql', 'gql', 'proto', 'css', 'scss', 'less', 'html', 'htm'])

export function fileName(path: string): string { return path.split('/').filter(Boolean).pop() ?? path }
export function fileExt(path: string): string { const n = fileName(path); const i = n.lastIndexOf('.'); return i > 0 ? n.slice(i + 1).toLowerCase() : '' }

export function isTestPath(path: string): boolean {
  const p = path.replace(/\\/g, '/').toLowerCase()
  const n = fileName(p)
  if (/(^|\/)(__tests__|__test__|tests?|spec|specs|e2e|cypress|__snapshots__|__mocks__|fixtures|testdata|test_data)(\/|$)/.test(p)) return true
  if (/\.(test|spec|e2e|stories|story|snap)\.[a-z0-9]+$/.test(n)) return true
  if (/^test_.*\.py$|_test\.(go|rs|py|rb|ex|exs|php|dart)$|_spec\.rb$|^spec_.*\.lua$/.test(n)) return true
  if (/(^|\/)conftest\.py$/.test(p)) return true
  return false
}

export function isTypesPath(path: string): boolean {
  const p = path.replace(/\\/g, '/').toLowerCase()
  const n = fileName(p)
  if (/\.d\.(ts|mts|cts)$/.test(n)) return true
  if (/(^|\/)(types|typings|interfaces|schemas?|models?|contracts?|dto|protos?)(\/|\.|$)/.test(p) && SOURCE_EXT.has(fileExt(n))) return true
  if (/^(types|interfaces|schema|contract|model)s?\.[a-z]+$/.test(n)) return true
  if (fileExt(n) === 'proto' || fileExt(n) === 'graphql' || fileExt(n) === 'gql') return true
  return false
}

export function fileKind(path: string): FileKind {
  const n = fileName(path).toLowerCase()
  const ext = fileExt(n)
  if (LOCK_FILES.has(n) || /\.lock$/.test(n)) return 'lock'
  if (isTestPath(path)) return 'test'
  if (DOC_EXT.has(ext) || /^(readme|changelog|license|licence|contributing|authors|notice|code_of_conduct)(\.|$)/.test(n) || /(^|\/)docs?\//.test(path.toLowerCase())) return 'doc'
  if (isTypesPath(path)) return 'types'
  if (CONFIG_FILES.has(n) || n.startsWith('.') || CONFIG_EXT.has(ext) || /(^|\/)(\.github|\.circleci|\.gitlab|ci|\.vscode|\.idea|config|configs|\.caret|\.cursor)\//.test(path.toLowerCase())) return 'config'
  if (SOURCE_EXT.has(ext)) return 'source'
  return 'other'
}

/** Review order: definitions first, then implementation, then verification and ancillary files. */
export const KIND_RANK: Record<FileKind, number> = { types: 0, source: 1, test: 2, doc: 3, config: 4, lock: 5, other: 6 }
export const KIND_LABEL: Record<FileKind, string> = { types: 'Types & schemas', source: 'Implementation', test: 'Tests', doc: 'Docs', config: 'Config', lock: 'Lockfiles', other: 'Other' }
/** Buckets that are collapsed by default in a review. */
export const COLLAPSED_KINDS: ReadonlySet<FileKind> = new Set<FileKind>(['test', 'doc', 'config', 'lock'])

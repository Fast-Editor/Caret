/** Pure helpers for code-block headers and path references in chat markdown. */

const PATH_RE = /^(?:[\w.@-]+\/)*[\w.@-]+\.[A-Za-z0-9]{1,8}$/

/** Extract a language and optional file path from a fenced block's info/className and first line. */
export function parseCodeHeader(className: string | undefined, code: string): { language: string; path?: string; code: string; startLine?: number } {
  let language = ''
  let path: string | undefined
  let startLine: number | undefined
  if (className) {
    const m = /language-([^\s]+)/.exec(className)
    if (m) {
      const info = m[1]
      // forms: "ts", "ts:src/a.ts", "src/a.ts", "ts title=\"src/a.ts\""
      const colon = info.indexOf(':')
      if (colon > 0) { language = info.slice(0, colon); path = info.slice(colon + 1) }
      else if (PATH_RE.test(info) && info.includes('.')) { path = info; language = langFromPath(info) }
      else language = info
    }
    const title = /title=["']([^"']+)["']/.exec(className)
    if (title) path = title[1]
  }
  // first-line comment conventions: "// path/to/file.ts", "# path/file.py", "<!-- file.html -->", "/* file.css */"
  const first = code.split('\n', 1)[0]?.trim() ?? ''
  const cm = /^(?:\/\/|#|--|;|\/\*|<!--)\s*(?:file(?:name|path)?:\s*)?([\w./@-]+\.[A-Za-z0-9]{1,8})(?::(\d+))?\s*(?:\*\/|-->)?\s*$/.exec(first)
  if (!path && cm && PATH_RE.test(cm[1])) {
    path = cm[1]
    if (cm[2]) startLine = Number(cm[2])
    code = code.split('\n').slice(1).join('\n')
    if (!language) language = langFromPath(path)
  }
  return { language: normalizeLang(language), path, code, startLine }
}

export function langFromPath(p: string): string {
  const ext = p.split('.').pop()?.toLowerCase() ?? ''
  const map: Record<string, string> = { ts: 'typescript', tsx: 'typescript', mts: 'typescript', cts: 'typescript', js: 'javascript', jsx: 'javascript', mjs: 'javascript', cjs: 'javascript', py: 'python', rs: 'rust', go: 'go', java: 'java', kt: 'kotlin', rb: 'ruby', php: 'php', cs: 'csharp', cpp: 'cpp', cc: 'cpp', cxx: 'cpp', hpp: 'cpp', c: 'c', h: 'c', m: 'objective-c', swift: 'swift', sh: 'shell', bash: 'shell', zsh: 'shell', json: 'json', jsonc: 'json', yml: 'yaml', yaml: 'yaml', toml: 'ini', ini: 'ini', md: 'markdown', mdx: 'markdown', html: 'html', htm: 'html', css: 'css', scss: 'scss', less: 'less', sql: 'sql', xml: 'xml', svg: 'xml', dockerfile: 'dockerfile', lua: 'lua', dart: 'dart', scala: 'scala', r: 'r', pl: 'perl', ps1: 'powershell', graphql: 'graphql', gql: 'graphql', proto: 'protobuf', vue: 'html', svelte: 'html', zig: 'zig' }
  return map[ext] ?? ''
}

export function normalizeLang(l: string): string {
  const m: Record<string, string> = { ts: 'typescript', js: 'javascript', py: 'python', rs: 'rust', sh: 'shell', bash: 'shell', zsh: 'shell', console: 'shell', shellscript: 'shell', yml: 'yaml', md: 'markdown', jsonc: 'json', 'c++': 'cpp', 'c#': 'csharp', cs: 'csharp', golang: 'go', kt: 'kotlin', rb: 'ruby', objc: 'objective-c', txt: 'plaintext', text: 'plaintext', '': '' }
  const k = l.toLowerCase()
  return m[k] ?? k
}

/** Does inline code look like `path/to/file.ext` or `file.ext:12`? Returns {path, line}. */
export function parsePathRef(text: string): { path: string; line?: number } | null {
  const m = /^((?:[\w.@-]+\/)*[\w.@-]+\.[A-Za-z0-9]{1,8})(?::(\d+))?(?::\d+)?$/.exec(text.trim())
  if (!m) return null
  if (!m[1].includes('.') || /^\d/.test(m[1])) return null
  // avoid matching things like "e.g" or version numbers
  if (/^[\d.]+$/.test(m[1])) return null
  return { path: m[1], line: m[2] ? Number(m[2]) : undefined }
}

/** Parse a unified diff into renderable lines. */
export interface DiffLine { kind: 'add' | 'del' | 'ctx' | 'hunk' | 'meta'; text: string; oldNo?: number; newNo?: number }
export function parseUnifiedDiff(diff: string): DiffLine[] {
  const out: DiffLine[] = []
  let oldNo = 0, newNo = 0
  for (const raw of diff.split('\n')) {
    if (raw.startsWith('@@')) {
      const m = /@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(raw)
      if (m) { oldNo = Number(m[1]); newNo = Number(m[2]) }
      out.push({ kind: 'hunk', text: raw })
    } else if (raw.startsWith('+++') || raw.startsWith('---') || raw.startsWith('diff ') || raw.startsWith('index ')) {
      out.push({ kind: 'meta', text: raw })
    } else if (raw.startsWith('+')) { out.push({ kind: 'add', text: raw.slice(1), newNo: newNo++ }) }
    else if (raw.startsWith('-')) { out.push({ kind: 'del', text: raw.slice(1), oldNo: oldNo++ }) }
    else if (raw.startsWith('\\')) { out.push({ kind: 'meta', text: raw }) }
    else if (raw === '' && out.length === 0) { continue }
    else { out.push({ kind: 'ctx', text: raw.startsWith(' ') ? raw.slice(1) : raw, oldNo: oldNo++, newNo: newNo++ }) }
  }
  return out
}

export function diffStats(diff: string): { additions: number; deletions: number } {
  let additions = 0, deletions = 0
  for (const l of diff.split('\n')) { if (l.startsWith('+') && !l.startsWith('+++')) additions++; else if (l.startsWith('-') && !l.startsWith('---')) deletions++ }
  return { additions, deletions }
}

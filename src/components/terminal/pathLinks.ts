/**
 * File-path detection for terminal output (compiler errors, git listings, test runners…). Pure functions; the
 * terminal instance validates candidates against the file system before turning them into links.
 */
export interface PathCandidate { text: string; start: number; end: number; path: string; line?: number; column?: number }

// A token with a directory separator, or a bare file name with an extension (package.json, Test.java), optionally
// followed by :line[:col] or (line,col). Leading ~, ./ and ../ allowed. Stops at quotes, brackets and whitespace.
const TOKEN = /(?:~|\.{1,2})?\/?[\w.@%+=-]+(?:\/[\w.@%+=-]+)*/g
const SUFFIX = /^(?::(\d+)(?::(\d+))?|\((\d+)(?:,(\d+))?\))/
const VERSION = /^v?\d+(\.\d+)+$/
const DOTFILE = /^\.(?:\w+rc|env(?:\.\w+)?|gitignore|npmignore|dockerignore|editorconfig|prettierrc|eslintrc(?:\.\w+)?)$/
const BARE_NAME_FALSE_POSITIVES = /^(?:Node|Vue|Next|Nuxt|Deno|Bun|React|Express)\.js$|^(?:e\.g|i\.e|etc|vs|cf)\.$/i

function trimTrailing(tok: string): string { return tok.replace(/[.,;:]+$/, '') }

/** All path-like tokens on one line of terminal text (columns are 0-based offsets into `line`). */
export function findPathCandidates(line: string): PathCandidate[] {
  const out: PathCandidate[] = []
  if (!line) return out
  TOKEN.lastIndex = 0
  let m: RegExpExecArray | null
  while ((m = TOKEN.exec(line))) {
    const raw = m[0]; const start = m.index
    // part of a URL (scheme://host/path): skip the whole thing
    if (/^[A-Za-z][\w+.-]*:\/\//.test(line.slice(start))) { const rest = line.slice(start); const len = (/^\S+/.exec(rest) ?? [''])[0].length; TOKEN.lastIndex = start + Math.max(len, raw.length); continue }
    const before = line.slice(Math.max(0, start - 3), start)
    if (before.endsWith('//') || before.endsWith(':/')) continue
    const tok = trimTrailing(raw)
    if (!tok || tok === '.' || tok === '..' || tok === '/' || VERSION.test(tok) || BARE_NAME_FALSE_POSITIVES.test(tok)) continue
    const hasSlash = tok.includes('/')
    const base = tok.slice(tok.lastIndexOf('/') + 1)
    const hasExt = /\.[A-Za-z][A-Za-z0-9]{0,7}$/.test(base) && !/^\.+$/.test(base) || DOTFILE.test(base)
    if (!hasSlash && !hasExt) continue
    if (!hasSlash && /^\d/.test(base)) continue
    let end = start + tok.length
    let lineNo: number | undefined, col: number | undefined
    const suf = SUFFIX.exec(line.slice(end))
    if (suf) { lineNo = parseInt(suf[1] ?? suf[3], 10); const c = suf[2] ?? suf[4]; if (c) col = parseInt(c, 10); end += suf[0].length }
    out.push({ text: line.slice(start, end), start, end, path: tok, line: lineNo, column: col })
  }
  return out
}

/** Collapse `.` / `..` segments of a path. */
export function normalizePath(p: string): string {
  const abs = p.startsWith('/')
  const parts: string[] = []
  for (const seg of p.split('/')) { if (!seg || seg === '.') continue; if (seg === '..') { parts.pop(); continue } parts.push(seg) }
  return (abs ? '/' : '') + parts.join('/')
}

/** Absolute paths to try for a candidate, most likely first: cwd-relative, then workspace-root-relative. */
export function resolveCandidatePaths(path: string, cwd: string | undefined, root: string | undefined, home?: string): string[] {
  if (path.startsWith('~')) return home ? [normalizePath(home + path.slice(1))] : []
  if (path.startsWith('/')) return [normalizePath(path)]
  const bases = [cwd, root].filter((b, i, a): b is string => !!b && a.indexOf(b) === i)
  return bases.map((b) => normalizePath(`${b}/${path}`))
}

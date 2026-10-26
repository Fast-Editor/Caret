/** Lazy Monaco-based syntax colorizer for chat code blocks (shares the editor's tokenizers/theme CSS). */
type Monaco = typeof import('monaco-editor')
let monacoPromise: Promise<Monaco> | null = null
const cache = new Map<string, string>()
const MAX_CACHE = 300
const MAX_LEN = 40_000

function loadMonaco(): Promise<Monaco> {
  if (!monacoPromise) monacoPromise = import('monaco-editor')
  return monacoPromise
}

export function escapeHtml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
}

/** Returns HTML (`<span class="mtkN">`) for `code`, or escaped plain text when unavailable. */
export async function colorize(code: string, language: string): Promise<string> {
  if (!language || language === 'plaintext' || code.length > MAX_LEN || typeof window === 'undefined') return escapeHtml(code)
  const key = `${language}\u0000${code}`
  const hit = cache.get(key)
  if (hit) return hit
  try {
    const monaco = await loadMonaco()
    const known = monaco.languages.getLanguages().some((l) => l.id === language)
    if (!known) return escapeHtml(code)
    const html = await monaco.editor.colorize(code, language, { tabSize: 2 })
    if (cache.size > MAX_CACHE) cache.delete(cache.keys().next().value as string)
    cache.set(key, html)
    return html
  } catch {
    return escapeHtml(code)
  }
}

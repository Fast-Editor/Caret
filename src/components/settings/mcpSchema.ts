/** Build an example args object from a JSON schema (strings → "", numbers → 0, booleans → false). */
export function skeletonFromSchema(schema: unknown): Record<string, unknown> {
  const s = schema as { properties?: Record<string, { type?: string | string[]; default?: unknown; enum?: unknown[] }>; required?: string[] } | undefined
  const out: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(s?.properties ?? {})) {
    if (v.default !== undefined) { out[k] = v.default; continue }
    if (v.enum?.length) { out[k] = v.enum[0]; continue }
    const t = Array.isArray(v.type) ? v.type[0] : v.type
    out[k] = t === 'number' || t === 'integer' ? 0 : t === 'boolean' ? false : t === 'array' ? [] : t === 'object' ? {} : ''
  }
  return out
}

/** Parse a shell-ish argument string into argv (quotes supported). */
export function parseArgs(text: string): string[] {
  if (!text.trim()) return []
  return (text.match(/(?:[^\s"']+|"[^"]*"|'[^']*')+/g) ?? []).map((a) => a.replace(/^(["'])(.*)\1$/, '$2'))
}

/** Directory hint shown next to a tab title when another open tab has the same file name (Cursor-style `…/parent`). */
export function tabDirHint(path: string | undefined, duplicates: number, rel: (p: string) => string = (p) => p): string | null {
  if (!path || duplicates < 2) return null
  const parts = rel(path).split('/').filter(Boolean)
  if (parts.length < 2) return null
  return `…/${parts[parts.length - 2]}`
}

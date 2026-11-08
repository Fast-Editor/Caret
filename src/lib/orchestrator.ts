/** Helpers for the "Caret as an MCP server" settings section (docs/ORCHESTRATOR.md). */

/** Mask every occurrence of `token` in `text` (first six characters kept) unless `show` is set. */
export function maskToken(text: string, token: string, show: boolean): string {
  if (show || !token) return text
  return text.split(token).join(`${token.slice(0, 6)}${'\u2022'.repeat(12)}`)
}

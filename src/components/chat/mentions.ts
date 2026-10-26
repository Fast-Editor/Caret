/** Pure helpers for @-mention and /slash detection in the composer textarea. */

export interface Trigger { kind: '@' | '/'; query: string; start: number; end: number }

/**
 * Find an active trigger ending at the caret: an `@` or `/` at the start of the text or preceded by whitespace,
 * with no whitespace between it and the caret. `/` only triggers at the very start of the input (slash commands).
 */
export function detectTrigger(text: string, caret: number): Trigger | null {
  if (caret < 0 || caret > text.length) return null
  let i = caret - 1
  while (i >= 0) {
    const ch = text[i]
    if (ch === '\n' || ch === ' ' || ch === '\t') return null
    if (ch === '@' || ch === '/') {
      const before = i === 0 ? '' : text[i - 1]
      if (before !== '' && !/\s/.test(before)) { i--; continue }
      if (ch === '/' && text.slice(0, i).trim() !== '') return null
      return { kind: ch, query: text.slice(i + 1, caret), start: i, end: caret }
    }
    i--
  }
  return null
}

/** Replace the trigger text (e.g. "@src/fo") with `replacement` and return the new text + caret. */
export function replaceTrigger(text: string, trigger: Trigger, replacement: string): { text: string; caret: number } {
  const next = text.slice(0, trigger.start) + replacement + text.slice(trigger.end)
  return { text: next, caret: trigger.start + replacement.length }
}

/** Parse a leading slash command: "/name rest of args" → { name, args }. */
export function parseSlashCommand(text: string): { name: string; args: string } | null {
  const m = /^\/([a-zA-Z0-9_:-]+)(?:\s+([\s\S]*))?$/.exec(text.trim())
  if (!m) return null
  return { name: m[1], args: (m[2] ?? '').trim() }
}

export const BUILTIN_SLASH = [
  { name: 'agent', description: 'Switch to Agent mode' },
  { name: 'ask', description: 'Switch to Ask mode' },
  { name: 'plan', description: 'Switch to Plan mode' },
  { name: 'debug', description: 'Switch to Debug mode' },
  { name: 'summarize', description: 'Summarize the conversation to free context' },
  { name: 'review', description: 'Review current changes (Bugbot)' },
  { name: 'commit', description: 'Generate a commit message for staged changes' },
  { name: 'clear', description: 'Start a new chat' },
  { name: 'worktree', description: 'Start a new agent in an isolated git worktree' },
  { name: 'help', description: 'Show chat shortcuts' },
] as const

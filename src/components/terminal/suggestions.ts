/**
 * Ghost-text command suggestions for an xterm instance (fish-style autosuggestions).
 *
 * The shell integration tells us when a prompt starts (OSC 133 A) and when a command starts (B/C/D). The input
 * column is pinned at the user's first keystroke after the prompt (Caret's integration emits B at execution time,
 * so it cannot mark the input start). Between those we read the typed text straight from the terminal buffer, look
 * it up in the command history (recent first), and — when history has nothing and the user pauses — ask the model
 * for a continuation. The suggestion is drawn as a dim decoration after the cursor; → or End accepts it by sending
 * the remainder to the pty.
 */
import type { IDecoration, IMarker, Terminal } from '@xterm/xterm'

export type SuggestionMode = 'off' | 'history' | 'ai'
export interface SuggestionSources {
  /** Commands that start with `prefix`, most recent first (may be sync or async). */
  history: (prefix: string, cwd?: string) => string[] | Promise<string[]>
  /** Model continuation for `prefix` (only the remainder). */
  ai?: (prefix: string) => Promise<string | null>
}
export interface SuggestionEngineOptions {
  term: Terminal
  mode: () => SuggestionMode
  sources: SuggestionSources
  sendInput: (text: string) => void
  cwd: () => string | undefined
  /** Pause before asking the model (ms). */
  aiDelayMs?: number
}
export interface SuggestionEngine {
  /** OSC 133 A: a new prompt is being drawn; input starts at the first keystroke after it. */
  onPromptStart(): void
  /** User keystroke reached the terminal (xterm onData) — pins the input column on the first one after a prompt. */
  noteUserInput(data: string): void
  /** OSC 133 B/C/D: a command is running or finished; no input is being edited. */
  onCommandStart(): void
  /** Diagnostics for the automation bridge. */
  debug(): { originLine: number | null; originCol: number | null; pending: boolean; lastInput: string; suggestion: string | null }
  /** Returns true when the key was consumed (accept / dismiss). */
  handleKey(e: KeyboardEvent): boolean
  /** Current suggestion remainder, if any (for tests / status). */
  current(): string | null
  dispose(): void
}

/** First history entry that extends `prefix` (never the prefix itself). */
export function pickHistorySuggestion(prefix: string, candidates: string[]): string | null {
  if (!prefix.trim()) return null
  for (const c of candidates) { if (c.length > prefix.length && c.startsWith(prefix)) return c.slice(prefix.length) }
  return null
}

/** Sanitise a model continuation: single line, no echo of the prefix, no trailing newline. */
export function cleanAiSuggestion(prefix: string, completion: string | null | undefined): string | null {
  if (!completion) return null
  let s = completion.replace(/\r/g, '').split('\n')[0] ?? ''
  if (prefix && s.startsWith(prefix)) s = s.slice(prefix.length)
  s = s.replace(/\s+$/, '')
  return s ? s : null
}

export function createSuggestionEngine(o: SuggestionEngineOptions): SuggestionEngine {
  const { term } = o
  let origin: { marker: IMarker; col: number } | null = null
  let pending = false // prompt drawn, waiting for the first keystroke to pin the input column
  let decoration: IDecoration | null = null
  let suggestion: string | null = null
  let lastInput = ''
  let aiTimer: ReturnType<typeof setTimeout> | null = null
  let aiSeq = 0
  let scheduled: ReturnType<typeof setTimeout> | null = null
  let disposed = false

  const clearDecoration = () => { decoration?.dispose(); decoration = null; suggestion = null }
  const cancelAi = () => { aiSeq++; if (aiTimer) { clearTimeout(aiTimer); aiTimer = null } }
  const reset = () => { cancelAi(); clearDecoration(); lastInput = '' }

  /** Typed text on the prompt line, or null when the cursor left the input line or sits mid-text. */
  const currentInput = (): string | null => {
    if (!origin || origin.marker.isDisposed || origin.marker.line < 0) return null
    const b = term.buffer.active
    const absCursor = b.baseY + b.cursorY
    if (absCursor !== origin.marker.line) return null
    const line = b.getLine(origin.marker.line); if (!line) return null
    const text = line.translateToString(true)
    if (b.cursorX < origin.col || text.length > b.cursorX) return null // editing in the middle of the line
    return text.slice(origin.col, b.cursorX)
  }

  const show = (remainder: string) => {
    clearDecoration()
    if (!origin || !remainder) return
    const b = term.buffer.active
    const x = b.cursorX
    const visible = remainder.slice(0, Math.max(0, term.cols - x))
    if (!visible) return
    const d = term.registerDecoration({ marker: origin.marker, x, width: visible.length })
    if (!d) return
    d.onRender((el) => {
      el.textContent = visible
      el.style.color = 'var(--fg-faint, rgba(128,128,128,0.75))'
      el.style.whiteSpace = 'pre'
      el.style.pointerEvents = 'none'
      el.style.overflow = 'hidden'
      el.style.fontFamily = String(term.options.fontFamily ?? 'monospace')
      el.style.fontSize = `${term.options.fontSize ?? 12}px`
      el.style.lineHeight = el.style.height
      el.setAttribute('data-testid', 'terminal-ghost')
    })
    decoration = d
    suggestion = remainder
  }

  const recompute = async () => {
    scheduled = null
    if (disposed || o.mode() === 'off') { reset(); return }
    const input = currentInput()
    if (input == null || !input.trim()) { reset(); return }
    if (input === lastInput && suggestion && input.length > 0) return
    lastInput = input
    cancelAi(); clearDecoration()
    let hist: string[] = []
    try { hist = await o.sources.history(input, o.cwd()) } catch { hist = [] }
    if (disposed || currentInput() !== input) return
    const fromHistory = pickHistorySuggestion(input, hist)
    if (fromHistory) { show(fromHistory); return }
    if (o.mode() !== 'ai' || !o.sources.ai || input.trim().length < 3) return
    const seq = ++aiSeq
    aiTimer = setTimeout(async () => {
      aiTimer = null
      if (disposed || seq !== aiSeq || currentInput() !== input) return
      let completion: string | null = null
      try { completion = cleanAiSuggestion(input, await o.sources.ai!(input)) } catch { completion = null }
      if (disposed || seq !== aiSeq || currentInput() !== input || !completion) return
      show(completion)
    }, o.aiDelayMs ?? 700)
  }
  // A short timer rather than requestAnimationFrame: rAF never fires while the window is hidden, which would freeze the engine.
  const schedule = () => { if (!origin || scheduled) return; scheduled = setTimeout(() => void recompute(), 0) }

  const subs = [term.onWriteParsed(schedule), term.onCursorMove(schedule), term.onResize(() => { clearDecoration(); schedule() })]

  return {
    onPromptStart() { reset(); origin?.marker.dispose(); origin = null; pending = true },
    noteUserInput(data) {
      if (!pending || !data) return
      // Enter on an empty prompt or a bare escape sequence does not start input.
      if (data === '\r' || data === '\n' || data === '\x1b') return
      pending = false
      origin?.marker.dispose()
      const marker = term.registerMarker(0)
      origin = marker ? { marker, col: term.buffer.active.cursorX } : null
      schedule()
    },
    onCommandStart() { reset(); origin?.marker.dispose(); origin = null; pending = false },
    debug() { return { originLine: origin ? origin.marker.line : null, originCol: origin?.col ?? null, pending, lastInput, suggestion } },
    handleKey(e) {
      if (e.type !== 'keydown' || !suggestion) return false
      if (e.metaKey || e.ctrlKey || e.altKey) return false
      if (e.key === 'ArrowRight' || e.key === 'End') {
        const remainder = suggestion
        clearDecoration(); lastInput = ''
        o.sendInput(remainder)
        return true
      }
      if (e.key === 'Escape') { reset(); return true }
      return false
    },
    current() { return suggestion },
    dispose() { disposed = true; reset(); origin?.marker.dispose(); origin = null; pending = false; subs.forEach((s) => s.dispose()); if (scheduled) { clearTimeout(scheduled); scheduled = null } },
  }
}

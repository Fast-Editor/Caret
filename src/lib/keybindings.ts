/**
 * Global keybinding service. Keys use a normalized notation: "mod+shift+p", "ctrl+`", chords "mod+k mod+s".
 * `mod` = ⌘ on macOS, Ctrl elsewhere. Bindings resolve to command ids (lib/commands.ts).
 */
import { runCommand, hasCommand } from './commands'
import { emit as busEmit } from './bus'

export interface Keybinding { key: string; command: string; args?: unknown; /** Handle even when Monaco/xterm has focus. */ global?: boolean; when?: KeyContext; source: 'default' | 'user' | 'file' }
export type KeyContext = 'explorer' | 'terminal' | 'editor' | 'chat' | 'sidebar' | 'panel' | 'search'

const IS_MAC = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent)
export const isMac = (): boolean => IS_MAC

// ───────────── parsing / formatting ─────────────
const MOD_ALIASES: Record<string, string> = { cmd: 'meta', command: 'meta', meta: 'meta', win: 'meta', super: 'meta', ctrl: 'ctrl', control: 'ctrl', alt: 'alt', option: 'alt', opt: 'alt', shift: 'shift' }
const KEY_ALIASES: Record<string, string> = { esc: 'escape', return: 'enter', spacebar: 'space', ' ': 'space', del: 'delete', plus: '=', minus: '-', arrowup: 'up', arrowdown: 'down', arrowleft: 'left', arrowright: 'right', backtick: '`', backquote: '`' }

export interface ParsedChord { ctrl: boolean; alt: boolean; shift: boolean; meta: boolean; key: string }

/** Parse one chord ("mod+shift+p") into a canonical struct. "mod" is resolved per platform. */
export function parseChord(chord: string): ParsedChord {
  const out: ParsedChord = { ctrl: false, alt: false, shift: false, meta: false, key: '' }
  const parts = chord.trim().toLowerCase().split('+').map((p) => p.trim()).filter((p, i, arr) => p !== '' || i === arr.length - 1)
  for (let i = 0; i < parts.length; i++) {
    let p = parts[i]
    const last = i === parts.length - 1
    if (p === '' && last) p = '+' // "mod++" means plus key
    if (!last) {
      const m = p === 'mod' ? (IS_MAC ? 'meta' : 'ctrl') : MOD_ALIASES[p]
      if (m) { (out as unknown as Record<string, boolean>)[m] = true; continue }
    }
    out.key = KEY_ALIASES[p] ?? p
  }
  return out
}
export function parseKeybinding(key: string): ParsedChord[] { return key.trim().split(/\s+/).filter(Boolean).map(parseChord) }
export function chordToString(c: ParsedChord): string {
  const parts: string[] = []
  if (c.ctrl) parts.push('ctrl'); if (c.alt) parts.push('alt'); if (c.shift) parts.push('shift'); if (c.meta) parts.push('meta')
  parts.push(c.key)
  return parts.join('+')
}
/** Canonical string (mod resolved) used for lookups. */
export function normalizeKeybinding(key: string): string { return parseKeybinding(key).map(chordToString).join(' ') }

const CODE_MAP: Record<string, string> = { Backquote: '`', Minus: '-', Equal: '=', BracketLeft: '[', BracketRight: ']', Backslash: '\\', Semicolon: ';', Quote: "'", Comma: ',', Period: '.', Slash: '/', Space: 'space', Enter: 'enter', NumpadEnter: 'enter', Escape: 'escape', Tab: 'tab', Backspace: 'backspace', Delete: 'delete', ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right', Home: 'home', End: 'end', PageUp: 'pageup', PageDown: 'pagedown', IntlBackslash: '`' }

/** Build a canonical chord from a KeyboardEvent (uses `code` so ⌥-combos don't produce dead chars). */
export function chordFromEvent(e: KeyboardEvent): ParsedChord | null {
  let key = ''
  const code = e.code
  if (/^Key[A-Z]$/.test(code)) key = code.slice(3).toLowerCase()
  else if (/^Digit[0-9]$/.test(code)) key = code.slice(5)
  else if (/^F([1-9]|1[0-2])$/.test(code)) key = code.toLowerCase()
  else if (CODE_MAP[code]) key = CODE_MAP[code]
  else if (e.key && e.key.length === 1) key = e.key.toLowerCase()
  else if (e.key) key = KEY_ALIASES[e.key.toLowerCase()] ?? e.key.toLowerCase()
  if (!key || ['shift', 'control', 'alt', 'meta', 'os'].includes(key)) return null
  return { ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, meta: e.metaKey, key }
}

const MAC_SYMBOLS: Record<string, string> = { ctrl: '⌃', alt: '⌥', shift: '⇧', meta: '⌘' }
const KEY_LABELS: Record<string, string> = { up: '↑', down: '↓', left: '←', right: '→', enter: '↩', escape: 'Esc', backspace: '⌫', delete: '⌦', tab: '⇥', space: 'Space', pageup: 'PgUp', pagedown: 'PgDn', home: 'Home', end: 'End' }

/** Human label: mac "⌘⇧P", other "Ctrl+Shift+P"; chords separated by a space. */
export function formatKeybinding(key: string): string {
  return parseKeybinding(key).map((c) => {
    const k = KEY_LABELS[c.key] ?? (c.key.length === 1 ? c.key.toUpperCase() : c.key.length <= 3 && /^f\d+$/.test(c.key) ? c.key.toUpperCase() : c.key[0]?.toUpperCase() + c.key.slice(1))
    if (IS_MAC) return `${c.ctrl ? MAC_SYMBOLS.ctrl : ''}${c.alt ? MAC_SYMBOLS.alt : ''}${c.shift ? MAC_SYMBOLS.shift : ''}${c.meta ? MAC_SYMBOLS.meta : ''}${k}`
    const parts = []
    if (c.ctrl) parts.push('Ctrl'); if (c.alt) parts.push('Alt'); if (c.shift) parts.push('Shift'); if (c.meta) parts.push('Win')
    parts.push(k)
    return parts.join('+')
  }).join(' ')
}

// ───────────── defaults ─────────────
const d = (key: string, command: string, extra: Partial<Keybinding> = {}): Keybinding => ({ key, command, source: 'default', ...extra })
export const DEFAULT_KEYBINDINGS: Keybinding[] = [
  d('mod+shift+p', 'palette.commands', { global: true }), d('f1', 'palette.commands', { global: true }),
  d('mod+p', 'palette.files', { global: true }),
  d('mod+shift+f', 'search.findInFiles', { global: true }), d('mod+shift+h', 'search.replaceInFiles', { global: true }),
  d('mod+b', 'view.toggleSidebar', { global: true }), d('mod+j', 'view.togglePanel', { global: true }),
  d('ctrl+`', 'terminal.toggle', { global: true }), d('ctrl+shift+`', 'terminal.new', { global: true }),
  d('mod+l', 'chat.addSelection', { global: true }), d('mod+i', 'chat.new', { global: true }), d('mod+e', 'chat.newBackground', { global: true }),
  d('mod+shift+i', 'chat.toggle', { global: true }),
  d('mod+,', 'settings.open', { global: true }), d('mod+shift+j', 'settings.open', { global: true, args: { page: 'agent' } }),
  d('mod+s', 'file.save', { global: true }), d('mod+alt+s', 'file.saveAll', { global: true }), d('mod+n', 'file.new', { global: true }),
  d('mod+w', 'editor.closeTab', { global: true }), d('mod+shift+t', 'editor.reopenClosed', { global: true }), d('mod+k mod+w', 'editor.closeAllTabs', { global: true }),
  d('mod+\\', 'editor.split', { global: true }), d('mod+1', 'editor.focusGroup', { global: true, args: 0 }), d('mod+2', 'editor.focusGroup', { global: true, args: 1 }), d('mod+3', 'editor.focusGroup', { global: true, args: 2 }),
  d('ctrl+g', 'editor.goToLine', { global: true }), d('mod+shift+o', 'editor.goToSymbol', { global: true }), d('mod+t', 'editor.goToSymbol', { global: true }),
  d('mod+shift+e', 'view.explorer', { global: true }), d('ctrl+shift+g', 'view.scm', { global: true }), d('mod+shift+a', 'view.agents', { global: true }),
  d('mod+alt+a', 'view.toggleAgentsLayout', { global: true }), d('mod+k z', 'view.zen', { global: true }),
  d('mod+=', 'view.zoomIn', { global: true }), d('mod+shift+=', 'view.zoomIn', { global: true }), d('mod+-', 'view.zoomOut', { global: true }), d('mod+0', 'view.zoomReset', { global: true }),
  d('mod+shift+m', 'view.problems', { global: true }), d('mod+shift+u', 'view.output', { global: true }),
  d('mod+alt+g', 'ai.commitMessage', { global: true }), d('mod+shift+r', 'ai.review', { global: true }),
  d('mod+k mod+s', 'settings.keybindings', { global: true }), d('mod+k mod+t', 'app.toggleTheme', { global: true }),
  d('mod+shift+n', 'file.newWindow', { global: true }), d('mod+o', 'workspace.openFolder', { global: true }), d('mod+k mod+o', 'workspace.openFolder', { global: true }),
  d('mod+k', 'ai.inlineEdit'), // editor/terminal own ⌘K when focused
  d('mod+enter', 'chat.acceptAll', { when: 'chat' }), d('mod+backspace', 'chat.rejectAll', { when: 'chat' }),
  d('escape', 'chat.stop', { when: 'chat' }),
  d('f2', 'explorer.rename', { when: 'explorer' }), d('delete', 'explorer.delete', { when: 'explorer' }), d('mod+backspace', 'explorer.delete', { when: 'explorer' }),
  d('mod+c', 'explorer.copy', { when: 'explorer' }), d('mod+x', 'explorer.cut', { when: 'explorer' }), d('mod+v', 'explorer.paste', { when: 'explorer' }), d('mod+f', 'explorer.filter', { when: 'explorer' }),
  d('mod+f', 'terminal.find', { when: 'terminal' }), d('mod+k', 'ai.terminalCmdK', { when: 'terminal' }),
  d('alt+mod+i', 'app.devtools', { global: true }), d('mod+r', 'app.reload', { global: true }),
  d('mod+shift+c', 'terminal.new', { global: true }),
  d('mod+alt+b', 'editor.toggleBlame', { global: true }), d('f8', 'editor.nextProblem', { global: true }),
]

// ───────────── registry / dispatch ─────────────
let bindings: Keybinding[] = []
let userOverrides: Record<string, string | null> = {}
let fileBindings: Keybinding[] = []
let pendingChord: { chord: string; bindings: Keybinding[]; timer: number } | null = null
let started = false
const changeListeners = new Set<() => void>()

function rebuild(): void {
  const overridden = new Set(Object.keys(userOverrides))
  const list: Keybinding[] = DEFAULT_KEYBINDINGS.filter((b) => !overridden.has(b.command))
  for (const [command, key] of Object.entries(userOverrides)) {
    if (key) list.push({ key, command, source: 'user', global: DEFAULT_KEYBINDINGS.find((b) => b.command === command)?.global ?? true })
  }
  list.push(...fileBindings)
  bindings = list.map((b) => ({ ...b, key: normalizeKeybinding(b.key) }))
  changeListeners.forEach((l) => l())
}
export function setUserOverrides(overrides: Record<string, string | null> | undefined): void { userOverrides = overrides ?? {}; rebuild() }
/** keybindings.json entries (VS Code style `{key, command, args?, when?}`; "-command" removes). */
export function setFileBindings(entries: { key: string; command: string; args?: unknown; when?: string }[]): void {
  fileBindings = []
  for (const e of entries) {
    if (!e.key || !e.command) continue
    if (e.command.startsWith('-')) { const cmd = e.command.slice(1); userOverrides = { ...userOverrides, [cmd]: null }; continue }
    fileBindings.push({ key: e.key, command: e.command, args: e.args, when: e.when as KeyContext | undefined, source: 'file', global: true })
  }
  rebuild()
}
export function listBindings(): Keybinding[] { return bindings.slice() }
export function getKeybindingFor(commandId: string): string | undefined { return bindings.find((b) => b.command === commandId)?.key }
export function onKeybindingsChanged(l: () => void): () => void { changeListeners.add(l); return () => { changeListeners.delete(l) } }

let persist: ((overrides: Record<string, string | null>) => Promise<void>) | null = null
/** Hook for the settings editor: persist via settings.update({ keybindings }). */
export function setPersistHandler(fn: typeof persist): void { persist = fn }
export async function setUserBinding(commandId: string, key: string | null): Promise<void> {
  userOverrides = { ...userOverrides, [commandId]: key ? normalizeKeybinding(key) : null }
  rebuild()
  await persist?.(userOverrides)
}
export async function resetUserBinding(commandId: string): Promise<void> {
  const next = { ...userOverrides }
  delete next[commandId]
  userOverrides = next
  rebuild()
  await persist?.(userOverrides)
}

function currentContext(target: EventTarget | null): KeyContext | null {
  const el = target instanceof Element ? target : document.activeElement
  if (!el) return null
  const ctx = el.closest<HTMLElement>('[data-keyctx]')
  return (ctx?.dataset.keyctx as KeyContext | undefined) ?? null
}
function isInsideOwnedWidget(target: EventTarget | null): boolean {
  const el = target instanceof Element ? target : null
  if (!el) return false
  return !!el.closest('.monaco-editor, .xterm, [data-owns-keys]')
}
function isTyping(target: EventTarget | null): boolean {
  const el = target instanceof HTMLElement ? target : null
  if (!el) return false
  const tag = el.tagName
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable
}

function matches(b: Keybinding, chord: string, ctx: KeyContext | null, inWidget: boolean, typing: boolean): 'full' | 'prefix' | null {
  if (b.when && b.when !== ctx) return null
  const hasMod = /(?:^|\+)(meta|ctrl|alt)\+/.test(chord + '+')
  // Non-global bindings don't fire inside Monaco/xterm, nor plain keys while typing in inputs.
  if (!b.global && inWidget && !b.when) return null
  if (typing && !hasMod && !b.when) return null
  if (b.key === chord) return 'full'
  if (b.key.startsWith(chord + ' ')) return 'prefix'
  return null
}

function dispatch(e: KeyboardEvent): void {
  if (e.defaultPrevented || e.isComposing) return
  const c = chordFromEvent(e)
  if (!c) return
  const chord = chordToString(c)
  const ctx = currentContext(e.target)
  const inWidget = isInsideOwnedWidget(e.target)
  const typing = isTyping(e.target)

  if (pendingChord) {
    const full = `${pendingChord.chord} ${chord}`
    const hit = pendingChord.bindings.find((b) => b.key === full && hasCommand(b.command))
    clearTimeout(pendingChord.timer)
    pendingChord = null
    busEmit('keybindings.chord', null)
    if (hit) { e.preventDefault(); e.stopPropagation(); void runCommand(hit.command, hit.args) }
    else if (chord !== 'escape') busEmit('keybindings.chord', { message: `(${formatKeybinding(full)}) is not a command` })
    return
  }

  let full: Keybinding | null = null
  const prefixes: Keybinding[] = []
  for (const b of bindings) {
    const m = matches(b, chord, ctx, inWidget, typing)
    if (m === 'full' && !full && hasCommand(b.command)) full = b
    else if (m === 'prefix') prefixes.push(b)
  }
  if (prefixes.length && !full) {
    e.preventDefault(); e.stopPropagation()
    const timer = window.setTimeout(() => { pendingChord = null; busEmit('keybindings.chord', null) }, 2500)
    pendingChord = { chord, bindings: prefixes, timer }
    busEmit('keybindings.chord', { message: `(${formatKeybinding(chord)}) was pressed. Waiting for second key of chord…` })
    return
  }
  if (full) {
    const cmd = full
    // Let widgets keep ⌘K/⌘F etc. unless the binding is global or context-specific.
    if (!cmd.global && inWidget && !cmd.when) return
    e.preventDefault(); e.stopPropagation()
    void runCommand(cmd.command, cmd.args)
  }
}

export function startKeybindings(): () => void {
  if (started) return () => {}
  started = true
  rebuild()
  window.addEventListener('keydown', dispatch, { capture: true })
  return () => { window.removeEventListener('keydown', dispatch, { capture: true }); started = false }
}

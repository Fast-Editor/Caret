#!/usr/bin/env node
// Drives a running Caret dev instance through the automation bridge (CARET_AUTOMATION=1).
//   node scripts/e2e.mjs health
//   node scripts/e2e.mjs windows                    → the open Caret windows (label, root, name, focused)
//   node scripts/e2e.mjs eval "return document.title"
//   node scripts/e2e.mjs shot name                  → /tmp/caret-shots/name.png
//   node scripts/e2e.mjs cmd editor.openFile '{"path":"/tmp/x.ts"}'   (uses window.__caret.runCommand)
//
// Window: eval, shot and cmd act on the focused Caret window by default. Target another one with
// `--window <label>` or `--window=<label>` anywhere on the command line (e.g. `eval --window w2 "return 1"`), or
// with CARET_E2E_WINDOW=<label>; labels come from `windows` (`main`, `w2`, `w3`, …). An unknown label is an error.
// From code: `evalJs(js, { window: 'w2' })`, `shot(name, { window })`, `cmd(id, args, { window })`.
import { mkdirSync, copyFileSync } from 'node:fs'

const BASE = process.env.CARET_AUTOMATION_URL ?? 'http://127.0.0.1:17420'
const DEFAULT_WINDOW = process.env.CARET_E2E_WINDOW || undefined

/** `?window=<label>` for the bridge, or '' for the focused window. */
function windowQuery(opts) {
  const w = opts?.window ?? DEFAULT_WINDOW
  return w ? `?window=${encodeURIComponent(w)}` : ''
}

async function post(path, body) {
  const r = await fetch(BASE + path, { method: 'POST', body })
  return r.json()
}
export async function health() { return (await fetch(BASE + '/health')).json() }
export async function windows() {
  const r = await (await fetch(BASE + '/windows')).json()
  if (!r.ok) throw new Error(`windows failed: ${r.error}`)
  return r.windows
}
export async function evalJs(js, opts) {
  const r = await post('/eval' + windowQuery(opts), js)
  if (!r.ok) throw new Error(`eval failed: ${r.error ?? r.value}`)
  try { return JSON.parse(r.value) } catch { return r.value }
}
export async function shot(name = `shot-${Date.now()}`, opts) {
  // bring the Caret window to the front (screencapture grabs a screen region; the bridge focuses the target window)
  try { const { execSync } = await import('node:child_process'); execSync(`osascript -e 'tell application "System Events" to set frontmost of (first process whose name is "caret" or name is "Caret") to true'`, { stdio: 'ignore' }); await sleep(600) } catch { /* ignore */ }
  const r = await (await fetch(BASE + '/screenshot' + windowQuery(opts))).json()
  if (!r.ok) throw new Error(`screenshot failed: ${r.error}`)
  mkdirSync('/tmp/caret-shots', { recursive: true })
  const out = `/tmp/caret-shots/${name}.png`
  copyFileSync(r.path, out)
  return out
}
export async function cmd(id, args, opts) {
  return evalJs(`return await window.__caret.runCommand(${JSON.stringify(id)}, ${args ? JSON.stringify(args) : 'undefined'})`, opts)
}
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

/** Pull `--window <label>` / `--window=<label>` out of argv; returns the remaining arguments and the label. */
export function parseWindowArg(argv) {
  const rest = []
  let window
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (a === '--window') { window = argv[++i]; continue }
    if (a.startsWith('--window=')) { window = a.slice('--window='.length); continue }
    rest.push(a)
  }
  return { rest, window }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const { rest, window } = parseWindowArg(process.argv.slice(2))
  const [op, a, b] = rest
  const opts = window ? { window } : undefined
  const run = async () => {
    switch (op) {
      case 'health': return health()
      case 'windows': return windows()
      case 'eval': return evalJs(a, opts)
      case 'shot': return shot(a, opts)
      case 'cmd': return cmd(a, b ? JSON.parse(b) : undefined, opts)
      default: throw new Error('usage: e2e.mjs [--window <label>] health|windows|eval <js>|shot [name]|cmd <id> [json]')
    }
  }
  run().then((r) => console.log(typeof r === 'string' ? r : JSON.stringify(r, null, 2))).catch((e) => { console.error(e.message); process.exit(1) })
}

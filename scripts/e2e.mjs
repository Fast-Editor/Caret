#!/usr/bin/env node
// Drives a running Caret dev instance through the automation bridge (CARET_AUTOMATION=1).
//   node scripts/e2e.mjs health
//   node scripts/e2e.mjs eval "return document.title"
//   node scripts/e2e.mjs shot name            → /tmp/caret-shots/name.png
//   node scripts/e2e.mjs cmd editor.openFile '{"path":"/tmp/x.ts"}'   (uses window.__caret.runCommand)
import { mkdirSync, copyFileSync } from 'node:fs'

const BASE = process.env.CARET_AUTOMATION_URL ?? 'http://127.0.0.1:17420'

async function post(path, body) {
  const r = await fetch(BASE + path, { method: 'POST', body })
  return r.json()
}
export async function health() { return (await fetch(BASE + '/health')).json() }
export async function evalJs(js) {
  const r = await post('/eval', js)
  if (!r.ok) throw new Error(`eval failed: ${r.error ?? r.value}`)
  try { return JSON.parse(r.value) } catch { return r.value }
}
export async function shot(name = `shot-${Date.now()}`) {
  // bring the Caret window to the front (screencapture grabs a screen region)
  try { const { execSync } = await import('node:child_process'); execSync(`osascript -e 'tell application "System Events" to set frontmost of (first process whose name is "caret" or name is "Caret") to true'`, { stdio: 'ignore' }); await sleep(600) } catch { /* ignore */ }
  const r = await (await fetch(BASE + '/screenshot')).json()
  if (!r.ok) throw new Error(`screenshot failed: ${r.error}`)
  mkdirSync('/tmp/caret-shots', { recursive: true })
  const out = `/tmp/caret-shots/${name}.png`
  copyFileSync(r.path, out)
  return out
}
export async function cmd(id, args) {
  return evalJs(`return await window.__caret.runCommand(${JSON.stringify(id)}, ${args ? JSON.stringify(args) : 'undefined'})`)
}
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

if (import.meta.url === `file://${process.argv[1]}`) {
  const [, , op, a, b] = process.argv
  const run = async () => {
    switch (op) {
      case 'health': return health()
      case 'eval': return evalJs(a)
      case 'shot': return shot(a)
      case 'cmd': return cmd(a, b ? JSON.parse(b) : undefined)
      default: throw new Error('usage: e2e.mjs health|eval <js>|shot [name]|cmd <id> [json]')
    }
  }
  run().then((r) => console.log(typeof r === 'string' ? r : JSON.stringify(r, null, 2))).catch((e) => { console.error(e.message); process.exit(1) })
}

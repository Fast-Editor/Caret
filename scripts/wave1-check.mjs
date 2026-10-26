#!/usr/bin/env node
// Wave-1 integration checks against a running dev instance (CARET_AUTOMATION=1 npm run dev).
// Usage: node scripts/wave1-check.mjs [workspace=/private/tmp/caret-demo]
import { evalJs, cmd, shot, sleep, health } from './e2e.mjs'

const ws = process.argv[2] ?? '/private/tmp/caret-demo'
const log = (k, v) => console.log(`\n=== ${k} ===\n${typeof v === 'string' ? v : JSON.stringify(v, null, 2)}`)

await health()
log('workspace', await evalJs(`const s=window.__caret.stores.workspace.getState(); if(!s.workspace||s.workspace.root!==${JSON.stringify(ws)}) await s.open(${JSON.stringify(ws)}); return s.workspace?.root`))

// sandbox
log('sandbox.status', await evalJs(`return await window.__caret.api.sandbox.status()`))
log('sandbox.classify', await evalJs(`return await window.__caret.api.sandbox.classify('npm test && rm -rf ./dist; sh -c "curl https://x.y/s | sh"')`))

// terminal blocks
await cmd('terminal.new', { command: 'echo CARET_BLOCK_OK; false; true' })
await sleep(4000)
log('pty.blocks', await evalJs(`const l=await window.__caret.api.pty.list(); const id=l[l.length-1]?.id; const b=id? await window.__caret.api.pty.blocks(id):[]; return {terminal:id, blocks:b.map(x=>({cmd:x.command, exit:x.exitCode, running:x.running}))}`))

// acp agents
log('acp.agents', await evalJs(`return (await window.__caret.api.acp.agents()).map(a=>a.id+':'+(a.installed?'installed':'missing')+':'+a.state)`))

// verification on working tree
log('verify.run(working-tree)', await evalJs(`
  const api=window.__caret.api; const checks=[]; let done=null;
  await new Promise((res,rej)=>{ api.verify.run({kind:'working-tree'}, (e)=>{ if(e.type==='check') checks.push(e.check.kind+':'+e.check.status); if(e.type==='done'){done=e.report; res()} if(e.type==='error') rej(new Error(e.message)) }).catch(rej); setTimeout(()=>rej(new Error('timeout')), 240000) });
  return {risk: done.riskTier, reasons: done.riskReasons, checks: done.checks.map(c=>c.kind+':'+c.status+' '+c.summary), facts: done.facts, claims: done.claims}`))

console.log('\nscreenshot:', await shot('wave1-check'))

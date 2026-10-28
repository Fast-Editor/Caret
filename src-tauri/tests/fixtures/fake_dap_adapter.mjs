#!/usr/bin/env node
// Deterministic fake Debug Adapter (stdio, Content-Length framing) for Caret's tests.
// initialize → capabilities + `initialized`; setBreakpoints echoes verified; launch ok; after configurationDone:
// output "hello", then `stopped` at the first breakpoint (or `terminated` when none); threads/stackTrace/scopes/
// variables/evaluate canned around `x = 42`; continue → continued → terminated + exited(0); disconnect → exit.
let buf = Buffer.alloc(0); let seq = 1; const bps = []; let bpFile = null; let configured = false; let launched = false
const send = (o) => { const b = Buffer.from(JSON.stringify({ seq: seq++, ...o })); process.stdout.write(`Content-Length: ${b.length}\r\n\r\n`); process.stdout.write(b) }
const respond = (req, body, success = true, message) => send({ type: 'response', request_seq: req.seq, success, command: req.command, body, ...(message ? { message } : {}) })
const event = (name, body) => send({ type: 'event', event: name, body })
const maybeRun = () => { if (!configured || !launched) return; event('output', { category: 'stdout', output: 'hello from fake adapter\n' }); if (bps.length) event('stopped', { reason: 'breakpoint', threadId: 1, allThreadsStopped: true, hitBreakpointIds: [1] }); else { event('terminated', {}); event('exited', { exitCode: 0 }) } }
process.stdin.on('data', (d) => {
  buf = Buffer.concat([buf, d])
  for (;;) {
    const hdr = buf.indexOf('\r\n\r\n'); if (hdr < 0) return
    const len = Number(/Content-Length:\s*(\d+)/i.exec(buf.slice(0, hdr).toString())?.[1] ?? 0)
    if (buf.length < hdr + 4 + len) return
    const msg = JSON.parse(buf.slice(hdr + 4, hdr + 4 + len).toString()); buf = buf.slice(hdr + 4 + len)
    if (msg.type !== 'request') continue
    switch (msg.command) {
      case 'initialize': respond(msg, { supportsConfigurationDoneRequest: true, supportsEvaluateForHovers: true }); event('initialized', {}); break
      case 'setBreakpoints': { bpFile = msg.arguments.source.path; bps.length = 0; for (const b of msg.arguments.breakpoints || []) bps.push(b.line); respond(msg, { breakpoints: bps.map((l, i) => ({ id: i + 1, verified: true, line: l })) }); break }
      case 'setExceptionBreakpoints': respond(msg, { breakpoints: [] }); break
      case 'configurationDone': respond(msg, {}); configured = true; setTimeout(maybeRun, 10); break
      case 'launch': case 'attach': respond(msg, {}); launched = true; setTimeout(maybeRun, 10); break
      case 'threads': respond(msg, { threads: [{ id: 1, name: 'main' }] }); break
      case 'stackTrace': respond(msg, { stackFrames: [{ id: 10, name: 'main', line: bps[0] ?? 1, column: 1, source: { path: bpFile, name: 'x.js' } }], totalFrames: 1 }); break
      case 'scopes': respond(msg, { scopes: [{ name: 'Locals', variablesReference: 100, expensive: false }] }); break
      case 'variables': respond(msg, { variables: msg.arguments.variablesReference === 100 ? [{ name: 'x', value: '42', type: 'number', variablesReference: 0 }, { name: 'obj', value: 'Object', type: 'object', variablesReference: 101 }] : [{ name: 'k', value: '"v"', type: 'string', variablesReference: 0 }] }); break
      case 'evaluate': respond(msg, { result: msg.arguments.expression === 'x' ? '42' : `<${msg.arguments.expression}>`, type: 'number', variablesReference: 0 }); break
      case 'continue': respond(msg, { allThreadsContinued: true }); event('continued', { threadId: 1, allThreadsContinued: true }); setTimeout(() => { event('terminated', {}); event('exited', { exitCode: 0 }) }, 10); break
      case 'next': case 'stepIn': case 'stepOut': respond(msg, {}); setTimeout(() => event('stopped', { reason: 'step', threadId: 1, allThreadsStopped: true }), 10); break
      case 'pause': respond(msg, {}); break
      case 'disconnect': case 'terminate': respond(msg, {}); setTimeout(() => process.exit(0), 10); break
      default: respond(msg, null, false, `unsupported ${msg.command}`)
    }
  }
})
process.stdin.on('end', () => process.exit(0))

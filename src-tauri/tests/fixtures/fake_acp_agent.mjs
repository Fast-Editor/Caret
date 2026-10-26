#!/usr/bin/env node
// Deterministic fake ACP agent used by Caret's integration tests. Speaks newline-delimited JSON-RPC 2.0 on stdio.
// Behaviour per prompt text:
//   "hello"            → streams two text chunks, a thought, then end_turn
//   "edit <path>"      → reads the file via fs/read_text_file, asks permission, writes via fs/write_text_file, reports a
//                        tool_call with a diff block, then end_turn
//   "run <cmd...>"     → terminal/create + wait_for_exit + output, tool_call with terminal block, end_turn
//   "plan"             → plan update with 3 entries, then end_turn
//   "slow"             → streams one chunk per 200 ms until cancelled → stopReason cancelled
//   "refuse"           → stopReason refusal
//   "needauth"         → (only when FAKE_ACP_NEED_AUTH=1) session/new fails with -32000 until authenticate is called
import readline from 'node:readline'
const rl = readline.createInterface({ input: process.stdin })
const out = (o) => process.stdout.write(JSON.stringify(o) + '\n')
let nextId = 1000; const pending = new Map()
const request = (method, params) => new Promise((res, rej) => { const id = nextId++; pending.set(id, { res, rej }); out({ jsonrpc: '2.0', id, method, params }) })
const notify = (method, params) => out({ jsonrpc: '2.0', method, params })
const update = (sessionId, u) => notify('session/update', { sessionId, update: u })
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
let authed = !process.env.FAKE_ACP_NEED_AUTH
const sessions = new Map(); const cancelled = new Set()
const configs = new Map()
const defaultConfig = () => [{ id: 'model', name: 'Model', category: 'model', type: 'select', currentValue: 'fast', options: [{ value: 'fast', name: 'Fast' }, { value: 'smart', name: 'Smart', description: 'Slower, better' }] }]
process.stderr.write('fake-acp-agent starting\n')

async function prompt(id, params) {
  const sid = params.sessionId; const text = (params.prompt || []).filter((b) => b.type === 'text').map((b) => b.text).join('\n').trim()
  const last = text.split('\n').filter(Boolean).pop() || ''
  const end = (stopReason) => out({ jsonrpc: '2.0', id, result: { stopReason } })
  if (last === 'refuse') return end('refusal')
  if (last === 'plan') {
    update(sid, { sessionUpdate: 'plan', entries: [{ content: 'Read files', priority: 'high', status: 'completed' }, { content: 'Edit code', priority: 'medium', status: 'in_progress' }, { content: 'Run tests', priority: 'low', status: 'pending' }] })
    update(sid, { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'Plan posted.' } })
    return end('end_turn')
  }
  if (last === 'slow') {
    for (let i = 0; i < 100; i++) { if (cancelled.has(sid)) { cancelled.delete(sid); return end('cancelled') } update(sid, { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: `tick ${i} ` } }); await sleep(200) }
    return end('end_turn')
  }
  if (last.startsWith('edit ')) {
    const path = last.slice(5).trim(); const tc = 'call_edit_1'
    const { content } = await request('fs/read_text_file', { sessionId: sid, path })
    update(sid, { sessionUpdate: 'tool_call', toolCallId: tc, title: `Edit ${path}`, kind: 'edit', status: 'pending', locations: [{ path, line: 1 }], rawInput: { path } })
    const proposed = content + '// edited by fake agent\n'
    const perm = await request('session/request_permission', { sessionId: sid, toolCall: { toolCallId: tc, title: `Edit ${path}`, kind: 'edit', content: [{ type: 'diff', path, oldText: content, newText: proposed }] }, options: [{ optionId: 'allow', name: 'Allow', kind: 'allow_once' }, { optionId: 'allow-always', name: 'Always allow', kind: 'allow_always' }, { optionId: 'reject', name: 'Reject', kind: 'reject_once' }] })
    if (perm.outcome?.outcome !== 'selected' || perm.outcome.optionId === 'reject') { update(sid, { sessionUpdate: 'tool_call_update', toolCallId: tc, status: 'failed', content: [{ type: 'content', content: { type: 'text', text: 'Permission denied' } }] }); update(sid, { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'Edit was rejected.' } }); return end('end_turn') }
    const newText = content + '// edited by fake agent\n'
    update(sid, { sessionUpdate: 'tool_call_update', toolCallId: tc, status: 'in_progress' })
    await request('fs/write_text_file', { sessionId: sid, path, content: newText })
    update(sid, { sessionUpdate: 'tool_call_update', toolCallId: tc, status: 'completed', content: [{ type: 'diff', path, oldText: content, newText }] })
    update(sid, { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: `Edited ${path}.` } })
    return end('end_turn')
  }
  if (last.startsWith('snippetedit ')) {
    // Writes the file itself (no fs/write_text_file) and reports a Claude-Code-style snippet diff.
    const fs = await import('node:fs'); const path = last.slice(12).trim(); const tc = 'call_snip_1'
    const content = fs.readFileSync(path, 'utf8'); const oldSnip = content.split('\n')[0]; const newSnip = '// header\n' + oldSnip
    update(sid, { sessionUpdate: 'tool_call', toolCallId: tc, title: `Edit ${path}`, kind: 'edit', status: 'in_progress', content: [{ type: 'diff', path, oldText: oldSnip, newText: newSnip }] })
    await sleep(20)
    fs.writeFileSync(path, content.replace(oldSnip, newSnip))
    update(sid, { sessionUpdate: 'tool_call_update', toolCallId: tc, status: 'completed' })
    update(sid, { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'Snippet edit done.' } })
    return end('end_turn')
  }
  if (last.startsWith('run ')) {
    const parts = last.slice(4).trim().split(/\s+/); const tc = 'call_run_1'
    const { terminalId } = await request('terminal/create', { sessionId: sid, command: parts[0], args: parts.slice(1), env: [], outputByteLimit: 65536 })
    update(sid, { sessionUpdate: 'tool_call', toolCallId: tc, title: `Run ${last.slice(4)}`, kind: 'execute', status: 'in_progress', content: [{ type: 'terminal', terminalId }], rawInput: { command: last.slice(4) } })
    const exit = await request('terminal/wait_for_exit', { sessionId: sid, terminalId })
    const o = await request('terminal/output', { sessionId: sid, terminalId })
    await request('terminal/release', { sessionId: sid, terminalId })
    update(sid, { sessionUpdate: 'tool_call_update', toolCallId: tc, status: exit.exitCode === 0 ? 'completed' : 'failed', rawOutput: { exitCode: exit.exitCode, output: o.output } })
    update(sid, { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: `Command exited ${exit.exitCode}: ${o.output.trim()}` } })
    return end('end_turn')
  }
  // default: hello
  update(sid, { sessionUpdate: 'agent_thought_chunk', content: { type: 'text', text: 'thinking about greeting' } })
  update(sid, { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'Hello from ' } })
  await sleep(20)
  update(sid, { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'the fake agent.' } })
  update(sid, { sessionUpdate: 'usage_update', used: 1234, size: 200000, cost: { amount: 0.01, currency: 'USD' } })
  end('end_turn')
}

rl.on('line', async (line) => {
  let m; try { m = JSON.parse(line) } catch { return }
  if (m.id != null && !m.method) { const p = pending.get(m.id); if (p) { pending.delete(m.id); m.error ? p.rej(m.error) : p.res(m.result) } return }
  const { id, method, params } = m
  switch (method) {
    case 'initialize': return out({ jsonrpc: '2.0', id, result: { protocolVersion: 1, agentCapabilities: { loadSession: true, promptCapabilities: { image: true, audio: false, embeddedContext: true }, mcpCapabilities: { http: true, sse: false } }, agentInfo: { name: 'fake-acp-agent', title: 'Fake Agent', version: '9.9.9' }, authMethods: [{ id: 'fake-login', name: 'Fake login', description: 'Pretend to log in' }] } })
    case 'authenticate': authed = true; return out({ jsonrpc: '2.0', id, result: {} })
    case 'session/new': {
      if (!authed) return out({ jsonrpc: '2.0', id, error: { code: -32000, message: 'Authentication required' } })
      const sid = 'fake-sess-' + Math.random().toString(36).slice(2, 8); sessions.set(sid, params)
      configs.set(sid, defaultConfig())
      out({ jsonrpc: '2.0', id, result: { sessionId: sid, modes: { currentModeId: 'ask', availableModes: [{ id: 'ask', name: 'Ask' }, { id: 'code', name: 'Code', description: 'Edits files' }] }, configOptions: configs.get(sid) } })
      update(sid, { sessionUpdate: 'available_commands_update', availableCommands: [{ name: 'help', description: 'Show help' }] })
      return
    }
    case 'session/load': return out({ jsonrpc: '2.0', id, result: {} })
    case 'session/set_mode': update(params.sessionId, { sessionUpdate: 'current_mode_update', currentModeId: params.modeId }); return out({ jsonrpc: '2.0', id, result: {} })
    case 'session/set_config_option': {
      const opts = configs.get(params.sessionId) || defaultConfig(); const o = opts.find((x) => x.id === params.configId)
      if (!o || !o.options.some((c) => c.value === params.value)) return out({ jsonrpc: '2.0', id, error: { code: -32602, message: `Invalid params: unknown ${params.configId}=${params.value}` } })
      o.currentValue = params.value; configs.set(params.sessionId, opts)
      out({ jsonrpc: '2.0', id, result: { configOptions: opts } })
      update(params.sessionId, { sessionUpdate: 'config_option_update', configOptions: opts })
      return
    }
    case 'session/cancel': cancelled.add(params.sessionId); return
    case 'session/prompt': return void prompt(id, params)
    default: if (id != null) out({ jsonrpc: '2.0', id, error: { code: -32601, message: `Method not found: ${method}` } })
  }
})
rl.on('close', () => process.exit(0))

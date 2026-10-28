#!/usr/bin/env node
// Tiny stand-in for the Lynkr sidecar used by Caret's router tests. Reads PORT (and TIER_*) from <cwd>/.env like
// Lynkr does, serves GET /health and echoes POST /v1/messages with X-Lynkr-* headers. FAKE_LYNKR_EXIT_AFTER_MS makes
// it exit (crash simulation) after the given delay.
import { createServer } from 'node:http'
import { readFileSync } from 'node:fs'
const env = {}
try { for (const line of readFileSync('.env', 'utf8').split('\n')) { const m = line.match(/^([A-Z0-9_]+)=(.*)$/); if (m) env[m[1]] = m[2].replace(/^'(.*)'$/, '$1') } } catch {}
const port = Number(process.env.PORT || env.PORT || 0)
const tierModel = (env.TIER_SIMPLE || 'azure-anthropic:unknown').split(':').slice(1).join(':')
const server = createServer((req, res) => {
  if (req.method === 'GET' && (req.url === '/health' || req.url === '/health/ready')) { res.writeHead(200, { 'content-type': 'application/json' }); return res.end('{"status":"ok"}') }
  if (req.method === 'POST' && req.url === '/v1/messages') {
    let body = ''; req.on('data', (c) => { body += c }); req.on('end', () => {
      res.writeHead(200, { 'content-type': 'application/json', 'x-lynkr-tier': 'SIMPLE', 'x-lynkr-model': tierModel, 'x-lynkr-provider': 'azure-anthropic', 'x-lynkr-complexity-score': '7' })
      res.end(JSON.stringify({ id: 'msg_fake', type: 'message', role: 'assistant', model: tierModel, content: [{ type: 'text', text: 'echo:' + body.length }], stop_reason: 'end_turn', usage: { input_tokens: 1, output_tokens: 1 } }))
    }); return
  }
  res.writeHead(404); res.end()
})
server.listen(port, '127.0.0.1', () => { process.stderr.write(`fake-lynkr listening on ${port}\n`) })
if (process.env.FAKE_LYNKR_EXIT_AFTER_MS) setTimeout(() => process.exit(3), Number(process.env.FAKE_LYNKR_EXIT_AFTER_MS))

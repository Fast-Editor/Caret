import { describe, it, expect } from 'vitest'
import { sortPorts, portUrl, originLabel, addressLabel } from './ports'
import type { ListeningPort } from '@/types'

const p = (port: number, pid: number, address = '127.0.0.1', ownedByCaret = false): ListeningPort => ({ port, address, pid, process: 'node', command: 'node', ownedByCaret, protocol: 'tcp' })

describe('ports helpers', () => {
  it('sorts by port then pid without mutating the input', () => {
    const input = [p(8080, 9), p(3000, 5), p(3000, 2)]
    const out = sortPorts(input)
    expect(out.map((x) => [x.port, x.pid])).toEqual([[3000, 2], [3000, 5], [8080, 9]])
    expect(input[0].port).toBe(8080)
  })
  it('builds localhost URLs for wildcard and IPv6 binds and keeps concrete IPv4 hosts', () => {
    expect(portUrl(p(3000, 1, '*'))).toBe('http://localhost:3000')
    expect(portUrl(p(3000, 1, '0.0.0.0'))).toBe('http://localhost:3000')
    expect(portUrl(p(3000, 1, '::1'))).toBe('http://localhost:3000')
    expect(portUrl(p(8081, 1, '127.0.0.1'))).toBe('http://127.0.0.1:8081')
  })
  it('labels origin and addresses', () => {
    expect(originLabel(p(1, 1, '*', true))).toBe('Caret terminal')
    expect(originLabel(p(1, 1))).toBe('Other')
    expect(addressLabel('*')).toBe('all interfaces'); expect(addressLabel('127.0.0.1')).toBe('127.0.0.1')
  })
})

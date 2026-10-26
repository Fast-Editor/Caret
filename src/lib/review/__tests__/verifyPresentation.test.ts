import { describe, it, expect } from 'vitest'
import { metricChips, findingsForPath, summarizeReport, overallFailing } from '../verifyPresentation'
import type { VerificationReport } from '@/types'

const report: VerificationReport = {
  id: 'r1', scope: { kind: 'session', sessionId: 's' }, workspaceRoot: '/r', startedAt: 1, status: 'fail', riskTier: 2, riskReasons: ['tests failing'], facts: [], claims: [], filesTouched: ['/r/src/a.ts'], additions: 3, deletions: 1, summary: '',
  checks: [
    { id: 'c1', kind: 'tests', title: 'Tests', status: 'fail', summary: '', findings: [{ id: 'f1', path: 'src/a.ts', line: 3, severity: 'high', message: 'boom' }], metrics: { passed: 11, failed: 1, total: 12 } },
    { id: 'c2', kind: 'mutation', title: 'Mutation', status: 'warn', summary: '', findings: [], metrics: { killed: 7, total: 9, score: 0.78 } },
    { id: 'c3', kind: 'dependencies', title: 'Deps', status: 'pass', summary: '', findings: [], metrics: { new: 2, vulnerable: 0 } },
    { id: 'c4', kind: 'security', title: 'Sec', status: 'warn', summary: '', findings: [{ id: 'f2', path: '/r/src/a.ts', line: 9, severity: 'medium', message: 'eval' }, { id: 'f3', severity: 'low', message: 'x' }] },
  ],
}
describe('verify presentation', () => {
  it('renders metric chips per kind', () => {
    expect(metricChips(report.checks[0])).toEqual(['11 passed', '1 failed'])
    expect(metricChips(report.checks[1])).toEqual(['7/9 killed', '78%'])
    expect(metricChips(report.checks[2])).toEqual(['2 new deps', '0 CVEs'])
    expect(metricChips(report.checks[3])).toEqual(['1 medium', '1 low'])
    expect(metricChips({ kind: 'lint', findings: [], metrics: { custom: 3 } })).toEqual(['custom: 3'])
  })
  it('matches findings by path suffix and summarises', () => {
    expect(findingsForPath(report, '/r/src/a.ts').map((f) => f.id)).toEqual(['f1', 'f2'])
    expect(overallFailing(report)).toBe(true)
    expect(summarizeReport(report)).toBe('1 check failing')
    expect(summarizeReport({ ...report, status: 'pass', checks: report.checks.map((c) => ({ ...c, status: 'pass' as const })) })).toBe('All checks passed')
  })
})

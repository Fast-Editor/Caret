/** Presentation helpers for verification reports (pure, tested). */
import type { RiskTier, VerificationReport, VerifyCheck, VerifyCheckKind, VerifyStatus } from '@/types'

export const CHECK_TITLES: Record<VerifyCheckKind, string> = {
  diagnostics: 'Diagnostics', 'project-check': 'Project check', dependencies: 'Dependencies', security: 'Security rules', tests: 'Tests', mutation: 'Mutation score', equivalence: 'Equivalence', properties: 'Properties', lint: 'Lint',
}
export const CHECK_ORDER: VerifyCheckKind[] = ['diagnostics', 'project-check', 'dependencies', 'security', 'tests', 'mutation', 'properties', 'equivalence', 'lint']

export function riskLabel(t: RiskTier | undefined): string { return t === 0 ? 'low risk' : t === 1 ? 'needs review' : t === 2 ? 'review carefully' : 'unverified' }
export function riskTone(t: RiskTier | undefined): 'success' | 'warning' | 'danger' | 'muted' { return t === 0 ? 'success' : t === 1 ? 'warning' : t === 2 ? 'danger' : 'muted' }

const pct = (n: number) => `${Math.round(n * 100)}%`
const num = (v: number | undefined) => (v === undefined || Number.isNaN(v) ? undefined : Math.round(v))

/** Metric chips per check kind (falls back to generic key: value chips). */
export function metricChips(check: Pick<VerifyCheck, 'kind' | 'metrics' | 'findings'>): string[] {
  const m = check.metrics ?? {}
  const out: string[] = []
  switch (check.kind) {
    case 'tests': {
      if (m.passed !== undefined) out.push(`${num(m.passed)} passed`)
      if (m.failed) out.push(`${num(m.failed)} failed`)
      if (m.skipped) out.push(`${num(m.skipped)} skipped`)
      if (out.length === 0 && m.total !== undefined) out.push(`${num(m.total)} tests`)
      break
    }
    case 'mutation': {
      if (m.total !== undefined) out.push(`${num(m.killed ?? 0)}/${num(m.total)} killed`)
      if (m.score !== undefined) out.push(pct(m.score > 1 ? m.score / 100 : m.score))
      if (m.timeouts) out.push(`${num(m.timeouts)} timeouts`)
      break
    }
    case 'dependencies': {
      if (m.new !== undefined) out.push(`${num(m.new)} new dep${m.new === 1 ? '' : 's'}`)
      if (m.vulnerable !== undefined) out.push(`${num(m.vulnerable)} CVE${m.vulnerable === 1 ? '' : 's'}`)
      if (m.unknown) out.push(`${num(m.unknown)} unknown`)
      if (m.unlocked) out.push(`${num(m.unlocked)} unlocked`)
      break
    }
    case 'diagnostics': case 'project-check': case 'lint': {
      if (m.errors !== undefined) out.push(`${num(m.errors)} error${m.errors === 1 ? '' : 's'}`)
      if (m.warnings !== undefined) out.push(`${num(m.warnings)} warning${m.warnings === 1 ? '' : 's'}`)
      if (m.filesChecked !== undefined) out.push(`${num(m.filesChecked)} files`)
      break
    }
    case 'security': {
      const sev: Record<string, number> = {}
      for (const f of check.findings ?? []) sev[f.severity] = (sev[f.severity] ?? 0) + 1
      for (const s of ['critical', 'high', 'medium', 'low'] as const) if (sev[s]) out.push(`${sev[s]} ${s}`)
      if (out.length === 0 && m.rules !== undefined) out.push(`${num(m.rules)} rules`)
      break
    }
    case 'equivalence': case 'properties': {
      if (m.cases !== undefined) out.push(`${num(m.cases)} cases`)
      if (m.failures) out.push(`${num(m.failures)} failures`)
      if (m.counterexamples) out.push(`${num(m.counterexamples)} counterexample${m.counterexamples === 1 ? '' : 's'}`)
      break
    }
  }
  if (out.length === 0) for (const [k, v] of Object.entries(m).slice(0, 4)) out.push(`${k}: ${typeof v === 'number' && !Number.isInteger(v) ? v.toFixed(2) : v}`)
  return out
}

export function statusRank(s: VerifyStatus): number { return ({ fail: 0, error: 1, running: 2, warn: 3, pending: 4, pass: 5, skipped: 6 } as Record<VerifyStatus, number>)[s] ?? 7 }

export function sortChecks(checks: VerifyCheck[]): VerifyCheck[] {
  return [...checks].sort((a, b) => CHECK_ORDER.indexOf(a.kind) - CHECK_ORDER.indexOf(b.kind))
}

export function overallFailing(report: VerificationReport | null | undefined): boolean {
  return !!report && (report.status === 'fail' || report.checks.some((c) => c.status === 'fail'))
}

/** Findings of a report for a given absolute or relative path (suffix match tolerates root differences). */
export function findingsForPath(report: VerificationReport | null | undefined, path: string) {
  if (!report) return []
  const norm = (p: string) => p.replace(/\\/g, '/')
  const target = norm(path)
  return report.checks.flatMap((c) => c.findings.filter((f) => f.path && (norm(f.path) === target || target.endsWith('/' + norm(f.path)) || norm(f.path).endsWith('/' + target))).map((f) => ({ ...f, checkKind: c.kind })))
}

export function summarizeReport(report: VerificationReport): string {
  const failing = report.checks.filter((c) => c.status === 'fail').length
  const warning = report.checks.filter((c) => c.status === 'warn').length
  const ran = report.checks.filter((c) => c.status !== 'skipped' && c.status !== 'pending').length
  if (report.status === 'running') return `Verifying… ${ran}/${report.checks.length} checks done`
  if (failing) return `${failing} check${failing === 1 ? '' : 's'} failing`
  if (warning) return `${warning} warning${warning === 1 ? '' : 's'}`
  return ran ? 'All checks passed' : 'No checks ran'
}

import { ShieldAlert, ShieldCheck, ShieldQuestion } from 'lucide-react'
import type { RiskTier } from '@/types'
import { riskLabel, riskTone } from '@/lib/review'

const TONE: Record<string, string> = { success: 'bg-success/15 text-success border-success/30', warning: 'bg-warning/15 text-warning border-warning/30', danger: 'bg-danger/15 text-danger border-danger/30', muted: 'bg-input text-muted border-border' }

export function RiskTierPill({ tier, reasons, size = 'sm' }: { tier: RiskTier | undefined; reasons?: string[]; size?: 'sm' | 'xs' }) {
  const tone = riskTone(tier)
  const Icon = tier === 0 ? ShieldCheck : tier === undefined ? ShieldQuestion : ShieldAlert
  const title = reasons && reasons.length ? `Risk tier ${tier}\n• ${reasons.join('\n• ')}` : tier === undefined ? 'Not verified yet' : `Risk tier ${tier}`
  return (
    <span title={title} className={`inline-flex items-center gap-1 rounded-full border ${TONE[tone]} ${size === 'xs' ? 'px-1.5 h-4 text-[9.5px]' : 'px-2 h-5 text-[10.5px]'} font-medium whitespace-nowrap`}>
      <Icon size={size === 'xs' ? 10 : 11} />{tier !== undefined ? `T${tier} · ` : ''}{riskLabel(tier)}
    </span>
  )
}

/** Compact status glyph used in file rows. */
export function ProofBadge({ status, count, title }: { status: 'pass' | 'warn' | 'fail' | 'running' | 'none'; count?: number; title?: string }) {
  const cls = status === 'pass' ? 'text-success' : status === 'warn' ? 'text-warning' : status === 'fail' ? 'text-danger' : status === 'running' ? 'text-accent pulse' : 'text-faint'
  const glyph = status === 'pass' ? '✓' : status === 'warn' ? '!' : status === 'fail' ? '✕' : status === 'running' ? '…' : '·'
  return <span title={title} className={`inline-flex items-center justify-center min-w-[16px] h-4 px-1 rounded border border-border text-[10px] ${cls}`}>{glyph}{count ? <span className="ml-0.5">{count}</span> : null}</span>
}

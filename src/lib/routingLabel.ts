import type { RoutingInfo } from '@/types'

/** Footer pill text for an Auto-routed message: `Auto → Claude Sonnet 5.5 · COMPLEX` (`↷` when the router fell back). */
export function routingLabel(r: RoutingInfo, displayName?: string): string {
  const name = displayName || r.servedModelId
  const tier = r.servedTier ?? r.requestedTier
  return `Auto → ${name}${tier ? ` · ${tier}` : ''}${r.fallback ? ' ↷' : ''}`
}

export function routingTooltip(r: RoutingInfo): string {
  const bits = [`Served by ${r.servedModelId}`]
  if (r.requestedTier && r.servedTier && r.requestedTier !== r.servedTier) bits.push(`requested ${r.requestedTier}, served ${r.servedTier}`)
  else if (r.requestedTier || r.servedTier) bits.push(`tier ${r.servedTier ?? r.requestedTier}`)
  if (r.score !== undefined) bits.push(`complexity ${Math.round(r.score)}`)
  if (r.method) bits.push(r.method)
  if (r.fallback) bits.push('the router climbed or fell back from its first choice')
  return bits.join(' · ')
}

/** Human label for a message's model id: configured display name, agent name for `acp:<id>`, else the raw id. */
export function modelLabel(modelId: string | undefined, displayName?: string): string | undefined {
  if (!modelId) return undefined
  if (displayName) return displayName
  if (modelId.startsWith('acp:')) return modelId.slice(4).split(/[-_]/).filter(Boolean).map((w) => w[0].toUpperCase() + w.slice(1)).join(' ')
  return modelId
}

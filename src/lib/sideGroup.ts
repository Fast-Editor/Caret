/**
 * Which editor group a "to the side" action targets: the group to the right of the active one, else the one to its
 * left; `null` when there is only one group (the caller splits first).
 */
export function sideGroupIndex(groupCount: number, activeIndex: number): number | null {
  if (groupCount <= 1) return null
  if (activeIndex < 0 || activeIndex >= groupCount) return 0
  return activeIndex + 1 < groupCount ? activeIndex + 1 : activeIndex - 1
}

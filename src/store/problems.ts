import { create } from 'zustand'
import type { LspDiagnostic } from '@/types'

export interface ProblemEntry { path: string; owner: string; diagnostics: LspDiagnostic[] }
interface ProblemsStore {
  /** path → owner → diagnostics */
  byPath: Record<string, Record<string, LspDiagnostic[]>>
  errors: number
  warnings: number
  infos: number
  set: (path: string, owner: string, diagnostics: LspDiagnostic[]) => void
  clearOwner: (owner: string) => void
  clearPath: (path: string) => void
  forPath: (path: string) => LspDiagnostic[]
}

function recount(byPath: ProblemsStore['byPath']) {
  let errors = 0, warnings = 0, infos = 0
  for (const owners of Object.values(byPath)) for (const list of Object.values(owners)) for (const d of list) {
    if (d.severity === 1 || d.severity === undefined) errors++
    else if (d.severity === 2) warnings++
    else infos++
  }
  return { errors, warnings, infos }
}

export const useProblems = create<ProblemsStore>((set, get) => ({
  byPath: {}, errors: 0, warnings: 0, infos: 0,
  set: (path, owner, diagnostics) => {
    const byPath = { ...get().byPath }
    const owners = { ...(byPath[path] ?? {}) }
    if (diagnostics.length) owners[owner] = diagnostics; else delete owners[owner]
    if (Object.keys(owners).length) byPath[path] = owners; else delete byPath[path]
    set({ byPath, ...recount(byPath) })
  },
  clearOwner: (owner) => {
    const byPath: ProblemsStore['byPath'] = {}
    for (const [p, owners] of Object.entries(get().byPath)) { const rest = { ...owners }; delete rest[owner]; if (Object.keys(rest).length) byPath[p] = rest }
    set({ byPath, ...recount(byPath) })
  },
  clearPath: (path) => { const byPath = { ...get().byPath }; delete byPath[path]; set({ byPath, ...recount(byPath) }) },
  forPath: (path) => Object.values(get().byPath[path] ?? {}).flat(),
}))

/** Compact text summary for prompts: `relpath:line:col [severity] message`. */
export function diagnosticsSummary(paths?: string[], max = 40): string {
  const { byPath } = useProblems.getState()
  const out: string[] = []
  for (const [p, owners] of Object.entries(byPath)) {
    if (paths && !paths.includes(p)) continue
    for (const list of Object.values(owners)) for (const d of list) {
      if ((d.severity ?? 1) > 2) continue
      out.push(`${p}:${d.range.start.line + 1}:${d.range.start.character + 1} [${d.severity === 2 ? 'warning' : 'error'}] ${d.message.split('\n')[0]}`)
      if (out.length >= max) return out.join('\n')
    }
  }
  return out.join('\n')
}

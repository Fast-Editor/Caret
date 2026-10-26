import { useUi } from '@/store/ui'
import { CommandPalette } from './CommandPalette'
import { QuickOpen } from './QuickOpen'
import { runCommand, hasCommand } from '@/lib/commands'
import { useEffect } from 'react'
export { parseQuickOpenInput, fuzzyScore } from './parse'
export { PaletteShell, PaletteItem, Highlight } from './PaletteShell'

/** Mount once: renders whichever palette is open. Symbols delegate to the editor module. */
export function PaletteHost() {
  const mode = useUi((s) => s.paletteOpen)
  const initial = useUi((s) => s.paletteInitial)
  const close = useUi((s) => s.closePalette)
  useEffect(() => {
    if (mode === 'symbols') {
      close()
      if (hasCommand('editor.goToSymbol')) void runCommand('editor.goToSymbol', { query: initial.replace(/^@/, '') })
    } else if (mode === 'search') {
      close()
      void runCommand('search.findInFiles', { query: initial.replace(/^#/, '') })
    }
  }, [mode, initial, close])
  if (mode === 'commands') return <CommandPalette key="cmd" initial={initial} />
  if (mode === 'files' || mode === 'recent') return <QuickOpen key="files" initial={initial} />
  return null
}

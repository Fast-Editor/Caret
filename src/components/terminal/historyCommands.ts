/** Commands for the persisted terminal command history (palette + clear). Registered from `registerShellCommands`. */
import { api } from '@/lib/api'
import { registerCommands } from '@/lib/commands'
import { confirm } from '@/components/dialogs'
import { notify } from '@/store/notifications'
import { useUi } from '@/store/ui'

export function registerTerminalHistoryCommands(): () => void {
  return registerCommands([
    { id: 'terminal.searchHistory', title: 'Terminal: Search Command History', category: 'Terminal', keybinding: 'mod+alt+h', run: () => useUi.getState().openPalette('terminalHistory') },
    { id: 'terminal.clearHistory', title: 'Terminal: Clear Command History', category: 'Terminal', run: async () => {
      const ok = await confirm({ title: 'Clear terminal command history?', message: 'Deletes the persisted history for every workspace (~/.caret/terminal/history.jsonl). This cannot be undone.', okLabel: 'Clear', danger: true })
      if (!ok) return
      try { await api.terminalHistory.clear(); notify('success', 'Terminal history cleared') } catch (e) { notify('error', 'Could not clear terminal history', String((e as Error).message ?? e)) }
    } },
  ])
}

import { useEffect, useState } from 'react'
import { KeyRound, Terminal } from 'lucide-react'
import { SchemaPage } from '../SchemaPage'
import { Button, Section, notify, errorMessage } from '../primitives'
import { api } from '@/lib/api'
import { useSettings } from '@/store/settings'
import { runCommand } from '@/lib/commands'
import type { CliStatus } from '@/types'

export function GeneralPage() {
  const [busy, setBusy] = useState(false)
  const [cli, setCli] = useState<CliStatus | null>(null)
  const [cliBusy, setCliBusy] = useState(false)
  const refreshCli = () => api.cli.status().then(setCli).catch(() => setCli(null))
  useEffect(() => { void refreshCli() }, [])
  const cliAction = async (id: 'cli.install' | 'cli.uninstall') => { setCliBusy(true); try { await runCommand(id) } finally { setCliBusy(false); void refreshCli() } }
  const importKeys = async () => {
    setBusy(true)
    try {
      const ids = await api.settings.importEnvKeys()
      notify(ids.length ? 'success' : 'info', ids.length ? `Imported ${ids.length} key${ids.length === 1 ? '' : 's'}` : 'No API keys found in environment', ids.length ? ids.join(', ') : 'Set ANTHROPIC_API_KEY, OPENAI_API_KEY, OPENROUTER_API_KEY, GEMINI_API_KEY… and relaunch from a terminal.')
      await useSettings.getState().load()
    } catch (e) { notify('error', 'Import failed', errorMessage(e)) } finally { setBusy(false) }
  }
  return (
    <SchemaPage page="general" before={
      <Section title="API keys" description="Keys are stored in ~/.caret/secrets.json (mode 600), never in settings.json. Manage per-provider keys on the Models page." actions={<Button onClick={importKeys} loading={busy}><KeyRound size={12} />Import from environment</Button>}>
        <div className="px-3 py-2.5 text-[11px] text-muted">Environment variables are read when Caret is launched from a terminal. Imported keys persist for Finder launches.</div>
      </Section>
    } after={
      <Section title="Command line" description="Install the `caret` command so `caret .` opens the current folder, like VS Code's `code .`." actions={
        cli?.installed
          ? <Button onClick={() => cliAction('cli.uninstall')} loading={cliBusy}><Terminal size={12} />Uninstall 'caret' command</Button>
          : <Button onClick={() => cliAction('cli.install')} loading={cliBusy}><Terminal size={12} />Install 'caret' command in PATH</Button>
      }>
        <div className="px-3 py-2.5 text-[11px] text-muted mono">{cli ? (cli.installed ? `${cli.path} → ${cli.scriptPath}` : cli.onPath ? `A different 'caret' is on your PATH${cli.target ? ` (${cli.target})` : ''}` : `Not installed. Script: ${cli.scriptPath}`) : '…'}</div>
      </Section>
    } />
  )
}

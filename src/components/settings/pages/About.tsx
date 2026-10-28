import { useEffect, useState } from 'react'
import { FolderOpen, Cpu, ExternalLink, RefreshCw, Download } from 'lucide-react'
import { checkForUpdates, installUpdate, type UpdateState } from '@/lib/updater'
import { api } from '@/lib/api'
import { runCommand, hasCommand } from '@/lib/commands'
import type { AppInfo, ProcessStats } from '@/types'
import { Section, Row, Button, formatBytes } from '../primitives'

export function AboutPage() {
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [stats, setStats] = useState<ProcessStats | null>(null)
  const [upd, setUpd] = useState<UpdateState>({ kind: 'idle' })
  const doCheck = async () => { setUpd({ kind: 'checking' }); setUpd(await checkForUpdates()) }
  const doInstall = async () => {
    if (upd.kind !== 'available') return
    const version = upd.version
    setUpd({ kind: 'downloading', version, received: 0 })
    try { await installUpdate((received, total) => setUpd({ kind: 'downloading', version, received, total })); setUpd({ kind: 'ready', version }) }
    catch (e) { setUpd({ kind: 'error', message: String((e as Error)?.message ?? e) }) }
  }
  useEffect(() => {
    api.app.info().then(setInfo).catch(() => {})
    const tick = () => api.app.processStats().then(setStats).catch(() => {})
    tick(); const id = window.setInterval(tick, 5000)
    return () => window.clearInterval(id)
  }, [])
  const openConfig = () => { if (hasCommand('app.openConfigFolder')) void runCommand('app.openConfigFolder'); else if (info) void api.app.showInFolder(info.configDir) }
  return (
    <>
      <Section title="Caret" description="A memory-efficient, Rust-powered AI code editor.">
        <Row title="Version"><span className="text-xs mono">{info ? `${info.version} · ${info.os}/${info.arch}${info.debug ? ' · debug' : ''}` : '—'}</span></Row>
        <Row title="WebView"><span className="text-xs mono">{info?.webview ?? '—'}</span></Row>
        <Row title="Config folder" description={info?.configDir}><Button onClick={openConfig}><FolderOpen size={12} />Open</Button></Row>
        <Row title="Updates" description={upd.kind === 'none' ? 'You are on the latest version.' : upd.kind === 'available' ? `Version ${upd.version} is available${upd.date ? ` (${upd.date.slice(0, 10)})` : ''}.` : upd.kind === 'downloading' ? `Downloading ${upd.version}… ${upd.total ? Math.round((upd.received / upd.total) * 100) + '%' : formatBytes(upd.received)}` : upd.kind === 'ready' ? 'Installed — restarting.' : upd.kind === 'error' ? `Updates unavailable: ${upd.message}` : 'Releases are published on GitHub; updates are verified with Caret\'s signing key.'}>
          <span className="flex items-center gap-1.5">
            {upd.kind === 'available' && <Button variant="primary" onClick={() => void doInstall()}><Download size={12} />Install and restart</Button>}
            <Button onClick={() => void doCheck()} disabled={upd.kind === 'checking' || upd.kind === 'downloading'}><RefreshCw size={12} className={upd.kind === 'checking' ? 'spin' : ''} />{upd.kind === 'checking' ? 'Checking…' : 'Check for updates'}</Button>
          </span>
        </Row>
        <Row title="Documentation"><Button onClick={() => void (hasCommand('help.docs') ? runCommand('help.docs') : api.app.openExternal('https://github.com/'))}><ExternalLink size={12} />Open docs</Button></Row>
      </Section>
      <Section title="Resource usage" description="Live stats for the Caret core process (the WebView renderer is a separate OS process).">
        <Row title={<span className="flex items-center gap-1.5"><Cpu size={12} />Memory (RSS)</span>}><span className="text-xs mono">{stats ? formatBytes(stats.rssBytes) : '—'}</span></Row>
        <Row title="CPU"><span className="text-xs mono">{stats ? `${stats.cpuPercent.toFixed(1)}%` : '—'}</span></Row>
        <Row title="Uptime"><span className="text-xs mono">{stats ? `${Math.floor(stats.uptimeSecs / 60)}m ${stats.uptimeSecs % 60}s` : '—'}</span></Row>
        <Row title="System memory"><span className="text-xs mono">{stats ? formatBytes(stats.totalMemoryBytes) : '—'}</span></Row>
      </Section>
    </>
  )
}

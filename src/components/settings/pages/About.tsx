import { useEffect, useState } from 'react'
import { FolderOpen, Cpu, ExternalLink } from 'lucide-react'
import { api } from '@/lib/api'
import { runCommand, hasCommand } from '@/lib/commands'
import type { AppInfo, ProcessStats } from '@/types'
import { Section, Row, Button, formatBytes } from '../primitives'

export function AboutPage() {
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [stats, setStats] = useState<ProcessStats | null>(null)
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

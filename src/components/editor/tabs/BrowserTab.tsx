import { useEffect, useState } from 'react'
import { Globe, ExternalLink, RefreshCw, Camera, X } from 'lucide-react'
import { api } from '@/lib/api'
import type { BrowserTab as BTab, BrowserSnapshot } from '@/types'

/** Tauri webviews are separate OS windows; this tab manages them and shows the agent's DOM snapshot. */
export function BrowserTab({ url: initialUrl, tabId }: { url?: string; tabId?: string }) {
  const [url, setUrl] = useState(initialUrl ?? 'https://')
  const [tabs, setTabs] = useState<BTab[]>([])
  const [selected, setSelected] = useState<string | undefined>(tabId)
  const [snap, setSnap] = useState<BrowserSnapshot | null>(null)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const refresh = async () => { try { const t = await api.browser.list(); setTabs(t); if (!selected && t[0]) setSelected(t[0].id) } catch { setTabs([]) } }
  useEffect(() => { void refresh() }, []) // eslint-disable-line react-hooks/exhaustive-deps
  const open = async () => { setErr(null); setBusy(true); try { const t = await api.browser.open(url.startsWith('http') ? url : `https://${url}`); setSelected(t.id); await refresh() } catch (e) { setErr(String((e as Error).message ?? e)) } finally { setBusy(false) } }
  const snapshot = async () => { if (!selected) return; setBusy(true); try { setSnap(await api.browser.snapshot(selected)) } catch (e) { setErr(String((e as Error).message ?? e)) } finally { setBusy(false) } }
  const close = async (id: string) => { try { await api.browser.close(id) } catch { /* ignore */ } setSnap(null); if (selected === id) setSelected(undefined); await refresh() }
  return (
    <div className="h-full flex flex-col bg-editor text-xs">
      <div className="h-9 shrink-0 flex items-center gap-2 px-2 border-b border-border">
        <Globe size={14} className="text-muted" />
        <input value={url} onChange={(e) => setUrl(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') void open() }} className="flex-1 h-6 px-2 rounded bg-input border border-border outline-none focus:border-accent" placeholder="https://" />
        <button className="px-2 py-0.5 rounded bg-accent text-accent-fg flex items-center gap-1" disabled={busy} onClick={() => void open()}><ExternalLink size={12} />Open window</button>
        <button className="p-1 rounded hover:bg-hover" title="Refresh list" onClick={() => void refresh()}><RefreshCw size={13} /></button>
      </div>
      <div className="flex-1 min-h-0 flex">
        <div className="w-56 shrink-0 border-r border-border overflow-auto p-1">
          <div className="px-2 py-1 text-faint uppercase tracking-wide text-[10px]">Browser windows</div>
          {tabs.map((t) => (
            <div key={t.id} className={`group flex items-center gap-1 px-2 h-7 rounded cursor-default ${selected === t.id ? 'bg-selected' : 'hover:bg-hover'}`} onClick={() => setSelected(t.id)}>
              <span className="truncate flex-1" title={t.url}>{t.title || t.url}</span>
              <button className="opacity-0 group-hover:opacity-100 p-0.5 rounded hover:bg-active" onClick={(e) => { e.stopPropagation(); void close(t.id) }}><X size={11} /></button>
            </div>
          ))}
          {!tabs.length && <div className="px-2 py-2 text-muted">No browser windows open. The agent's browser tools (navigate, snapshot, click, type) open windows here.</div>}
        </div>
        <div className="flex-1 min-w-0 overflow-auto p-3">
          {err && <div className="text-danger mb-2">{err}</div>}
          {selected && <button className="mb-3 px-2 py-0.5 rounded bg-active hover:bg-hover flex items-center gap-1" disabled={busy} onClick={() => void snapshot()}><Camera size={12} />Take DOM snapshot</button>}
          {snap && (
            <div className="space-y-3 selectable">
              <div><div className="text-faint">{snap.url}</div><div className="font-medium text-sm">{snap.title}</div></div>
              <div><div className="text-faint uppercase text-[10px] mb-1">Interactive elements ({snap.elements.length})</div>
                <div className="max-h-56 overflow-auto rounded border border-border">{snap.elements.slice(0, 200).map((e) => <div key={e.ref} className="flex gap-2 px-2 py-0.5 border-b border-border last:border-0"><span className="text-accent mono">{e.ref}</span><span className="text-muted">{e.tag}{e.role ? `[${e.role}]` : ''}</span><span className="truncate">{e.text || e.placeholder || e.href}</span></div>)}</div></div>
              <div><div className="text-faint uppercase text-[10px] mb-1">Page text</div><pre className="whitespace-pre-wrap mono text-[11px] max-h-96 overflow-auto rounded border border-border p-2">{snap.text.slice(0, 20000)}</pre></div>
              {snap.consoleLogs.length > 0 && <div><div className="text-faint uppercase text-[10px] mb-1">Console</div><pre className="mono text-[11px] max-h-40 overflow-auto rounded border border-border p-2">{snap.consoleLogs.slice(-50).join('\n')}</pre></div>}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

import { useEffect, useState } from 'react'
import { BookOpen, Pause, Play, Plus, RefreshCw, RotateCcw, Trash2, Search, ExternalLink, Layers } from 'lucide-react'
import { initIndexingStore, useIndexing } from '@/store/indexing'
import { useWorkspace } from '@/store/workspace'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import type { DocSearchHit, IndexStatus } from '@/types'
import { SchemaPage } from '../SchemaPage'
import { Section, Row, Button, IconButton, Badge, Dialog, Field, TextInput, NumberInput, Empty, confirmDialog, notify, errorMessage, formatBytes, relativeTime, Spinner } from '../primitives'

const STATE_TONE: Record<IndexStatus['state'], 'default' | 'accent' | 'success' | 'warning' | 'danger' | 'info'> = { disabled: 'default', idle: 'default', scanning: 'info', indexing: 'accent', embedding: 'accent', ready: 'success', error: 'danger', paused: 'warning' }

export function IndexingPage() {
  const status = useIndexing((s) => s.status)
  const docs = useIndexing((s) => s.docs)
  const loadDocs = useIndexing((s) => s.loadDocs)
  const ws = useWorkspace((s) => s.workspace)
  const [addOpen, setAddOpen] = useState(false)
  const [q, setQ] = useState(''); const [hits, setHits] = useState<DocSearchHit[] | null>(null); const [searching, setSearching] = useState(false)
  useEffect(() => { initIndexingStore() }, [])

  const act = async (f: () => Promise<void>, label: string) => { try { await f() } catch (e) { notify('error', `${label} failed`, errorMessage(e)) } }
  const busy = status && ['scanning', 'indexing', 'embedding'].includes(status.state)
  const openIgnore = async (name: string) => {
    if (!ws) return
    const p = `${ws.root}/${name}`
    try { if (!(await api.fs.exists(p))) await api.fs.writeFile(p, `# Paths excluded from Caret's index and the agent\n# Syntax: .gitignore\n`); await runCommand('editor.openFile', { path: p }) } catch (e) { notify('error', `Cannot open ${name}`, errorMessage(e)) }
  }
  const search = async () => { if (!q.trim()) return; setSearching(true); try { setHits(await api.docs.search(q.trim(), undefined, 8)) } catch (e) { notify('error', 'Search failed', errorMessage(e)) } finally { setSearching(false) } }

  return (
    <SchemaPage page="indexing"
      before={
        <Section title="Codebase index" description="Local BM25 (tantivy) plus optional embeddings power @Codebase and the codebase_search tool. Nothing leaves your machine unless an embeddings provider is configured."
          actions={<>
            {status?.state === 'paused' ? <Button onClick={() => act(api.index.resume, 'Resume')}><Play size={12} />Resume</Button> : busy ? <Button onClick={() => act(api.index.pause, 'Pause')}><Pause size={12} />Pause</Button> : <Button onClick={() => act(api.index.start, 'Start')} disabled={!ws}><Play size={12} />Sync now</Button>}
            <Button onClick={async () => { if (await confirmDialog({ title: 'Rebuild index?', message: 'All files will be re-chunked and re-embedded.' })) await act(api.index.rebuild, 'Rebuild') }} disabled={!ws}><RotateCcw size={12} />Rebuild</Button>
            <IconButton title="Delete index" onClick={async () => { if (await confirmDialog({ title: 'Delete the index for this workspace?', danger: true, confirmLabel: 'Delete' })) await act(api.index.clear, 'Clear') }}><Trash2 size={12} /></IconButton>
          </>}>
          {!ws && <Empty icon={<Layers size={22} />} title="Open a folder to index it" />}
          {ws && !status && <div className="px-3 py-4 text-xs text-muted flex items-center gap-2"><Spinner />Waiting for index status…</div>}
          {ws && status && (
            <>
              <div className="px-3 py-3">
                <div className="flex items-center gap-2 mb-2"><Badge tone={STATE_TONE[status.state]}>{status.state}</Badge>{status.error && <span className="text-[11px] text-danger">{status.error}</span>}{status.lastIndexedAt && <span className="text-[11px] text-muted">Last synced {relativeTime(status.lastIndexedAt)}</span>}</div>
                <div className="h-1.5 w-full rounded-full bg-hover overflow-hidden"><div className="h-full bg-accent transition-[width]" style={{ width: `${Math.round(Math.max(0, Math.min(1, status.progress)) * 100)}%` }} /></div>
                <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 mt-3 text-[11px]">
                  <Stat label="Files" value={`${status.indexedFiles.toLocaleString()} / ${status.totalFiles.toLocaleString()}`} />
                  <Stat label="Chunks" value={status.totalChunks.toLocaleString()} />
                  <Stat label="Embedded" value={status.embeddingsAvailable ? `${status.embeddedChunks.toLocaleString()} (${status.totalChunks ? Math.round((status.embeddedChunks / status.totalChunks) * 100) : 0}%)` : 'off — keyword search only'} />
                  <Stat label="On disk" value={formatBytes(status.sizeBytes)} />
                </div>
              </div>
              <Row title="Pending files" description="Changed files waiting to be re-indexed (updated by the file watcher)."><span className="text-xs mono">{status.pendingFiles}</span></Row>
            </>
          )}
        </Section>
      }
      after={
        <>
          <Section title="Ignore files" description="Caret honours .gitignore plus these files. .caretignore/.cursorignore also hide files from the agent; .cursorindexingignore only affects the index.">
            {['.caretignore', '.cursorignore', '.cursorindexingignore'].map((n) => <Row key={n} title={<span className="mono">{n}</span>}><Button size="sm" disabled={!ws} onClick={() => openIgnore(n)}><ExternalLink size={11} />Open / create</Button></Row>)}
          </Section>
          <Section title="Docs" description="Crawl documentation sites into a searchable index; reference them with @Docs." actions={<><IconButton title="Refresh list" onClick={() => void loadDocs()}><RefreshCw size={12} /></IconButton><Button variant="primary" onClick={() => setAddOpen(true)}><Plus size={12} />Add new doc</Button></>}>
            {docs.length === 0 && <Empty icon={<BookOpen size={22} />} title="No docs indexed" description="Add a documentation URL (e.g. https://docs.rs/tokio) and Caret will crawl pages under it." />}
            {docs.map((d) => (
              <div key={d.id} className="px-3 py-2 flex items-center gap-3">
                <div className="flex-1 min-w-0"><div className="text-xs font-medium flex items-center gap-2">{d.name}<Badge tone={d.status === 'ready' ? 'success' : d.status === 'error' ? 'danger' : 'info'}>{d.status}</Badge></div><div className="text-[11px] text-muted mono truncate-1">{d.url}</div><div className="text-[10px] text-faint">{d.pages} pages · {d.chunks} chunks{d.error ? ` · ${d.error}` : ''}</div></div>
                <div className="flex gap-0.5"><IconButton title="Re-crawl" onClick={() => act(() => api.docs.refresh(d.id).then(loadDocs), 'Refresh')}><RefreshCw size={12} /></IconButton><IconButton title="Remove" onClick={async () => { if (await confirmDialog({ title: `Remove docs “${d.name}”?`, danger: true, confirmLabel: 'Remove' })) await act(() => api.docs.remove(d.id).then(loadDocs), 'Remove') }}><Trash2 size={12} /></IconButton></div>
              </div>
            ))}
            {docs.length > 0 && (
              <div className="px-3 py-2.5">
                <div className="flex gap-1 max-w-[520px]"><TextInput value={q} onChange={setQ} placeholder="Test docs search…" onKeyDown={(e) => { if (e.key === 'Enter') void search() }} /><Button onClick={search} loading={searching}><Search size={12} />Search</Button></div>
                {hits && <div className="mt-2 space-y-1">{hits.length === 0 && <div className="text-[11px] text-muted">No results.</div>}{hits.map((h, i) => <button key={i} onClick={() => void api.app.openExternal(h.url)} className="block w-full text-left rounded border border-border p-2 hover:bg-hover"><div className="text-[11px] font-medium truncate-1">{h.title}</div><div className="text-[10px] text-muted truncate-1">{h.url}</div><div className="text-[11px] mt-0.5 line-clamp-2 text-muted">{h.snippet}</div></button>)}</div>}
              </div>
            )}
          </Section>
          <AddDocDialog open={addOpen} onClose={() => setAddOpen(false)} onAdded={loadDocs} />
        </>
      } />
  )
}

function Stat({ label, value }: { label: string; value: string }) { return <div><div className="text-[10px] uppercase tracking-wide text-faint">{label}</div><div className="mono">{value}</div></div> }

function AddDocDialog({ open, onClose, onAdded }: { open: boolean; onClose: () => void; onAdded: () => Promise<void> }) {
  const [url, setUrl] = useState(''); const [name, setName] = useState(''); const [maxPages, setMaxPages] = useState(200); const [prefix, setPrefix] = useState(''); const [busy, setBusy] = useState(false)
  const valid = /^https?:\/\/\S+/i.test(url.trim())
  const add = async () => { setBusy(true); try { await api.docs.add(url.trim(), name.trim() || undefined, { maxPages, prefix: prefix.trim() || undefined }); notify('success', 'Crawling docs', url); setUrl(''); setName(''); setPrefix(''); onClose(); await onAdded() } catch (e) { notify('error', 'Could not add docs', errorMessage(e)) } finally { setBusy(false) } }
  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) onClose() }} title="Add documentation" description="Pages are crawled in the background and chunked into the docs index." footer={<><Button onClick={onClose}>Cancel</Button><Button variant="primary" disabled={!valid} loading={busy} onClick={add}>Add</Button></>}>
      <Field label="Entry URL"><TextInput mono value={url} onChange={setUrl} placeholder="https://docs.rs/tokio/latest/tokio/" autoFocus /></Field>
      <div className="grid grid-cols-2 gap-x-4">
        <Field label="Name" hint="Shown as @Docs name"><TextInput value={name} onChange={setName} placeholder="tokio" /></Field>
        <Field label="Max pages"><NumberInput value={maxPages} onChange={setMaxPages} min={1} max={5000} /></Field>
      </div>
      <Field label="URL prefix filter" hint="Only crawl links starting with this prefix (defaults to the entry URL's directory)."><TextInput mono value={prefix} onChange={setPrefix} placeholder="https://docs.rs/tokio/latest/" /></Field>
    </Dialog>
  )
}

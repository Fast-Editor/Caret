import { useEffect, useRef, useState } from 'react'
import { Hammer, Pencil, Eye, Save, Loader2, CheckCircle2 } from 'lucide-react'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import { Markdown } from '../Markdown'
import { monaco, initMonaco, editorOptionsFromSettings } from '@/lib/monaco/setup'
import { useSettings } from '@/store/settings'
import { Events, on } from '@/lib/tauri'
import type { FsChangeEvent } from '@/types'

export function PlanTab({ path }: { path: string }) {
  const [text, setText] = useState('')
  const [editing, setEditing] = useState(false)
  const [building, setBuilding] = useState(false)
  const [dirty, setDirty] = useState(false)
  const hostRef = useRef<HTMLDivElement>(null)
  const edRef = useRef<monaco.editor.IStandaloneCodeEditor | null>(null)
  const load = async () => { try { setText(await api.plans.read(path)) } catch { try { setText((await api.fs.readFile(path)).content) } catch { setText('') } } }
  useEffect(() => { void load(); return on<FsChangeEvent>(Events.fsChanged, (e) => { if (e.paths.includes(path) && !editing) void load() }) }, [path]) // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (!editing) { edRef.current?.dispose(); edRef.current = null; return }
    initMonaco()
    const ed = monaco.editor.create(hostRef.current!, { ...editorOptionsFromSettings(useSettings.getState().settings?.ui), value: text, language: 'markdown', wordWrap: 'on', minimap: { enabled: false }, lineNumbers: 'off' })
    edRef.current = ed
    const d = ed.onDidChangeModelContent(() => setDirty(true))
    return () => { d.dispose(); ed.dispose() }
  }, [editing]) // eslint-disable-line react-hooks/exhaustive-deps
  const save = async () => { const v = edRef.current?.getValue() ?? text; await api.fs.writeFile(path, v); setText(v); setDirty(false) }
  const meta = /^---\n([\s\S]*?)\n---/.exec(text)?.[1] ?? ''
  const status = /status:\s*(\w+)/.exec(meta)?.[1]
  const title = /title:\s*(.*)/.exec(meta)?.[1] ?? path.split('/').pop()
  const body = text.replace(/^---\n[\s\S]*?\n---\n?/, '')
  const build = async () => {
    setBuilding(true)
    try { if (dirty) await save(); const sessionId = await api.plans.build(path); await api.plans.setStatus(path, 'building').catch(() => {}); void runCommand('chat.open', { sessionId }) }
    catch (e) { void runCommand('app.notify', { level: 'error', title: 'Build plan failed', message: String((e as Error).message ?? e) }) } finally { setBuilding(false) }
  }
  return (
    <div className="h-full flex flex-col bg-editor">
      <div className="h-9 shrink-0 flex items-center gap-2 px-3 border-b border-border text-xs">
        <span className="font-medium truncate">{title}</span>
        {status && <span className={`px-1.5 rounded ${status === 'done' ? 'bg-success/20 text-success' : status === 'approved' || status === 'building' ? 'bg-accent/20 text-accent' : 'bg-active text-muted'}`}>{status}</span>}
        <span className="flex-1" />
        {editing && dirty && <button className="px-2 py-0.5 rounded bg-active hover:bg-hover flex items-center gap-1" onClick={() => void save()}><Save size={12} />Save</button>}
        <button className="px-2 py-0.5 rounded bg-active hover:bg-hover flex items-center gap-1" onClick={() => setEditing((v) => !v)}>{editing ? <><Eye size={12} />Preview</> : <><Pencil size={12} />Edit</>}</button>
        {status !== 'done' && <button className="px-2 py-0.5 rounded bg-accent text-accent-fg flex items-center gap-1" disabled={building} onClick={() => void build()}>{building ? <Loader2 size={12} className="spin" /> : <Hammer size={12} />}Build plan</button>}
        {status === 'done' && <span className="flex items-center gap-1 text-success"><CheckCircle2 size={12} />Built</span>}
      </div>
      {editing ? <div ref={hostRef} className="flex-1 min-h-0" /> : <div className="flex-1 overflow-auto px-8 py-6 max-w-3xl"><Markdown text={body || '_Empty plan_'} /></div>}
    </div>
  )
}

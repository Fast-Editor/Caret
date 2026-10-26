import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import { Markdown } from '../Markdown'
import { getModel } from '@/store/editor'
import { subscribe } from '@/lib/bus'
import { Events, on } from '@/lib/tauri'
import { runCommand } from '@/lib/commands'
import type { FsChangeEvent } from '@/types'

export function MarkdownPreviewTab({ path }: { path: string }) {
  const [text, setText] = useState('')
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null
    const load = async () => { const m = getModel(path); if (m) setText(m.getValue()); else { try { setText((await api.fs.readFile(path)).content) } catch { setText('') } } }
    const schedule = () => { if (timer) clearTimeout(timer); timer = setTimeout(() => void load(), 200) }
    void load()
    const offs = [subscribe<{ path: string }>('editor:changed', (e) => { if (e.path === path) schedule() }), on<FsChangeEvent>(Events.fsChanged, (e) => { if (e.paths.includes(path)) schedule() })]
    return () => { if (timer) clearTimeout(timer); offs.forEach((o) => o()) }
  }, [path])
  const dir = path.slice(0, path.lastIndexOf('/'))
  return (
    <div className="h-full overflow-auto bg-editor">
      <div className="max-w-3xl mx-auto px-10 py-8">
        <Markdown text={text} onOpenLink={(href) => { if (/^https?:/.test(href)) void api.app.openExternal(href); else void runCommand('editor.openFile', { path: href.startsWith('/') ? href : `${dir}/${href.replace(/^\.\//, '')}`, preview: true }) }} />
      </div>
    </div>
  )
}

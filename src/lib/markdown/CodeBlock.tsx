import { memo, useEffect, useState } from 'react'
import { Check, Copy, CornerDownLeft, ExternalLink, LoaderCircle, WandSparkles } from 'lucide-react'
import { colorize, escapeHtml } from './highlight'
import { parseCodeHeader } from './parse'
import { runCommand, hasCommand } from '@/lib/commands'
import { api } from '@/lib/api'
import { useWorkspace } from '@/store/workspace'

export interface CodeBlockProps {
  className?: string
  code: string
  /** When true the block is still being streamed; highlighting is deferred. */
  streaming?: boolean
  sessionId?: string
  compact?: boolean
}

async function copyText(text: string) {
  try { const { writeText } = await import('@tauri-apps/plugin-clipboard-manager'); await writeText(text) } catch { try { await navigator.clipboard.writeText(text) } catch {} }
}

function resolvePath(p: string | undefined, root: string | null | undefined): string | undefined {
  if (!p) return undefined
  if (p.startsWith('/')) return p
  return root ? `${root}/${p}` : p
}

export const CodeBlock = memo(function CodeBlock({ className, code: rawCode, streaming, sessionId, compact }: CodeBlockProps) {
  const { language, path, code } = parseCodeHeader(className, rawCode)
  const root = useWorkspace((s) => s.workspace?.root)
  const [html, setHtml] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const [applying, setApplying] = useState<null | 'apply' | 'insert'>(null)
  const [error, setError] = useState<string | null>(null)
  const absPath = resolvePath(path, root)

  useEffect(() => {
    if (streaming) return
    let alive = true
    colorize(code, language).then((h) => { if (alive) setHtml(h) })
    return () => { alive = false }
  }, [code, language, streaming])

  const onCopy = async () => { await copyText(code); setCopied(true); setTimeout(() => setCopied(false), 1200) }
  const onInsert = async () => {
    if (!hasCommand('editor.insertAtCursor')) return
    setApplying('insert')
    try { await runCommand('editor.insertAtCursor', { text: code }) } finally { setApplying(null) }
  }
  const onApply = async () => {
    setApplying('apply'); setError(null)
    try {
      let target = absPath
      if (!target && hasCommand('editor.getActiveFile')) target = (await runCommand<{ path: string } | null>('editor.getActiveFile'))?.path
      if (!target) { setError('Open a file to apply to, or add a path header to the code block.'); return }
      const res = await api.ai.apply({ path: target, code, language: language || undefined, sessionId })
      if (res.method === 'create' || !hasCommand('editor.openDiff')) {
        void runCommand('app.notify', { level: 'success', title: 'Applied', message: `${res.method} → ${target.split('/').pop()}` })
        if (hasCommand('editor.openFile')) void runCommand('editor.openFile', { path: target })
      } else {
        await runCommand('editor.openDiff', { path: target, original: res.before, modified: res.after, title: `Apply: ${target.split('/').pop()}`, language: language || undefined, changeId: res.changeId, sessionId })
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally { setApplying(null) }
  }
  const onOpen = () => { if (absPath && hasCommand('editor.openFile')) void runCommand('editor.openFile', { path: absPath }) }

  const lines = code.split('\n').length
  return (
    <div className={`group/code my-2 rounded-md border border-border bg-editor overflow-hidden ${compact ? 'text-[11px]' : 'text-[12px]'}`}>
      <div className="flex items-center gap-2 px-2 h-7 bg-panel border-b border-border text-[11px] text-muted">
        {path ? (
          <button type="button" onClick={onOpen} title={absPath} className="truncate-1 hover:text-fg mono max-w-[60%] text-left">{path}</button>
        ) : (
          <span className="mono">{language || 'text'}</span>
        )}
        {path && language && <span className="text-faint">{language}</span>}
        <span className="text-faint">{lines} {lines === 1 ? 'line' : 'lines'}</span>
        <span className="flex-1" />
        <div className="flex items-center gap-0.5 opacity-70 group-hover/code:opacity-100">
          {!streaming && (
            <button type="button" onClick={onApply} disabled={!!applying} title="Apply to file (uses the path header or the active file)" className="flex items-center gap-1 px-1.5 h-5 rounded hover:bg-hover text-accent disabled:opacity-50">
              {applying === 'apply' ? <LoaderCircle size={12} className="spin" /> : <WandSparkles size={12} />}<span>Apply</span>
            </button>
          )}
          {hasCommand('editor.insertAtCursor') && !streaming && (
            <button type="button" onClick={onInsert} disabled={!!applying} title="Insert at cursor" className="flex items-center gap-1 px-1.5 h-5 rounded hover:bg-hover"><CornerDownLeft size={12} /></button>
          )}
          {absPath && <button type="button" onClick={onOpen} title="Open file" className="flex items-center px-1.5 h-5 rounded hover:bg-hover"><ExternalLink size={12} /></button>}
          <button type="button" onClick={onCopy} title="Copy" className="flex items-center gap-1 px-1.5 h-5 rounded hover:bg-hover">{copied ? <Check size={12} className="text-success" /> : <Copy size={12} />}</button>
        </div>
      </div>
      <pre className="m-0 p-2.5 overflow-x-auto selectable mono leading-[1.5]" style={{ tabSize: 2 }}>
        {html !== null && !streaming
          ? <code className={`language-${language}`} dangerouslySetInnerHTML={{ __html: html }} />
          : <code className={`language-${language}`} dangerouslySetInnerHTML={{ __html: escapeHtml(code) }} />}
      </pre>
      {error && <div className="px-2 py-1 text-[11px] text-danger border-t border-border">{error}</div>}
    </div>
  )
})

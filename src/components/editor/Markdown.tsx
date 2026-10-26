import { useEffect, useRef, useState, type ReactNode } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { Copy, Check, FilePlus2 } from 'lucide-react'
import { monaco, initMonaco } from '@/lib/monaco/setup'
import { runCommand } from '@/lib/commands'

/** Markdown renderer for previews/answers; code blocks are colorized with Monaco's tokenizer. */
export function Markdown({ text, onOpenLink, showApply }: { text: string; onOpenLink?: (href: string) => void; showApply?: boolean }) {
  return (
    <div className="caret-md text-[13px]">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={{
        a: ({ href, children }) => <a href={href} onClick={(e) => { e.preventDefault(); if (href) (onOpenLink ?? ((h: string) => void runCommand('app.openExternal', { url: h })))(href) }}>{children}</a>,
        code: ({ className, children, ...rest }) => {
          const lang = /language-([\w+-]+)/.exec(className ?? '')?.[1]
          const code = String(children ?? '').replace(/\n$/, '')
          const inline = !className && !code.includes('\n')
          if (inline) return <code {...rest}>{children}</code>
          return <CodeBlock code={code} language={lang} showApply={showApply} />
        },
        pre: ({ children }) => <>{children}</>,
      }}>{text}</ReactMarkdown>
    </div>
  )
}

export function CodeBlock({ code, language, showApply }: { code: string; language?: string; showApply?: boolean }) {
  const ref = useRef<HTMLElement>(null)
  const [copied, setCopied] = useState(false)
  useEffect(() => {
    let alive = true
    initMonaco()
    const el = ref.current; if (!el) return
    el.textContent = code
    if (language && code.length < 60_000) monaco.editor.colorize(code, language, { tabSize: 2 }).then((html) => { if (alive && ref.current) ref.current.innerHTML = html }).catch(() => {})
    return () => { alive = false }
  }, [code, language])
  const pathHint = /^(?:\/\/|#|--)\s*([\w./-]+\.[a-z]{1,6})\s*$/m.exec(code.split('\n')[0] ?? '')?.[1]
  return (
    <div className="relative group">
      <div className="absolute right-1 top-1 flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
        {showApply && <button className="px-1.5 py-0.5 rounded bg-active text-[11px] flex items-center gap-1 hover:bg-hover" title="Apply to file" onClick={() => void runCommand('ai.apply', { code, language, path: pathHint })}><FilePlus2 size={11} />Apply</button>}
        <button className="px-1.5 py-0.5 rounded bg-active text-[11px] flex items-center gap-1 hover:bg-hover" onClick={() => { void navigator.clipboard.writeText(code); setCopied(true); setTimeout(() => setCopied(false), 1200) }}>{copied ? <Check size={11} /> : <Copy size={11} />}{copied ? 'Copied' : 'Copy'}</button>
      </div>
      {language && <span className="absolute left-2 top-1 text-[10px] text-faint">{language}</span>}
      <pre className="pt-5"><code ref={ref} className="mono" /></pre>
    </div>
  )
}
export type { ReactNode }

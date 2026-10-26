import { memo, useMemo, type ReactElement, type ReactNode } from 'react'
import ReactMarkdown, { type Components } from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { CodeBlock } from './CodeBlock'
import { parsePathRef } from './parse'
import { api } from '@/lib/api'
import { runCommand, hasCommand } from '@/lib/commands'
import { useWorkspace } from '@/store/workspace'

export { CodeBlock } from './CodeBlock'
export { parseCodeHeader, parsePathRef, parseUnifiedDiff, diffStats } from './parse'

export interface MarkdownProps { text: string; streaming?: boolean; sessionId?: string; className?: string; compact?: boolean }

const remarkPlugins = [remarkGfm]

function InlineCode({ children }: { children?: ReactNode }) {
  const text = typeof children === 'string' ? children : Array.isArray(children) ? children.join('') : String(children ?? '')
  const ref = parsePathRef(text)
  const root = useWorkspace((s) => s.workspace?.root)
  if (ref && hasCommand('editor.openFile')) {
    const abs = ref.path.startsWith('/') ? ref.path : root ? `${root}/${ref.path}` : ref.path
    return <code className="mono text-[0.92em] px-1 py-px rounded bg-input border border-border cursor-pointer hover:border-accent text-accent" title={`Open ${abs}`} onClick={() => void runCommand('editor.openFile', { path: abs, line: ref.line })}>{text}</code>
  }
  return <code className="mono text-[0.92em] px-1 py-px rounded bg-input border border-border">{children}</code>
}

function makeComponents(streaming: boolean | undefined, sessionId: string | undefined, compact: boolean | undefined): Components {
  return {
    pre({ children }) {
      const child = (Array.isArray(children) ? children[0] : children) as ReactElement<{ className?: string; children?: ReactNode }> | undefined
      if (child && typeof child === 'object' && 'props' in child) {
        const code = String(child.props.children ?? '').replace(/\n$/, '')
        return <CodeBlock className={child.props.className} code={code} streaming={streaming} sessionId={sessionId} compact={compact} />
      }
      return <pre>{children}</pre>
    },
    code({ className, children, ...rest }) {
      // Block code is handled by `pre`; anything that reaches here is inline.
      void rest
      if (className && /language-/.test(className)) return <code className={className}>{children}</code>
      return <InlineCode>{children}</InlineCode>
    },
    a({ href, children }) {
      return <a href={href} className="text-accent underline underline-offset-2 hover:text-accent-hover" onClick={(e) => { e.preventDefault(); if (href) void api.app.openExternal(href) }}>{children}</a>
    },
    table({ children }) { return <div className="my-2 overflow-x-auto"><table className="text-[12px] border-collapse [&_th]:border [&_th]:border-border [&_th]:px-2 [&_th]:py-1 [&_th]:bg-panel [&_td]:border [&_td]:border-border [&_td]:px-2 [&_td]:py-1">{children}</table></div> },
    ul({ children, className }) { return <ul className={`my-1.5 pl-5 list-disc space-y-0.5 ${className?.includes('contains-task-list') ? 'list-none pl-1' : ''}`}>{children}</ul> },
    ol({ children }) { return <ol className="my-1.5 pl-5 list-decimal space-y-0.5">{children}</ol> },
    li({ children, className }) { return <li className={className?.includes('task-list-item') ? 'flex items-start gap-1.5' : ''}>{children}</li> },
    input(props) { return <input {...props} disabled className="mt-1 accent-[var(--accent)]" /> },
    h1({ children }) { return <h1 className="text-[1.25em] font-semibold mt-3 mb-1.5">{children}</h1> },
    h2({ children }) { return <h2 className="text-[1.15em] font-semibold mt-3 mb-1.5">{children}</h2> },
    h3({ children }) { return <h3 className="text-[1.05em] font-semibold mt-2.5 mb-1">{children}</h3> },
    h4({ children }) { return <h4 className="font-semibold mt-2 mb-1">{children}</h4> },
    p({ children }) { return <p className="my-1.5 leading-[1.6]">{children}</p> },
    blockquote({ children }) { return <blockquote className="my-2 pl-3 border-l-2 border-border-strong text-muted">{children}</blockquote> },
    hr() { return <hr className="my-3 border-border" /> },
    strong({ children }) { return <strong className="font-semibold text-fg">{children}</strong> },
  }
}

/** Markdown renderer for chat. Memoized; during streaming the parent throttles `text` updates. */
export const Markdown = memo(function Markdown({ text, streaming, sessionId, className, compact }: MarkdownProps) {
  const components = useMemo(() => makeComponents(streaming, sessionId, compact), [streaming, sessionId, compact])
  return (
    <div className={`selectable break-words [&>*:first-child]:mt-0 [&>*:last-child]:mb-0 ${className ?? ''}`}>
      <ReactMarkdown remarkPlugins={remarkPlugins} components={components}>{text}</ReactMarkdown>
    </div>
  )
})

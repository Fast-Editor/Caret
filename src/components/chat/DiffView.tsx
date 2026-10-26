import { memo, useMemo } from 'react'
import { parseUnifiedDiff } from '@/lib/markdown/parse'

export const DiffView = memo(function DiffView({ diff, maxLines = 400, className }: { diff: string; maxLines?: number; className?: string }) {
  const lines = useMemo(() => parseUnifiedDiff(diff), [diff])
  const shown = lines.slice(0, maxLines)
  return (
    <div className={`mono text-[11.5px] leading-[1.45] overflow-x-auto rounded-md border border-border bg-editor selectable ${className ?? ''}`}>
      <table className="w-full border-collapse">
        <tbody>
          {shown.map((l, i) => {
            if (l.kind === 'meta') return null
            if (l.kind === 'hunk') return <tr key={i} className="bg-panel text-faint"><td colSpan={3} className="px-2 py-0.5">{l.text}</td></tr>
            const bg = l.kind === 'add' ? 'bg-diff-add' : l.kind === 'del' ? 'bg-diff-del' : ''
            const fg = l.kind === 'add' ? 'text-diff-add-fg' : l.kind === 'del' ? 'text-diff-del-fg' : ''
            return (
              <tr key={i} className={bg}>
                <td className="w-9 pr-1 text-right text-faint select-none align-top">{l.oldNo ?? ''}</td>
                <td className="w-9 pr-2 text-right text-faint select-none align-top">{l.newNo ?? ''}</td>
                <td className={`pr-3 whitespace-pre ${fg}`}><span className="inline-block w-3 select-none text-faint">{l.kind === 'add' ? '+' : l.kind === 'del' ? '−' : ' '}</span>{l.text}</td>
              </tr>
            )
          })}
        </tbody>
      </table>
      {lines.length > maxLines && <div className="px-2 py-1 text-faint text-[11px] border-t border-border">… {lines.length - maxLines} more lines</div>}
    </div>
  )
})

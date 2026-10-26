import { runCommand } from '@/lib/commands'
const rows: [string, string][] = [['Show All Commands', '⌘⇧P'], ['Go to File', '⌘P'], ['Find in Files', '⌘⇧F'], ['Open Chat', '⌘L'], ['New Agent', '⌘I'], ['Inline Edit', '⌘K'], ['Toggle Terminal', '⌃`']]
export function EmptyState() {
  return (
    <div className="h-full w-full flex flex-col items-center justify-center text-muted select-none bg-editor" onDoubleClick={() => void runCommand('file.new')}>
      <div className="text-5xl font-semibold tracking-tight text-faint mb-6">^</div>
      <div className="grid grid-cols-[auto_auto] gap-x-6 gap-y-1.5 text-xs">
        {rows.map(([l, k]) => <div key={l} className="contents"><span className="text-right">{l}</span><span className="text-faint"><kbd className="px-1 rounded bg-input border border-border text-[10px]">{k}</kbd></span></div>)}
      </div>
    </div>
  )
}

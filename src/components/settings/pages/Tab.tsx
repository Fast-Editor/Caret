import { Zap } from 'lucide-react'
import { SchemaPage } from '../SchemaPage'

export function TabPage() {
  return (
    <SchemaPage page="tab" before={
      <div className="mb-5 rounded-md border border-border bg-panel/60 p-3 flex gap-3">
        <Zap size={18} className="text-accent shrink-0 mt-0.5" />
        <div className="text-[11px] text-muted leading-relaxed">
          <div className="text-xs font-medium text-fg mb-0.5">Tab predicts your next edit</div>
          Suggestions appear as ghost text. <kbd className="mono px-1 rounded bg-hover border border-border">Tab</kbd> accepts, <kbd className="mono px-1 rounded bg-hover border border-border">⌘→</kbd> accepts a word, <kbd className="mono px-1 rounded bg-hover border border-border">Esc</kbd> dismisses. After an edit, Tab can also jump to the predicted next location. Use a fast, cheap model here — it is called on nearly every pause in typing.
        </div>
      </div>
    } />
  )
}

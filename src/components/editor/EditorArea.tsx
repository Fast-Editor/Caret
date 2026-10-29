import { useEffect } from 'react'
import { Group, Panel, Separator } from 'react-resizable-panels'
import { useEditor } from '@/store/editor'
import { EditorGroupView } from './EditorGroup'
import { ConfirmCloseDialog } from './ConfirmCloseDialog'
import { SymbolPicker } from './SymbolPicker'
import { initEditorModule } from './commands'
import './editor.css'

/** Editor groups side by side with resizable separators. Fills its container. */
export function EditorArea() {
  const groups = useEditor((s) => s.groups)
  useEffect(() => { initEditorModule() }, [])
  return (
    <div className="h-full w-full min-w-0 min-h-0 relative">
      {groups.length === 1 ? <EditorGroupView group={groups[0]} /> : (
        <Group orientation="horizontal" className="h-full w-full">
          {groups.map((g, i) => (
            <Panel key={g.id} id={g.id} minSize="15%" defaultSize={`${100 / groups.length}%`} className="min-w-0">
              {i > 0 && null}
              <EditorGroupView group={g} />
            </Panel>
          )).flatMap((p, i) => (i === 0 ? [p] : [<Separator key={`sep-${i}`} className="w-px bg-transparent hover:bg-border-strong data-[separator=active]:bg-border-strong transition-colors cursor-col-resize" />, p]))}
        </Group>
      )}
      <ConfirmCloseDialog />
      <SymbolPicker />
    </div>
  )
}

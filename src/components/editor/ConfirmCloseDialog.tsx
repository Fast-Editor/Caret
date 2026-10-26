import { useEffect } from 'react'
import { useEditor } from '@/store/editor'

export function ConfirmCloseDialog() {
  const cc = useEditor((s) => s.confirmClose)
  useEffect(() => {
    if (!cc) return
    const key = (e: KeyboardEvent) => { if (e.key === 'Escape') cc.resolve('cancel'); if (e.key === 'Enter') cc.resolve('save'); if ((e.metaKey || e.ctrlKey) && e.key === 'd') cc.resolve('discard') }
    window.addEventListener('keydown', key, true); return () => window.removeEventListener('keydown', key, true)
  }, [cc])
  if (!cc) return null
  return (
    <div className="fixed inset-0 z-[1100] flex items-start justify-center pt-24 bg-black/30" onMouseDown={() => cc.resolve('cancel')}>
      <div className="w-[420px] rounded-md border border-border bg-panel shadow-[var(--shadow)] p-4 text-sm" onMouseDown={(e) => e.stopPropagation()}>
        <div className="font-medium mb-1">Do you want to save the changes you made to {cc.title}?</div>
        <div className="text-muted text-xs mb-4">Your changes will be lost if you don't save them.</div>
        <div className="flex justify-end gap-2 text-xs">
          <button className="px-3 py-1 rounded bg-active hover:bg-hover" onClick={() => cc.resolve('discard')}>Don't Save</button>
          <button className="px-3 py-1 rounded bg-active hover:bg-hover" onClick={() => cc.resolve('cancel')}>Cancel</button>
          <button className="px-3 py-1 rounded bg-accent text-accent-fg" autoFocus onClick={() => cc.resolve('save')}>Save</button>
        </div>
      </div>
    </div>
  )
}

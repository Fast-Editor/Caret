import { useEffect, useRef, useState } from 'react'
import { X, Pin, Circle, SplitSquareHorizontal, AlertTriangle, Settings2, Sparkles, FileText, Image as ImageIcon, GitCompare, Globe, ListChecks, Keyboard, Home, ShieldCheck } from 'lucide-react'
import { useEditor, type EditorGroup, type EditorTab } from '@/store/editor'
import { relPath } from '@/store/workspace'
import { fileIconFor } from '@/components/explorer/fileIcons'
import { tabDirHint } from '@/lib/tabHints'
import { runCommand } from '@/lib/commands'
import { clsx } from 'clsx'

function TabIcon({ tab }: { tab: EditorTab }) {
  const cls = 'shrink-0 text-muted'
  switch (tab.kind) {
    case 'settings': return <Settings2 size={13} className={cls} />
    case 'welcome': return <Home size={13} className={cls} />
    case 'plan': return <ListChecks size={13} className={cls} />
    case 'image': return <ImageIcon size={13} className={cls} />
    case 'diff': return <GitCompare size={13} className={cls} />
    case 'browser': return <Globe size={13} className={cls} />
    case 'keybindings': return <Keyboard size={13} className={cls} />
    case 'review': return <ShieldCheck size={13} className={cls} />
    case 'markdown-preview': return <Sparkles size={13} className={cls} />
    default: return tab.path
      ? <span className="shrink-0 inline-flex items-center justify-center size-3.5 [&_svg]:size-3.5 [&>span]:size-3.5 [&>span]:text-[7px]">{fileIconFor(tab.title, false)}</span>
      : <FileText size={13} className={cls} />
  }
}

export function TabBar({ group, active }: { group: EditorGroup; active: boolean }) {
  const { setActive, closeTab, keepTab, reorderTab, moveTab, pinTab, closeOthers, closeToRight, closeAll, split } = useEditor()
  const groupsCount = useEditor((s) => s.groups.length)
  const [menu, setMenu] = useState<{ x: number; y: number; tab: EditorTab } | null>(null)
  const scrollRef = useRef<HTMLDivElement>(null)
  const dragIdx = useRef<number | null>(null)
  useEffect(() => { const el = scrollRef.current?.querySelector('[data-active="true"]'); el?.scrollIntoView({ block: 'nearest', inline: 'nearest' }) }, [group.activeTabId])
  useEffect(() => { if (!menu) return; const close = () => setMenu(null); window.addEventListener('mousedown', close, true); window.addEventListener('keydown', close, true); return () => { window.removeEventListener('mousedown', close, true); window.removeEventListener('keydown', close, true) } }, [menu])
  const dupTitles = new Map<string, number>(); for (const t of group.tabs) dupTitles.set(t.title, (dupTitles.get(t.title) ?? 0) + 1)
  return (
    <div className={clsx('h-[35px] shrink-0 flex items-stretch bg-panel', !active && 'opacity-90')} onDoubleClick={(e) => { if (e.target === e.currentTarget) void runCommand('file.new') }}>
      <div ref={scrollRef} className="tabbar flex-1 flex items-stretch overflow-x-auto overflow-y-hidden min-w-0" onWheel={(e) => { if (e.deltaY && !e.deltaX) { e.currentTarget.scrollLeft += e.deltaY } }}>
        {group.tabs.map((tab, i) => {
          const isActive = tab.id === group.activeTabId
          const hint = tabDirHint(tab.path, dupTitles.get(tab.title) ?? 0, relPath)
          return (
            <div key={tab.id} data-active={isActive} draggable
              onDragStart={(e) => { dragIdx.current = i; e.dataTransfer.setData('text/caret-tab', JSON.stringify({ group: group.id, tabId: tab.id })); e.dataTransfer.effectAllowed = 'move' }}
              onDragOver={(e) => { e.preventDefault(); e.dataTransfer.dropEffect = 'move' }}
              onDrop={(e) => { e.preventDefault(); const raw = e.dataTransfer.getData('text/caret-tab'); if (!raw) return; const { group: g, tabId } = JSON.parse(raw) as { group: string; tabId: string }; if (g === group.id) { const from = group.tabs.findIndex((t) => t.id === tabId); if (from >= 0) reorderTab(group.id, from, i) } else moveTab(g, tabId, group.id, i) }}
              onMouseDown={(e) => { if (e.button === 1) { e.preventDefault(); void closeTab(group.id, tab.id) } else if (e.button === 0) setActive(group.id, tab.id) }}
              onDoubleClick={(e) => { e.stopPropagation(); keepTab(group.id, tab.id) }}
              onContextMenu={(e) => { e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY, tab }) }}
              title={tab.path ?? tab.title}
              className={clsx('group relative flex items-center gap-1.5 pl-3 pr-1.5 min-w-[110px] max-w-[220px] border-r border-b border-r-border text-xs select-none', isActive ? 'bg-editor text-fg border-b-editor' : 'bg-panel text-muted hover:bg-hover border-b-border')}>
              <TabIcon tab={tab} />
              <span className={clsx('truncate-1 flex-1', tab.preview && 'italic', tab.conflict && 'text-warning')}>{tab.title}{hint && <span className="text-faint ml-1 not-italic">{hint}</span>}</span>
              {tab.conflict && <AlertTriangle size={11} className="text-warning" />}
              {tab.pinned ? <Pin size={11} className="text-faint" /> : (
                <button className={clsx('w-5 h-5 flex items-center justify-center rounded hover:bg-active', !tab.dirty && !isActive && 'opacity-0 group-hover:opacity-100')} onMouseDown={(e) => e.stopPropagation()} onClick={(e) => { e.stopPropagation(); void closeTab(group.id, tab.id) }} title={tab.dirty ? 'Unsaved changes' : 'Close'}>
                  {tab.dirty ? <Circle size={9} className="fill-current group-hover:hidden" /> : null}
                  <X size={12} className={clsx(tab.dirty && 'hidden group-hover:block')} />
                </button>
              )}
            </div>
          )
        })}
        <div className="flex-1 min-w-[40px] border-b border-border" onDragOver={(e) => e.preventDefault()} onDrop={(e) => { const raw = e.dataTransfer.getData('text/caret-tab'); if (!raw) return; const { group: g, tabId } = JSON.parse(raw) as { group: string; tabId: string }; if (g === group.id) { const from = group.tabs.findIndex((t) => t.id === tabId); reorderTab(group.id, from, group.tabs.length - 1) } else moveTab(g, tabId, group.id) }} />
      </div>
      <div className="flex items-center gap-0.5 px-1 border-l border-b border-border">
        {groupsCount < 3 && <button className="p-1 rounded hover:bg-hover text-muted" title="Split editor right (⌘\)" onClick={() => split()}><SplitSquareHorizontal size={14} /></button>}
      </div>
      {menu && (
        <div className="fixed z-[1000] min-w-[200px] py-1 rounded-md border border-border bg-tooltip text-xs shadow-[var(--shadow)]" style={{ top: menu.y, left: Math.min(menu.x, window.innerWidth - 220) }} onMouseDown={(e) => e.stopPropagation()}>
          {[
            { l: 'Close', r: () => closeTab(group.id, menu.tab.id), k: '⌘W' },
            { l: 'Close Others', r: () => closeOthers(group.id, menu.tab.id) },
            { l: 'Close to the Right', r: () => closeToRight(group.id, menu.tab.id) },
            { l: 'Close All', r: () => closeAll(group.id), sep: true },
            { l: menu.tab.pinned ? 'Unpin' : 'Pin', r: () => pinTab(group.id, menu.tab.id, !menu.tab.pinned) },
            { l: 'Keep Open', r: () => keepTab(group.id, menu.tab.id), hide: !menu.tab.preview },
            { l: 'Split Right', r: () => { setActive(group.id, menu.tab.id); split() }, sep: true, hide: groupsCount >= 3 },
            { l: 'Copy Path', r: () => navigator.clipboard.writeText(menu.tab.path ?? ''), hide: !menu.tab.path, sep: true },
            { l: 'Copy Relative Path', r: () => navigator.clipboard.writeText(relPath(menu.tab.path ?? '')), hide: !menu.tab.path },
            { l: 'Reveal in Explorer', r: () => runCommand('view.explorer', { reveal: menu.tab.path }), hide: !menu.tab.path },
            { l: 'Reveal in Finder', r: () => runCommand('file.revealInFinder', { path: menu.tab.path }), hide: !menu.tab.path },
            { l: 'Open Preview', r: () => runCommand('editor.markdownPreview', { path: menu.tab.path }), hide: !menu.tab.path?.endsWith('.md') },
          ].filter((it) => !it.hide).map((it, i) => (
            <div key={i}>{it.sep && i > 0 && <div className="my-1 border-t border-border" />}<button className="w-full text-left px-3 h-[26px] hover:bg-hover flex items-center" onClick={() => { setMenu(null); void it.r() }}><span className="flex-1">{it.l}</span>{it.k && <span className="text-faint">{it.k}</span>}</button></div>
          ))}
        </div>
      )}
    </div>
  )
}

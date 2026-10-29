import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { X, Pin, Circle, SplitSquareHorizontal, AlertTriangle, Settings2, Sparkles, FileText, Image as ImageIcon, GitCompare, Globe, ListChecks, Keyboard, Home, ShieldCheck } from 'lucide-react'
import { useEditor, type EditorGroup, type EditorTab } from '@/store/editor'
import { relPath } from '@/store/workspace'
import { fileIconFor } from '@/components/explorer/fileIcons'
import { tabDirHint } from '@/lib/tabHints'
import { runCommand } from '@/lib/commands'
import { duration } from '@/lib/motion'
import { flipDeltas, playFlip, snapshotPositions } from '@/lib/flip'
import { clsx } from 'clsx'
import './tabbar.css'

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

/** Tabs that were just closed stay in the strip for one exit animation, then drop out. */
function useLeavingTabs(tabs: EditorTab[]): { tabs: EditorTab[]; leaving: Set<string> } {
  const prev = useRef<EditorTab[]>(tabs)
  // Closed tabs stay mounted for one exit animation. Each ghost owns its removal timer: tying the timer to the effect
  // cleanup (the previous version) meant any further tab-list change within ~140 ms cancelled it, and closing a tab
  // always causes such a change, so ghosts accumulated at the end of the strip.
  const [leaving, setLeaving] = useState<Map<string, { tab: EditorTab; index: number }>>(new Map())
  const timers = useRef<Map<string, number>>(new Map())
  useEffect(() => {
    const ms = duration('fast')
    const ids = new Set(tabs.map((t) => t.id))
    const gone = prev.current.map((t, index) => ({ tab: t, index })).filter((g) => !ids.has(g.tab.id))
    prev.current = tabs
    // A tab that is back in the list (reopened, moved between groups) must not linger as a ghost.
    setLeaving((m) => { if (![...m.keys()].some((id) => ids.has(id))) return m; const n = new Map(m); for (const id of ids) { n.delete(id); const t = timers.current.get(id); if (t) { window.clearTimeout(t); timers.current.delete(id) } } return n })
    if (!gone.length || ms === 0) return
    setLeaving((m) => { const n = new Map(m); for (const g of gone) n.set(g.tab.id, g); return n })
    for (const g of gone) {
      const old = timers.current.get(g.tab.id); if (old) window.clearTimeout(old)
      timers.current.set(g.tab.id, window.setTimeout(() => { timers.current.delete(g.tab.id); setLeaving((m) => { if (!m.has(g.tab.id)) return m; const n = new Map(m); n.delete(g.tab.id); return n }) }, ms + 20))
    }
  }, [tabs])
  useEffect(() => () => { for (const t of timers.current.values()) window.clearTimeout(t); timers.current.clear() }, [])
  if (!leaving.size) return { tabs, leaving: new Set() }
  // Re-insert leaving tabs at the index they had when they left so neighbours slide rather than jump.
  const merged = [...tabs]
  for (const g of [...leaving.values()].sort((a, b) => a.index - b.index)) merged.splice(Math.min(g.index, merged.length), 0, g.tab)
  return { tabs: merged, leaving: new Set(leaving.keys()) }
}

export function TabBar({ group, active }: { group: EditorGroup; active: boolean }) {
  const { setActive, closeTab, keepTab, reorderTab, moveTab, pinTab, closeOthers, closeToRight, closeAll, split } = useEditor()
  const groupsCount = useEditor((s) => s.groups.length)
  const [menu, setMenu] = useState<{ x: number; y: number; tab: EditorTab } | null>(null)
  const scrollRef = useRef<HTMLDivElement>(null)
  const dragIdx = useRef<number | null>(null)
  const { tabs: shownTabs, leaving } = useLeavingTabs(group.tabs)
  // FLIP: snapshot tab x-positions before React re-lays them out, then animate from the old spot to the new one.
  const positions = useRef<Record<string, number>>({})
  const orderKey = group.tabs.map((t) => t.id).join('|')
  useLayoutEffect(() => {
    const el = scrollRef.current
    const next = snapshotPositions(el, 'x')
    playFlip(el, flipDeltas(positions.current, next), 'x', duration('base'))
    positions.current = next
  }, [orderKey])
  // Sliding active underline: position it under the active tab whenever the active tab or the layout changes.
  const [underline, setUnderline] = useState<{ left: number; width: number } | null>(null)
  useLayoutEffect(() => {
    const strip = scrollRef.current; const el = strip?.querySelector<HTMLElement>('[data-active="true"]')
    if (!strip || !el) { setUnderline(null); return }
    setUnderline({ left: el.offsetLeft, width: el.offsetWidth })
  }, [group.activeTabId, orderKey])
  useEffect(() => { const el = scrollRef.current?.querySelector('[data-active="true"]'); el?.scrollIntoView({ block: 'nearest', inline: 'nearest' }) }, [group.activeTabId])
  const menuRef = useRef<HTMLDivElement>(null)
  // Close on an outside press or Escape. The listener is capture-phase, so a press *inside* the menu must be ignored
  // here: closing on it would unmount the item before its click fired, and no menu action would ever run.
  useEffect(() => {
    if (!menu) return
    const down = (e: MouseEvent) => { if (menuRef.current && e.target instanceof Node && menuRef.current.contains(e.target)) return; setMenu(null) }
    const key = (e: KeyboardEvent) => { if (e.key === 'Escape') setMenu(null) }
    window.addEventListener('mousedown', down, true); window.addEventListener('keydown', key, true)
    return () => { window.removeEventListener('mousedown', down, true); window.removeEventListener('keydown', key, true) }
  }, [menu])
  const dupTitles = new Map<string, number>(); for (const t of group.tabs) dupTitles.set(t.title, (dupTitles.get(t.title) ?? 0) + 1)
  return (
    <div className={clsx('h-[35px] shrink-0 flex items-stretch bg-panel', !active && 'opacity-90')} onDoubleClick={(e) => { if (e.target === e.currentTarget) void runCommand('file.new') }}>
      <div ref={scrollRef} className="tabbar relative flex-1 flex items-stretch overflow-x-auto overflow-y-hidden min-w-0" onWheel={(e) => { if (e.deltaY && !e.deltaX) { e.currentTarget.scrollLeft += e.deltaY } }}>
        {underline && <div aria-hidden className="tab-underline" style={{ transform: `translateX(${underline.left}px)`, width: underline.width }} />}
        {shownTabs.map((tab) => {
          const i = group.tabs.findIndex((t) => t.id === tab.id)
          const isLeaving = leaving.has(tab.id)
          const isActive = !isLeaving && tab.id === group.activeTabId
          const hint = tabDirHint(tab.path, dupTitles.get(tab.title) ?? 0, relPath)
          return (
            <div key={tab.id} data-active={isActive} data-flip-id={tab.id} draggable={!isLeaving}
              onDragStart={(e) => { dragIdx.current = i; e.dataTransfer.setData('text/caret-tab', JSON.stringify({ group: group.id, tabId: tab.id })); e.dataTransfer.effectAllowed = 'move' }}
              onDragOver={(e) => { e.preventDefault(); e.dataTransfer.dropEffect = 'move' }}
              onDrop={(e) => { e.preventDefault(); const raw = e.dataTransfer.getData('text/caret-tab'); if (!raw) return; const { group: g, tabId } = JSON.parse(raw) as { group: string; tabId: string }; if (g === group.id) { const from = group.tabs.findIndex((t) => t.id === tabId); if (from >= 0) reorderTab(group.id, from, i) } else moveTab(g, tabId, group.id, i) }}
              onMouseDown={(e) => { if (e.button === 1) { e.preventDefault(); void closeTab(group.id, tab.id) } else if (e.button === 0) setActive(group.id, tab.id) }}
              onDoubleClick={(e) => { e.stopPropagation(); keepTab(group.id, tab.id) }}
              onContextMenu={(e) => { e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY, tab }) }}
              title={tab.path ?? tab.title}
              className={clsx('tab-item group relative flex items-center gap-1.5 pl-3 pr-1.5 min-w-[110px] max-w-[220px] text-xs select-none', isActive ? 'bg-editor text-fg' : 'bg-panel text-muted hover:bg-hover', isLeaving && 'tab-item-leaving')}>
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
        <div className="flex-1 min-w-[40px]" onDragOver={(e) => e.preventDefault()} onDrop={(e) => { const raw = e.dataTransfer.getData('text/caret-tab'); if (!raw) return; const { group: g, tabId } = JSON.parse(raw) as { group: string; tabId: string }; if (g === group.id) { const from = group.tabs.findIndex((t) => t.id === tabId); reorderTab(group.id, from, group.tabs.length - 1) } else moveTab(g, tabId, group.id) }} />
      </div>
      <div className="flex items-center gap-0.5 px-1">
        {groupsCount < 3 && <button className="p-1 rounded hover:bg-hover text-muted" title="Split editor right (⌘\)" onClick={() => split()}><SplitSquareHorizontal size={14} /></button>}
      </div>
      {menu && (
        <div ref={menuRef} role="menu" className="fixed z-[1000] min-w-[200px] py-1 rounded-md surface-float text-xs animate-pop" style={{ top: menu.y, left: Math.min(menu.x, window.innerWidth - 220) }}>
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

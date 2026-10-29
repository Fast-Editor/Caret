import { useEffect, useState } from 'react'
import { Command } from 'cmdk'
import { create } from 'zustand'
import { activeEditor } from '@/store/editor'
import { getDocumentSymbols, flattenSymbols, type FlatSymbol } from './symbols'
import { SymbolIcon } from './SymbolIcon'
import { symbolKindName } from '@/lib/lsp/convert'

interface PickerState { open: boolean; symbols: FlatSymbol[]; setOpen: (v: boolean) => void; setSymbols: (s: FlatSymbol[]) => void }
export const useSymbolPicker = create<PickerState>((set) => ({ open: false, symbols: [], setOpen: (open) => set({ open }), setSymbols: (symbols) => set({ symbols }) }))

export async function openSymbolPicker(): Promise<void> {
  const ed = activeEditor(); const model = ed?.getModel(); if (!ed || !model) return
  const syms = await getDocumentSymbols(model)
  useSymbolPicker.getState().setSymbols(flattenSymbols(syms))
  useSymbolPicker.getState().setOpen(true)
}

export function SymbolPicker() {
  const { open, symbols, setOpen } = useSymbolPicker()
  const [q, setQ] = useState('')
  useEffect(() => { if (open) setQ('') }, [open])
  if (!open) return null
  const go = (s: FlatSymbol) => { setOpen(false); const ed = activeEditor(); if (!ed) return; ed.revealRangeInCenter(s.selectionRange); ed.setSelection(s.selectionRange); ed.focus() }
  return (
    <div className="fixed inset-0 z-[1050] flex items-start justify-center pt-[12vh]" onMouseDown={() => setOpen(false)}>
      <Command className="w-[560px] max-h-[60vh] flex flex-col rounded-md surface-float animate-pop text-sm" onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => { if (e.key === 'Escape') setOpen(false) }} filter={(value, search) => value.toLowerCase().includes(search.toLowerCase()) ? 1 : 0}>
        <Command.Input autoFocus value={q} onValueChange={setQ} placeholder="Go to symbol in editor… (type @ for grouped)" className="px-3 h-9 bg-transparent outline-none border-b border-border" />
        <Command.List className="overflow-auto p-1">
          <Command.Empty className="px-3 py-2 text-muted text-xs">No symbols</Command.Empty>
          {symbols.map((s) => (
            <Command.Item key={s.id} value={`${s.name} ${symbolKindName(s.kind + 1)}`} onSelect={() => go(s)} className="flex items-center gap-2 px-2 h-7 rounded text-xs data-[selected=true]:bg-selected cursor-pointer">
              <span style={{ paddingLeft: s.depth * 10 }} className="flex items-center gap-2"><SymbolIcon kind={s.kind} />{s.name}</span>
              <span className="text-faint">{s.detail}</span>
              <span className="flex-1" />
              <span className="text-faint">{symbolKindName(s.kind + 1)} · {s.range.startLineNumber}</span>
            </Command.Item>
          ))}
        </Command.List>
      </Command>
    </div>
  )
}

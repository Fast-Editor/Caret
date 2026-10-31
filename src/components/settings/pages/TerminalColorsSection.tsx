import { useMemo, useState } from 'react'
import { Download, RotateCcw, Upload, ClipboardPaste } from 'lucide-react'
import { api } from '@/lib/api'
import { THEMES } from '@/lib/theme'
import { ANSI_KEYS, ANSI_LABELS, mergedAnsi, parseColorScheme, toWindowsTerminalScheme, type AnsiKey } from '@/lib/terminalColors'
import { defaultAnsi } from '@/components/terminal/xtermTheme'
import { useSettings } from '@/store/settings'
import { Button, Section, TextArea, notify } from '../primitives'

/** Appearance → Terminal colours: the 16 ANSI colours of the current theme, with import/export of common scheme files. */
export function TerminalColorsSection() {
  const settings = useSettings((s) => s.settings)
  const update = useSettings((s) => s.update)
  const [pasting, setPasting] = useState(false)
  const [pasteText, setPasteText] = useState('')
  const theme = settings?.ui.theme ?? 'caret-dark'
  const themeLabel = THEMES.find((t) => t.id === theme)?.label ?? theme
  const allOverrides = settings?.ui.terminalAnsi ?? {}
  const overrides = allOverrides[theme] ?? {}
  const base = useMemo(() => defaultAnsi(theme), [theme])
  const palette = useMemo(() => mergedAnsi(base, overrides), [base, overrides])
  const changed = Object.keys(overrides).length > 0

  const save = (next: Record<string, string>) => {
    const map = { ...allOverrides }
    if (Object.keys(next).length) map[theme] = next; else delete map[theme]
    void update({ ui: { terminalAnsi: map } } as never)
  }
  const setColor = (k: AnsiKey, v: string) => save({ ...overrides, [k]: v })
  const applyScheme = (text: string, source: string) => {
    const scheme = parseColorScheme(text)
    if (!scheme) { notify('error', 'Not a colour scheme', 'Expected Windows Terminal JSON, VS Code colorCustomizations or an iTerm2 .itermcolors file.'); return }
    const next: Record<string, string> = { ...overrides }
    for (const k of ANSI_KEYS) { const v = scheme.ansi[k]; if (v) next[k] = v }
    save(next)
    const n = Object.keys(scheme.ansi).length
    notify('success', `Imported ${scheme.name ?? source}`, `${n} colour${n === 1 ? '' : 's'} applied to ${themeLabel}.${scheme.background ? ' Background and foreground stay with the app theme.' : ''}`)
    setPasting(false); setPasteText('')
  }
  const importFile = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const picked = await open({ multiple: false, filters: [{ name: 'Colour schemes', extensions: ['json', 'itermcolors', 'plist'] }] })
      const path = typeof picked === 'string' ? picked : (picked as { path?: string } | null)?.path
      if (!path) return
      const file = await api.fs.readFile(path)
      applyScheme(file.content, path.split('/').pop() ?? 'file')
    } catch (e) { notify('error', 'Import failed', String((e as Error).message ?? e)) }
  }
  const exportScheme = async () => {
    await navigator.clipboard.writeText(toWindowsTerminalScheme(`Caret ${themeLabel}`, palette))
    notify('success', 'Scheme copied', 'Windows Terminal JSON is on the clipboard.')
  }

  return (
    <Section title="Terminal colours" description={`ANSI palette for ${themeLabel}. Changes apply live to open terminals; each app theme keeps its own palette.`}
      actions={<div className="flex items-center gap-1">
        <Button size="sm" variant="ghost" onClick={() => setPasting((v) => !v)} title="Paste a scheme"><ClipboardPaste className="size-3.5" /> Paste</Button>
        <Button size="sm" variant="ghost" onClick={() => void importFile()} title="Import a Windows Terminal, VS Code or iTerm2 scheme file"><Upload className="size-3.5" /> Import…</Button>
        <Button size="sm" variant="ghost" onClick={() => void exportScheme()} title="Copy as Windows Terminal JSON"><Download className="size-3.5" /> Export</Button>
        <Button size="sm" variant="ghost" disabled={!changed} onClick={() => save({})} title="Back to the theme defaults"><RotateCcw className="size-3.5" /> Reset</Button>
      </div>}>
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-x-4 gap-y-2 py-1" data-testid="terminal-ansi-grid">
        {ANSI_KEYS.map((k) => (
          <label key={k} className="flex items-center gap-2 text-xs cursor-pointer group">
            <span className="relative size-5 rounded-md border border-border shrink-0 overflow-hidden" style={{ background: palette[k] }}>
              <input type="color" value={palette[k]} onChange={(e) => setColor(k, e.target.value)} className="absolute inset-0 opacity-0 cursor-pointer" aria-label={ANSI_LABELS[k]} />
            </span>
            <span className={overrides[k] ? 'text-fg' : 'text-muted'}>{ANSI_LABELS[k]}</span>
            <span className="ml-auto mono text-[10px] text-faint">{palette[k]}</span>
          </label>
        ))}
      </div>
      {pasting && (
        <div className="flex flex-col gap-2 pt-2">
          <TextArea value={pasteText} onChange={setPasteText} rows={5} mono placeholder='Paste a Windows Terminal scheme { "name": …, "black": "#…", … }, a VS Code "workbench.colorCustomizations" block, or an iTerm2 .itermcolors file' />
          <div className="flex gap-2"><Button size="sm" onClick={() => applyScheme(pasteText, 'pasted scheme')} disabled={!pasteText.trim()}>Apply</Button><Button size="sm" variant="ghost" onClick={() => { setPasting(false); setPasteText('') }}>Cancel</Button></div>
        </div>
      )}
      <div className="pt-2 text-[11px] text-faint">Preview: <span className="mono">{ANSI_KEYS.slice(0, 8).map((k) => <span key={k} style={{ color: palette[k] }}>■ </span>)}{ANSI_KEYS.slice(8).map((k) => <span key={k} style={{ color: palette[k] }}>■ </span>)}</span></div>
    </Section>
  )
}

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ArrowUp, AtSign, FolderGit2, Image as ImageIcon, Paperclip, Square } from 'lucide-react'
import type { ContextItem, ImageAttachment, SlashCommandInfo } from '@/types'
import { AUTO_MODEL, isSessionBusy, resolveModel, useChat } from '@/store/chat'
import { useSettings } from '@/store/settings'
import { api } from '@/lib/api'
import { runCommand } from '@/lib/commands'
import { ContextPill } from './composer/ContextPills'
import { MentionPicker } from './composer/MentionPicker'
import { SlashCommandPicker } from './composer/SlashCommandPicker'
import { ContextRing, ModeSelector, ModelPicker, ThinkingControls } from './composer/Pickers'
import { AgentPicker, CARET_AGENT } from './composer/AgentPicker'
import { AgentOptions } from './composer/AgentOptions'
import { ImageThumb } from './messages/Parts'
import { detectTrigger, parseSlashCommand, replaceTrigger, type Trigger } from './mentions'
import { baseName, fmtTokens, uid, MOD } from './util'
import { getActiveFile } from '@/lib/chatBridge'

const MODE_SLASH = new Set(['agent', 'ask', 'plan', 'debug'])

async function fileToAttachment(file: File): Promise<ImageAttachment | null> {
  if (!file.type.startsWith('image/')) return null
  const buf = await file.arrayBuffer()
  let bin = ''; const bytes = new Uint8Array(buf); for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
  return { id: uid('img'), name: file.name || 'pasted-image.png', mime: file.type, dataBase64: btoa(bin) }
}

export function Composer({ sessionId, autoFocusNonce }: { sessionId: string; autoFocusNonce?: number }) {
  const draft = useChat((s) => s.drafts[sessionId]) ?? useChat.getState().draft(sessionId)
  const update = useChat((s) => s.updateDraft)
  const send = useChat((s) => s.send)
  const stop = useChat((s) => s.stop)
  const addContext = useChat((s) => s.addContext)
  const removeContext = useChat((s) => s.removeContext)
  const addImage = useChat((s) => s.addImage)
  const slash = useChat((s) => s.slashCommands)
  const setMode = useChat((s) => s.setMode)
  const session = useChat((s) => s.loaded[sessionId])
  const status = session?.status
  const busy = isSessionBusy(status)
  const settings = useSettings((s) => s.settings)
  const taRef = useRef<HTMLTextAreaElement>(null)
  const [trigger, setTrigger] = useState<Trigger | null>(null)
  const [activeFile, setActiveFile] = useState<{ path: string } | null>(null)
  const [tokens, setTokens] = useState<number | null>(null)
  const [dragOver, setDragOver] = useState(false)
  const model = resolveModel(draft.modelId)

  useEffect(() => { taRef.current?.focus() }, [autoFocusNonce, sessionId])
  // Materialise the draft so selectors return a stable object.
  useEffect(() => { if (!useChat.getState().drafts[sessionId]) update(sessionId, {}) }, [sessionId, update])
  // Track the active file for the auto "Current file" pill (polled lightly on focus, not continuously).
  useEffect(() => { let alive = true; const refresh = () => getActiveFile().then((f) => { if (alive) setActiveFile(f ? { path: f.path } : null) }); void refresh(); const onFocus = () => void refresh(); window.addEventListener('focusin', onFocus); return () => { alive = false; window.removeEventListener('focusin', onFocus) } }, [sessionId])

  // auto-grow
  const resize = useCallback(() => { const ta = taRef.current; if (!ta) return; ta.style.height = 'auto'; const max = Math.max(120, window.innerHeight * 0.4); ta.style.height = `${Math.min(max, ta.scrollHeight)}px` }, [])
  useEffect(resize, [draft.text, resize])

  // token estimate (debounced)
  useEffect(() => {
    const items: ContextItem[] = [...draft.context, ...(draft.includeActiveFile && activeFile ? [{ id: 'active', kind: 'active-file' as const, label: 'Current file', path: activeFile.path }] : [])]
    if (!items.length && !draft.text) { setTokens(null); return }
    const t = setTimeout(() => { api.chat.contextPreview(items).then((r) => setTokens(r.totalTokens + Math.ceil(draft.text.length / 3.6))).catch(() => setTokens(Math.ceil(draft.text.length / 3.6))) }, 400)
    return () => clearTimeout(t)
  }, [draft.context, draft.text, draft.includeActiveFile, activeFile])

  const onChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const text = e.target.value
    update(sessionId, { text })
    setTrigger(detectTrigger(text, e.target.selectionStart ?? text.length))
  }
  const onSelect = () => { const ta = taRef.current; if (!ta) return; setTrigger(detectTrigger(ta.value, ta.selectionStart ?? ta.value.length)) }

  const applyReplacement = (replacement: string) => {
    if (!trigger) return
    const { text, caret } = replaceTrigger(draft.text, trigger, replacement)
    update(sessionId, { text })
    setTrigger(null)
    requestAnimationFrame(() => { const ta = taRef.current; if (ta) { ta.focus(); ta.setSelectionRange(caret, caret) } })
  }
  const pickMention = (item: ContextItem) => { addContext(sessionId, item); applyReplacement('') }
  const pickSlash = (c: SlashCommandInfo) => {
    if (MODE_SLASH.has(c.name)) { void setMode(sessionId, c.name); applyReplacement(''); return }
    if (c.name === 'clear') { applyReplacement(''); void runCommand('chat.new'); return }
    if (c.name === 'summarize') { applyReplacement(''); void api.chat.summarize(sessionId); return }
    if (c.name === 'review') { applyReplacement(''); void runCommand('ai.review'); return }
    if (c.name === 'commit') { applyReplacement(''); void runCommand('ai.commitMessage'); return }
    if (c.name === 'worktree') { applyReplacement(''); void runCommand('chat.newBackground'); return }
    if (c.name === 'help') { applyReplacement(''); void runCommand('help.shortcuts'); return }
    applyReplacement(`/${c.name} `)
  }

  const doSend = async () => {
    const text = draft.text.trim()
    const sc = parseSlashCommand(text)
    if (sc && MODE_SLASH.has(sc.name)) { void setMode(sessionId, sc.name); update(sessionId, { text: sc.args }); if (!sc.args) return }
    const ctx = [...draft.context]
    if (draft.includeActiveFile && activeFile && !ctx.some((c) => c.path === activeFile.path && (c.kind === 'file' || c.kind === 'active-file'))) ctx.push({ id: uid('af'), kind: 'active-file', label: 'Current file', path: activeFile.path })
    try { await send(sessionId, { context: ctx, activeFile: activeFile?.path }) } catch (e) { void runCommand('app.notify', { level: 'error', title: 'Send failed', message: e instanceof Error ? e.message : String(e) }) }
  }

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (trigger && (e.key === 'ArrowDown' || e.key === 'ArrowUp' || e.key === 'Enter' || e.key === 'Tab')) {
      // forward navigation keys to the cmdk list
      const list = document.querySelector('[cmdk-root] [cmdk-input]') as HTMLInputElement | null
      if (list) { list.dispatchEvent(new KeyboardEvent('keydown', { key: e.key, bubbles: true })); e.preventDefault(); return }
    }
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault()
      if (e.metaKey || e.ctrlKey) { const pending = session?.pendingChanges.some((c) => c.status === 'pending'); if (pending && !draft.text.trim()) { void runCommand('chat.acceptAll'); return } }
      void doSend(); return
    }
    if (e.key === 'Backspace' && (e.metaKey || e.ctrlKey) && !draft.text.trim() && session?.pendingChanges.some((c) => c.status === 'pending')) { e.preventDefault(); void runCommand('chat.rejectAll'); return }
    if (e.key === 'Escape') { if (trigger) { setTrigger(null); return } if (busy) void stop(sessionId) }
    if (e.key === 'ArrowUp' && !draft.text && session) { const last = [...session.messages].reverse().find((m) => m.role === 'user'); if (last) { e.preventDefault(); useChat.getState().setEditing(last.id) } }
    if (e.key === '.' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); void runCommand('chat.cycleMode') }
  }

  const onPaste = async (e: React.ClipboardEvent) => {
    const files = Array.from(e.clipboardData.files)
    if (!files.length) return
    e.preventDefault()
    for (const f of files) { const a = await fileToAttachment(f); if (a) { if (model && !model.supportsVision) void runCommand('app.notify', { level: 'warn', title: 'Model has no vision support', message: `${model.displayName} may ignore images.` }); addImage(sessionId, a) } }
  }
  const onDrop = async (e: React.DragEvent) => {
    e.preventDefault(); setDragOver(false)
    const custom = e.dataTransfer.getData('application/x-caret-path') || e.dataTransfer.getData('text/uri-list')
    if (custom) { for (const raw of custom.split(/\r?\n/).filter(Boolean)) { const p = raw.replace(/^file:\/\//, ''); addContext(sessionId, { id: uid('f'), kind: p.endsWith('/') ? 'folder' : 'file', label: baseName(p), path: p }) } return }
    for (const f of Array.from(e.dataTransfer.files)) { const a = await fileToAttachment(f); if (a) addImage(sessionId, a) }
  }
  const attachImage = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const picked = await open({ multiple: true, filters: [{ name: 'Images', extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp'] }] })
      const paths = Array.isArray(picked) ? picked : picked ? [picked] : []
      for (const p of paths) { const img = await api.fs.readImage(p); addImage(sessionId, { id: uid('img'), name: baseName(p), mime: img.mime, dataBase64: img.dataBase64 }) }
    } catch (e) { void runCommand('app.notify', { level: 'error', title: 'Attach failed', message: String(e) }) }
  }
  const insertAt = () => { const ta = taRef.current; if (!ta) return; const pos = ta.selectionStart ?? draft.text.length; const before = draft.text.slice(0, pos); const needsSpace = before && !/\s$/.test(before); const text = `${before}${needsSpace ? ' ' : ''}@${draft.text.slice(pos)}`; update(sessionId, { text }); requestAnimationFrame(() => { ta.focus(); const c = pos + (needsSpace ? 2 : 1); ta.setSelectionRange(c, c); setTrigger(detectTrigger(text, c)) }) }

  const autoRun = settings?.agent.autoRun ?? 'allowlist'
  const ctxWindow = model?.contextWindow ?? 128_000
  const used = session?.contextTokens ?? 0
  const placeholder = useMemo(() => (draft.mode === 'ask' ? 'Ask anything about your code…' : draft.mode === 'plan' ? 'Describe what to plan…' : 'Plan, search, build anything… (@ for context, / for commands)'), [draft.mode])

  return (
    <div className={`relative m-2 mt-1 rounded-lg border ${dragOver ? 'border-accent' : 'border-border'} bg-input focus-within:border-border-strong`} onDragOver={(e) => { e.preventDefault(); setDragOver(true) }} onDragLeave={() => setDragOver(false)} onDrop={(e) => void onDrop(e)}>
      {trigger?.kind === '@' && <MentionPicker query={trigger.query} sessionId={sessionId} onPick={pickMention} onClose={() => setTrigger(null)} />}
      {trigger?.kind === '/' && <SlashCommandPicker query={trigger.query} commands={slash} onPick={pickSlash} onClose={() => setTrigger(null)} />}
      <div className="flex flex-wrap items-center gap-1 px-2 pt-1.5">
        <button type="button" onClick={insertAt} title="Add context (@)" className="flex items-center justify-center w-5 h-5 rounded border border-dashed border-border text-faint hover:text-fg hover:border-border-strong"><AtSign size={11} /></button>
        {activeFile && !draft.context.some((c) => c.path === activeFile.path && c.kind === 'file') && (
          <ContextPill item={{ id: 'active', kind: 'active-file', label: 'Current file', path: activeFile.path }} dim={!draft.includeActiveFile} onRemove={draft.includeActiveFile ? () => update(sessionId, { includeActiveFile: false }) : undefined} onTogglePin={!draft.includeActiveFile ? () => update(sessionId, { includeActiveFile: true }) : undefined} small />
        )}
        {draft.context.map((c) => <ContextPill key={c.id} item={c} small onRemove={() => removeContext(sessionId, c.id)} onTogglePin={() => update(sessionId, (d) => ({ context: d.context.map((x) => (x.id === c.id ? { ...x, pinned: !x.pinned } : x)) }))} />)}
        {draft.useWorktree && <span className="inline-flex items-center gap-1 h-[18px] px-1 rounded border border-border text-[10.5px] text-muted" title={session?.worktreePath ?? 'Runs in an isolated git worktree'}><FolderGit2 size={10} /> worktree</span>}
      </div>
      {draft.images.length > 0 && <div className="flex flex-wrap gap-2 px-2 pt-2">{draft.images.map((im) => <ImageThumb key={im.id} image={im} size={56} onRemove={() => update(sessionId, (d) => ({ images: d.images.filter((x) => x.id !== im.id) }))} />)}</div>}
      <textarea ref={taRef} value={draft.text} onChange={onChange} onSelect={onSelect} onKeyDown={onKeyDown} onPaste={(e) => void onPaste(e)} placeholder={placeholder} rows={1} spellCheck
        className="w-full resize-none bg-transparent px-2.5 py-2 outline-none text-[13px] leading-[1.5] placeholder:text-faint min-h-[40px]" />
      <div className="flex items-center gap-0.5 px-1.5 pb-1.5">
        <AgentPicker sessionId={sessionId} />
        {(draft.agentId ?? CARET_AGENT) === CARET_AGENT ? (
          <>
            <ModeSelector sessionId={sessionId} />
            <ModelPicker sessionId={sessionId} />
            <ThinkingControls sessionId={sessionId} supportsThinking={!!model?.supportsThinking} supportsEffort={!!model?.supportsEffort} />
          </>
        ) : (
          <AgentOptions sessionId={sessionId} />
        )}
        <span className="flex-1" />
        <button type="button" onClick={() => void runCommand('settings.open', { page: 'agent' })} title={`Auto-run: ${autoRun}. Click to configure.`} className={`h-6 px-1.5 rounded hover:bg-hover text-[10.5px] ${autoRun === 'all' ? 'text-warning' : autoRun === 'off' ? 'text-faint' : 'text-muted'}`}>{autoRun === 'all' ? 'auto-run: all' : autoRun === 'off' ? 'auto-run: off' : 'auto-run'}</button>
        {tokens !== null && <span className="text-[10.5px] text-faint px-1" title="Estimated prompt tokens">~{fmtTokens(tokens)}</span>}
        <ContextRing used={used} total={ctxWindow} onClick={() => void api.chat.summarize(sessionId)} />
        <button type="button" onClick={() => void attachImage()} title="Attach image" className="flex items-center justify-center w-6 h-6 rounded hover:bg-hover text-muted">{draft.images.length ? <ImageIcon size={13} /> : <Paperclip size={13} />}</button>
        {busy ? (
          <button type="button" onClick={() => void stop(sessionId)} title="Stop (Esc)" className="flex items-center justify-center w-6 h-6 rounded bg-danger/20 text-danger hover:bg-danger/30"><Square size={11} fill="currentColor" /></button>
        ) : (
          <button type="button" onClick={() => void doSend()} disabled={!draft.text.trim() && !draft.images.length} title={`Send (⏎)${draft.modelId === AUTO_MODEL ? ' · Auto model' : ''} · ${MOD}⏎ accepts changes`} className="flex items-center justify-center w-6 h-6 rounded bg-accent text-accent-fg hover:bg-accent-hover disabled:opacity-40 disabled:hover:bg-accent"><ArrowUp size={14} /></button>
        )}
      </div>
    </div>
  )
}

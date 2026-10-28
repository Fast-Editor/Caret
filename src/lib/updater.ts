// In-app updates (tauri-plugin-updater). Release artifacts are signed with Caret's minisign key and described by
// `latest.json` on the GitHub release; the plugin verifies the signature before installing.
import { check, type Update } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { runCommand } from '@/lib/commands'

export type UpdateState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'none'; current: string }
  | { kind: 'available'; update: Update; version: string; notes?: string; date?: string }
  | { kind: 'downloading'; version: string; received: number; total?: number }
  | { kind: 'ready'; version: string }
  | { kind: 'error'; message: string }

let pending: Update | null = null

/** Ask GitHub for a newer release. Dev builds and unsigned local builds report an error, which callers render softly. */
export async function checkForUpdates(): Promise<UpdateState> {
  try {
    const u = await check({ timeout: 15_000 })
    if (!u) return { kind: 'none', current: '' }
    pending = u
    return { kind: 'available', update: u, version: u.version, notes: u.body ?? undefined, date: u.date ?? undefined }
  } catch (e) {
    return { kind: 'error', message: String((e as Error)?.message ?? e) }
  }
}

/** Download + install the pending update, reporting progress, then relaunch. */
export async function installUpdate(onProgress?: (received: number, total?: number) => void): Promise<void> {
  const u = pending
  if (!u) throw new Error('no update pending — check for updates first')
  let received = 0; let total: number | undefined
  await u.downloadAndInstall((ev) => {
    if (ev.event === 'Started') { total = ev.data.contentLength ?? undefined }
    else if (ev.event === 'Progress') { received += ev.data.chunkLength; onProgress?.(received, total) }
    else if (ev.event === 'Finished') { onProgress?.(total ?? received, total) }
  })
  await relaunch()
}

/** Silent startup check: one notification when a newer version exists; quiet on errors (offline, dev build, unsigned). */
export async function checkForUpdatesSilently(): Promise<void> {
  const st = await checkForUpdates()
  if (st.kind === 'available') {
    void runCommand('app.notify', { level: 'info', title: `Caret ${st.version} is available`, message: 'Install it from Settings → About.', timeoutMs: 8000 })
  }
}

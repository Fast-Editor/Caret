/**
 * Remote access glue mounted once from App.tsx (docs/REMOTE.md).
 * * In the desktop webview: starts the executor that runs relayed commands for `caret serve` clients.
 * * In a browser session (the shim set `window.__CARET_REMOTE__`): shows a thin "Remote session" strip.
 */
import { useEffect, useState } from 'react'
import { startRemoteExecutor } from '@/lib/remoteExecutor'

interface RemoteFlag { connected: boolean; token: string | null; url: string; attempts?: number }
const remoteFlag = (): RemoteFlag | undefined => (typeof window === 'undefined' ? undefined : (window as unknown as { __CARET_REMOTE__?: RemoteFlag }).__CARET_REMOTE__)

export function RemoteGlue() {
  const remote = remoteFlag()
  useEffect(() => { if (remoteFlag()) return; return startRemoteExecutor() }, [])
  if (!remote) return null
  return <RemoteBanner flag={remote} />
}

function RemoteBanner({ flag }: { flag: RemoteFlag }) {
  const [connected, setConnected] = useState(flag.connected)
  useEffect(() => {
    const onChange = (e: Event) => setConnected(!!(e as CustomEvent<{ connected: boolean }>).detail?.connected)
    window.addEventListener('caret-remote', onChange)
    return () => window.removeEventListener('caret-remote', onChange)
  }, [])
  return (
    <div className="remote-banner" data-connected={String(connected)} role="status" aria-live="polite">
      <span className="dot" aria-hidden />
      <span>Remote session:</span>
      <span className="mono">{flag.url}</span>
      <span className="text-faint">{connected ? 'connected to the Caret desktop' : 'reconnecting to the Caret desktop'}</span>
    </div>
  )
}

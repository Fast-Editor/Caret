import { useEffect, useState } from 'react'
import { api } from '@/lib/api'
import { ZoomIn, ZoomOut, Maximize } from 'lucide-react'

export function ImageTab({ path }: { path: string }) {
  const [src, setSrc] = useState<string | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [zoom, setZoom] = useState(1)
  const [dims, setDims] = useState<{ w: number; h: number } | null>(null)
  useEffect(() => { setSrc(null); setErr(null); api.fs.readImage(path).then((r) => setSrc(`data:${r.mime};base64,${r.dataBase64}`)).catch((e) => setErr(String(e?.message ?? e))) }, [path])
  return (
    <div className="h-full flex flex-col bg-editor">
      <div className="h-7 shrink-0 flex items-center gap-2 px-2 border-b border-border text-xs text-muted">
        <span className="truncate">{path.split('/').pop()}</span>{dims && <span>{dims.w}×{dims.h}</span>}<span className="flex-1" />
        <button className="p-1 rounded hover:bg-hover" onClick={() => setZoom((z) => Math.max(0.1, z / 1.25))}><ZoomOut size={13} /></button>
        <span className="w-10 text-center">{Math.round(zoom * 100)}%</span>
        <button className="p-1 rounded hover:bg-hover" onClick={() => setZoom((z) => Math.min(16, z * 1.25))}><ZoomIn size={13} /></button>
        <button className="p-1 rounded hover:bg-hover" onClick={() => setZoom(1)}><Maximize size={13} /></button>
      </div>
      <div className="flex-1 overflow-auto flex items-center justify-center p-4 [background:repeating-conic-gradient(var(--bg-input)_0_25%,transparent_0_50%)_0_0/16px_16px]" onWheel={(e) => { if (e.metaKey || e.ctrlKey) { e.preventDefault(); setZoom((z) => Math.min(16, Math.max(0.1, z * (e.deltaY < 0 ? 1.1 : 0.9)))) } }}>
        {err && <div className="text-danger text-xs">{err}</div>}
        {src && <img src={src} alt="" style={{ transform: `scale(${zoom})`, transformOrigin: 'center', imageRendering: zoom > 2 ? 'pixelated' : 'auto', maxWidth: zoom === 1 ? '100%' : undefined, maxHeight: zoom === 1 ? '100%' : undefined }} onLoad={(e) => setDims({ w: e.currentTarget.naturalWidth, h: e.currentTarget.naturalHeight })} />}
      </div>
    </div>
  )
}

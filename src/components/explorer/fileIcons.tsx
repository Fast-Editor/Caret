import { File, FileCode2, FileJson, FileText, FileImage, FileArchive, FileTerminal, FileType2, Folder, FolderOpen, Settings2, Lock, GitBranch, Braces, Database, FileSpreadsheet, FileVideo, FileAudio, Package, BookOpen, Cog } from 'lucide-react'
import type { ReactNode } from 'react'

/** Small colored chip for language identity (VS Code-ish palette via inline color only for file-type hues). */
function chip(text: string, color: string): ReactNode {
  return <span className="inline-flex items-center justify-center size-4 rounded-[3px] text-[8px] font-bold leading-none" style={{ color, background: `${color}22` }}>{text}</span>
}

const EXT_CHIPS: Record<string, [string, string]> = {
  ts: ['TS', '#3b82f6'], tsx: ['TX', '#3b82f6'], mts: ['TS', '#3b82f6'], cts: ['TS', '#3b82f6'],
  js: ['JS', '#eab308'], jsx: ['JX', '#eab308'], mjs: ['JS', '#eab308'], cjs: ['JS', '#eab308'],
  py: ['PY', '#22c55e'], rs: ['RS', '#f97316'], go: ['GO', '#06b6d4'], java: ['JV', '#ef4444'], kt: ['KT', '#a855f7'], swift: ['SW', '#f97316'],
  c: ['C', '#60a5fa'], h: ['H', '#60a5fa'], cpp: ['C+', '#60a5fa'], cc: ['C+', '#60a5fa'], hpp: ['H+', '#60a5fa'], cs: ['C#', '#a855f7'], rb: ['RB', '#ef4444'], php: ['PH', '#818cf8'],
  css: ['#', '#38bdf8'], scss: ['S#', '#f472b6'], less: ['L#', '#60a5fa'], html: ['<>', '#f97316'], vue: ['V', '#22c55e'], svelte: ['SV', '#f97316'],
  sql: ['SQ', '#f59e0b'], sh: ['$', '#a3e635'], bash: ['$', '#a3e635'], zsh: ['$', '#a3e635'], lua: ['LU', '#60a5fa'], zig: ['ZG', '#f59e0b'], dart: ['DA', '#06b6d4'], ex: ['EX', '#a855f7'], exs: ['EX', '#a855f7'], erl: ['ER', '#ef4444'], hs: ['HS', '#a855f7'], ml: ['ML', '#f97316'], scala: ['SC', '#ef4444'], r: ['R', '#60a5fa'], m: ['OC', '#60a5fa'], tf: ['TF', '#a855f7'], proto: ['PB', '#60a5fa'], graphql: ['GQ', '#f472b6'], gql: ['GQ', '#f472b6'], wasm: ['WA', '#a855f7'],
}

export function fileIconFor(name: string, isDir: boolean, open = false): ReactNode {
  if (isDir) return open ? <FolderOpen className="text-accent/80" /> : <Folder className="text-accent/80" />
  const lower = name.toLowerCase()
  const ext = lower.includes('.') ? lower.split('.').pop()! : ''
  if (EXT_CHIPS[ext]) { const [t, c] = EXT_CHIPS[ext]; return chip(t, c) }
  if (lower === 'package.json') return <Package className="text-success" />
  if (lower === 'cargo.toml' || lower === 'cargo.lock') return <Package className="text-warning" />
  if (lower.startsWith('.git')) return <GitBranch className="text-danger/80" />
  if (lower === 'dockerfile' || lower.startsWith('docker-compose')) return <Database className="text-info" />
  if (lower.endsWith('.lock') || lower === 'yarn.lock' || lower === 'pnpm-lock.yaml') return <Lock className="text-muted" />
  if (lower.startsWith('.env')) return <Cog className="text-warning" />
  if (/^(readme|license|licence|changelog|contributing)/.test(lower)) return <BookOpen className="text-info" />
  switch (ext) {
    case 'json': case 'jsonc': case 'json5': return <FileJson className="text-warning" />
    case 'yml': case 'yaml': case 'toml': case 'ini': case 'cfg': case 'conf': case 'properties': return <Settings2 className="text-muted" />
    case 'md': case 'mdx': case 'mdc': case 'txt': case 'rst': case 'adoc': return <FileText className="text-info" />
    case 'png': case 'jpg': case 'jpeg': case 'gif': case 'webp': case 'svg': case 'ico': case 'bmp': case 'avif': case 'icns': return <FileImage className="text-success" />
    case 'mp4': case 'mov': case 'webm': case 'mkv': return <FileVideo className="text-muted" />
    case 'mp3': case 'wav': case 'ogg': case 'flac': return <FileAudio className="text-muted" />
    case 'zip': case 'tar': case 'gz': case 'tgz': case 'bz2': case 'xz': case '7z': case 'rar': case 'jar': case 'asar': return <FileArchive className="text-muted" />
    case 'csv': case 'tsv': case 'xlsx': case 'xls': return <FileSpreadsheet className="text-success" />
    case 'xml': case 'plist': case 'xaml': return <Braces className="text-warning" />
    case 'ttf': case 'otf': case 'woff': case 'woff2': return <FileType2 className="text-muted" />
    case 'exe': case 'bin': case 'dll': case 'so': case 'dylib': case 'o': case 'a': return <FileTerminal className="text-muted" />
    case 'log': return <FileText className="text-faint" />
    default: return ext ? <FileCode2 className="text-muted" /> : <File className="text-muted" />
  }
}

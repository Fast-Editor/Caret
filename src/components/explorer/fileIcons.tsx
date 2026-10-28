import { File, FileCode2, FileJson, FileText, FileImage, FileArchive, FileTerminal, FileType2, Folder, FolderOpen, Settings2, Lock, GitBranch, Braces, Database, FileSpreadsheet, FileVideo, FileAudio, Package, BookOpen, Cog, type LucideIcon } from 'lucide-react'
import type { ReactNode } from 'react'

/** Pure description of a file icon (unit-tested); `fileIconFor` renders it. */
export type IconSpec =
  | { kind: 'folder'; open: boolean }
  | { kind: 'chip'; text: string; color: string }
  | { kind: 'icon'; icon: string; cls: string }

const ICONS: Record<string, LucideIcon> = {
  file: File, code: FileCode2, json: FileJson, text: FileText, image: FileImage, archive: FileArchive, binary: FileTerminal, font: FileType2,
  settings: Settings2, lock: Lock, git: GitBranch, braces: Braces, database: Database, spreadsheet: FileSpreadsheet, video: FileVideo, audio: FileAudio, package: Package, book: BookOpen, cog: Cog,
}

/** Two-letter language glyphs in the type colour (text only — no filled badge, so names dominate the row). */
const EXT_CHIPS: Record<string, [string, string]> = {
  ts: ['TS', '#3b82f6'], tsx: ['TX', '#3b82f6'], mts: ['TS', '#3b82f6'], cts: ['TS', '#3b82f6'],
  js: ['JS', '#eab308'], jsx: ['JX', '#eab308'], mjs: ['JS', '#eab308'], cjs: ['JS', '#eab308'],
  py: ['PY', '#22c55e'], rs: ['RS', '#f97316'], go: ['GO', '#06b6d4'], java: ['JV', '#ef4444'], kt: ['KT', '#a855f7'], swift: ['SW', '#f97316'],
  c: ['C', '#60a5fa'], h: ['H', '#60a5fa'], cpp: ['C+', '#60a5fa'], cc: ['C+', '#60a5fa'], hpp: ['H+', '#60a5fa'], cs: ['C#', '#a855f7'], rb: ['RB', '#ef4444'], php: ['PH', '#818cf8'],
  css: ['#', '#38bdf8'], scss: ['S#', '#f472b6'], less: ['L#', '#60a5fa'], html: ['<>', '#f97316'], vue: ['V', '#22c55e'], svelte: ['SV', '#f97316'],
  sql: ['SQ', '#f59e0b'], sh: ['$', '#a3e635'], bash: ['$', '#a3e635'], zsh: ['$', '#a3e635'], lua: ['LU', '#60a5fa'], zig: ['ZG', '#f59e0b'], dart: ['DA', '#06b6d4'], ex: ['EX', '#a855f7'], exs: ['EX', '#a855f7'], erl: ['ER', '#ef4444'], hs: ['HS', '#a855f7'], ml: ['ML', '#f97316'], scala: ['SC', '#ef4444'],
}

export function iconSpecFor(name: string, isDir: boolean, open = false): IconSpec {
  if (isDir) return { kind: 'folder', open }
  const lower = name.toLowerCase()
  const ext = lower.includes('.') ? lower.split('.').pop()! : ''
  if (EXT_CHIPS[ext]) { const [text, color] = EXT_CHIPS[ext]; return { kind: 'chip', text, color } }
  if (lower === 'package.json') return { kind: 'icon', icon: 'package', cls: 'text-success' }
  if (lower === 'cargo.toml' || lower === 'cargo.lock') return { kind: 'icon', icon: 'package', cls: 'text-warning' }
  if (lower.startsWith('.git')) return { kind: 'icon', icon: 'git', cls: 'text-danger/80' }
  if (lower === 'dockerfile' || lower.startsWith('docker-compose')) return { kind: 'icon', icon: 'database', cls: 'text-info' }
  if (lower.endsWith('.lock') || lower === 'yarn.lock' || lower === 'pnpm-lock.yaml') return { kind: 'icon', icon: 'lock', cls: 'text-muted' }
  if (lower.startsWith('.env')) return { kind: 'icon', icon: 'cog', cls: 'text-warning' }
  if (/^(readme|license|licence|changelog|contributing)/.test(lower)) return { kind: 'icon', icon: 'book', cls: 'text-info' }
  switch (ext) {
    case 'json': case 'jsonc': case 'json5': return { kind: 'icon', icon: 'json', cls: 'text-warning' }
    case 'yml': case 'yaml': case 'toml': case 'ini': case 'cfg': case 'conf': case 'properties': return { kind: 'icon', icon: 'settings', cls: 'text-muted' }
    case 'md': case 'mdx': case 'mdc': case 'txt': case 'rst': case 'adoc': return { kind: 'icon', icon: 'text', cls: 'text-info' }
    case 'png': case 'jpg': case 'jpeg': case 'gif': case 'webp': case 'svg': case 'ico': case 'bmp': case 'avif': case 'icns': return { kind: 'icon', icon: 'image', cls: 'text-success' }
    case 'mp4': case 'mov': case 'webm': case 'mkv': return { kind: 'icon', icon: 'video', cls: 'text-muted' }
    case 'mp3': case 'wav': case 'ogg': case 'flac': return { kind: 'icon', icon: 'audio', cls: 'text-muted' }
    case 'zip': case 'tar': case 'gz': case 'tgz': case 'bz2': case 'xz': case '7z': case 'rar': case 'jar': case 'asar': return { kind: 'icon', icon: 'archive', cls: 'text-muted' }
    case 'csv': case 'tsv': case 'xlsx': case 'xls': return { kind: 'icon', icon: 'spreadsheet', cls: 'text-success' }
    case 'xml': case 'plist': case 'xaml': return { kind: 'icon', icon: 'braces', cls: 'text-warning' }
    case 'ttf': case 'otf': case 'woff': case 'woff2': return { kind: 'icon', icon: 'font', cls: 'text-muted' }
    case 'exe': case 'bin': case 'dll': case 'so': case 'dylib': case 'o': case 'a': return { kind: 'icon', icon: 'binary', cls: 'text-muted' }
    case 'log': return { kind: 'icon', icon: 'text', cls: 'text-faint' }
    default: return ext ? { kind: 'icon', icon: 'code', cls: 'text-muted/80' } : { kind: 'icon', icon: 'file', cls: 'text-muted/80' }
  }
}

/** 14px, light-stroke icon (or a 9px two-letter glyph) — sized to sit under the text, not compete with it. */
export function fileIconFor(name: string, isDir: boolean, open = false): ReactNode {
  const spec = iconSpecFor(name, isDir, open)
  if (spec.kind === 'folder') return spec.open ? <FolderOpen className="size-3.5 text-muted" strokeWidth={1.75} /> : <Folder className="size-3.5 text-muted" strokeWidth={1.75} />
  if (spec.kind === 'chip') return <span className="inline-flex items-center justify-center size-4 text-[9px] font-semibold leading-none tracking-tight select-none" style={{ color: spec.color }}>{spec.text}</span>
  const Icon = ICONS[spec.icon] ?? File
  return <Icon className={`size-3.5 ${spec.cls}`} strokeWidth={1.75} />
}

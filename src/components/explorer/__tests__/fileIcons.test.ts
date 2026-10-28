import { describe, expect, it } from 'vitest'
import { iconSpecFor } from '../fileIcons'

describe('iconSpecFor', () => {
  it('maps languages to coloured two-letter glyphs', () => {
    expect(iconSpecFor('app.ts', false)).toEqual({ kind: 'chip', text: 'TS', color: '#3b82f6' })
    expect(iconSpecFor('index.js', false)).toEqual({ kind: 'chip', text: 'JS', color: '#eab308' })
    expect(iconSpecFor('main.rs', false)).toMatchObject({ kind: 'chip', text: 'RS' })
    expect(iconSpecFor('tool.py', false)).toMatchObject({ kind: 'chip', text: 'PY' })
    expect(iconSpecFor('styles.css', false)).toMatchObject({ kind: 'chip' })
    expect(iconSpecFor('page.html', false)).toMatchObject({ kind: 'chip' })
  })
  it('maps well-known files and extensions to distinct icons', () => {
    const icon = (n: string) => { const s = iconSpecFor(n, false); return s.kind === 'icon' ? s.icon : s.kind }
    expect(icon('package.json')).toBe('package')
    expect(icon('settings.json')).toBe('json')
    expect(icon('README.md')).toBe('book')
    expect(icon('notes.md')).toBe('text')
    expect(icon('config.yaml')).toBe('settings')
    expect(icon('Cargo.toml')).toBe('package')
    expect(icon('pnpm-lock.yaml')).toBe('lock')
    expect(icon('.env.local')).toBe('cog')
    expect(icon('Dockerfile')).toBe('database')
    expect(icon('.gitignore')).toBe('git')
    expect(icon('logo.png')).toBe('image')
    expect(icon('bundle.zip')).toBe('archive')
    expect(icon('Makefile')).toBe('file')
    expect(icon('weird.xyz')).toBe('code')
  })
  it('keeps the required file types visually distinct', () => {
    const names = ['a.ts', 'a.js', 'a.json', 'a.md', 'a.py', 'a.rs', 'a.css', 'a.html', 'a.yaml', 'a.toml', 'a.lock', '.env', 'Dockerfile', '.gitignore', 'a.png', 'a.zip']
    const keys = names.map((n) => { const s = iconSpecFor(n, false); return s.kind === 'chip' ? `chip:${s.text}:${s.color}` : s.kind === 'icon' ? `icon:${s.icon}:${s.cls}` : s.kind })
    // yaml/toml intentionally share the settings icon; everything else must differ
    expect(new Set(keys).size).toBeGreaterThanOrEqual(names.length - 1)
  })
  it('distinguishes open and closed folders', () => {
    expect(iconSpecFor('src', true)).toEqual({ kind: 'folder', open: false })
    expect(iconSpecFor('src', true, true)).toEqual({ kind: 'folder', open: true })
  })
})

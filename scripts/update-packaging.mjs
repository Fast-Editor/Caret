#!/usr/bin/env node
// Rewrite the version and sha256 fields of every package manager manifest from a release's SHA256SUMS.txt.
//   node scripts/update-packaging.mjs 0.2.0 path/to/SHA256SUMS.txt [--date YYYY-MM-DD]
//   npm run release:packaging -- 0.2.0 SHA256SUMS.txt
// Touches: packaging/winget/*.yaml, packaging/scoop/caret.json, packaging/aur/PKGBUILD, packaging/aur/.SRCINFO, flake.nix.
// Regex based and idempotent: a second run with the same inputs changes nothing. An asset missing from the sums
// file (for example when the Windows build failed) gets REPLACE_WITH_SHA256 and a warning, never a stale hash.
// The release workflow's `packaging` job runs this after `checksums` and commits the result to main.
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const PLACEHOLDER = 'REPLACE_WITH_SHA256'

const args = process.argv.slice(2)
const dateIdx = args.indexOf('--date')
const date = dateIdx >= 0 ? args.splice(dateIdx, 2)[1] : null
const [version, sumsPath] = args
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/.test(version ?? '') || !sumsPath) {
  console.error('usage: node scripts/update-packaging.mjs <semver> <SHA256SUMS.txt> [--date YYYY-MM-DD]')
  process.exit(2)
}
if (date && !/^\d{4}-\d{2}-\d{2}$/.test(date)) { console.error('--date must be YYYY-MM-DD'); process.exit(2) }

// SHA256SUMS.txt lines look like `<hex>  <file>` (sha256sum format; a leading `*` marks binary mode).
const sums = new Map()
for (const line of readFileSync(path.resolve(sumsPath), 'utf8').split('\n')) {
  const m = /^([0-9a-fA-F]{64})\s+\*?(.+?)\s*$/.exec(line)
  if (m) sums.set(path.basename(m[2]), m[1].toLowerCase())
}
const missing = []
const hashOf = (asset) => { const h = sums.get(asset); if (!h) missing.push(asset); return h ?? PLACEHOLDER }

const base = `https://github.com/Fast-Editor/Caret/releases/download/v${version}`
const asset = {
  dmgArm: `Caret_${version}_aarch64.dmg`,
  appImage: `Caret_${version}_amd64.AppImage`,
  deb: `Caret_${version}_amd64.deb`,
  exe: `Caret_${version}_x64-setup.exe`,
}
const hash = { dmgArm: hashOf(asset.dmgArm), appImage: hashOf(asset.appImage), deb: hashOf(asset.deb), exe: hashOf(asset.exe) }
// The AUR package installs the MIT text from the tag; the repo's LICENSE is that file.
const licenseHash = createHash('sha256').update(readFileSync(path.join(root, 'LICENSE'))).digest('hex')
const today = date ?? new Date().toISOString().slice(0, 10)

const changed = []
function rewrite(rel, fn) {
  const file = path.join(root, rel)
  const before = readFileSync(file, 'utf8')
  const after = fn(before, rel)
  if (after !== before) { writeFileSync(file, after); changed.push(rel) }
}
// Replace a pattern that must exist, so a manifest that drifted from the expected shape fails loudly.
function sub(text, re, replacer, label) {
  if (!re.test(text)) throw new Error(`${label}: expected pattern not found: ${re}`)
  re.lastIndex = 0
  return text.replace(re, replacer)
}

// winget: version, installer and default-locale manifests (schema 1.6).
rewrite('packaging/winget/FastEditor.Caret.yaml', (t, f) =>
  sub(t, /^(PackageVersion: ).*$/m, (_, p) => p + version, f))

rewrite('packaging/winget/FastEditor.Caret.installer.yaml', (t, f) => {
  const previous = /^PackageVersion: (.*)$/m.exec(t)?.[1]
  t = sub(t, /^(PackageVersion: ).*$/m, (_, p) => p + version, f)
  t = sub(t, /^(\s*InstallerUrl: ).*$/m, (_, p) => `${p}${base}/${asset.exe}`, f)
  t = sub(t, /^(\s*InstallerSha256: ).*$/m, (_, p) => p + (hash.exe === PLACEHOLDER ? PLACEHOLDER : hash.exe.toUpperCase()), f)
  // Only move the release date when the version actually changes, so re-runs stay idempotent.
  if (previous !== version) t = sub(t, /^(ReleaseDate: ).*$/m, (_, p) => p + today, f)
  return t
})

rewrite('packaging/winget/FastEditor.Caret.locale.en-US.yaml', (t, f) => {
  t = sub(t, /^(PackageVersion: ).*$/m, (_, p) => p + version, f)
  t = sub(t, /^(ReleaseNotesUrl: ).*$/m, (_, p) => `${p}https://github.com/Fast-Editor/Caret/releases/tag/v${version}`, f)
  return t
})

// scoop: the concrete url/hash under architecture.64bit; the autoupdate block keeps its $version placeholders.
rewrite('packaging/scoop/caret.json', (t, f) => {
  t = sub(t, /^(\s*"version": ")[^"]*(")/m, (_, a, b) => a + version + b, f)
  t = sub(t, /("url": ")https:\/\/github\.com\/Fast-Editor\/Caret\/releases\/download\/v(?!\$version)[^"]*(")/, (_, a, b) => `${a}${base}/${asset.exe}${b}`, f)
  t = sub(t, /^(\s*"hash": ")[^"]*(")/m, (_, a, b) => a + hash.exe + b, f)
  return t
})

// AUR: PKGBUILD derives its URLs from pkgver, so only pkgver, pkgrel and the sums change; .SRCINFO spells everything out.
rewrite('packaging/aur/PKGBUILD', (t, f) => {
  const previous = /^pkgver=(.*)$/m.exec(t)?.[1]
  t = sub(t, /^pkgver=.*$/m, () => `pkgver=${version}`, f)
  if (previous !== version) t = sub(t, /^pkgrel=.*$/m, () => 'pkgrel=1', f)
  t = sub(t, /^sha256sums=\([^)]*\)/m, () => `sha256sums=('${hash.deb}'\n            '${licenseHash}')`, f)
  return t
})

rewrite('packaging/aur/.SRCINFO', (t, f) => {
  const previous = /^\tpkgver = (.*)$/m.exec(t)?.[1]
  t = sub(t, /^(\tpkgver = ).*$/m, (_, p) => p + version, f)
  if (previous !== version) t = sub(t, /^(\tpkgrel = ).*$/m, (_, p) => p + '1', f)
  t = sub(t, /caret-bin-[^\s:]+?(\.deb|-LICENSE)/g, (_, suffix) => `caret-bin-${version}${suffix}`, f)
  t = sub(t, /releases\/download\/v[^/]+\/Caret_[^_]+_amd64\.deb/g, () => `releases/download/v${version}/Caret_${version}_amd64.deb`, f)
  t = sub(t, /raw\.githubusercontent\.com\/Fast-Editor\/Caret\/v[^/]+\/LICENSE/g, () => `raw.githubusercontent.com/Fast-Editor/Caret/v${version}/LICENSE`, f)
  const order = [hash.deb, licenseHash]
  let i = 0
  t = sub(t, /^(\tsha256sums = ).*$/gm, (_, p) => p + (order[i++] ?? PLACEHOLDER), f)
  return t
})

// Nix flake: one version and two hex hashes in the let block.
rewrite('flake.nix', (t, f) => {
  t = sub(t, /^(\s*version = ")[^"]*(";)/m, (_, a, b) => a + version + b, f)
  t = sub(t, /^(\s*aarch64-darwin = ")[^"]*(";)/m, (_, a, b) => a + hash.dmgArm + b, f)
  t = sub(t, /^(\s*x86_64-linux = ")[^"]*(";)/m, (_, a, b) => a + hash.appImage + b, f)
  return t
})

for (const m of missing) console.warn(`warning: ${m} is not in ${sumsPath}; wrote ${PLACEHOLDER}`)
console.log(changed.length ? `updated for v${version}:\n  ${changed.join('\n  ')}` : `already up to date for v${version}`)

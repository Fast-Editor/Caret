#!/usr/bin/env node
// Bump the app version everywhere it lives and create the release commit + tag.
//   npm run release:bump 0.2.0          → edits package.json, package-lock.json, src-tauri/tauri.conf.json,
//                                          src-tauri/Cargo.toml, src-tauri/Cargo.lock; commits "release: v0.2.0"; tags v0.2.0
//   git push && git push --tags         → the Release workflow builds and drafts the GitHub Release
import { readFileSync, writeFileSync } from 'node:fs'
import { execSync } from 'node:child_process'

const version = process.argv[2]
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?$/.test(version ?? '')) { console.error('usage: node scripts/bump-version.mjs <semver>'); process.exit(2) }

const editJson = (file, fn) => { const j = JSON.parse(readFileSync(file, 'utf8')); fn(j); writeFileSync(file, JSON.stringify(j, null, 2) + '\n') }
editJson('package.json', (j) => { j.version = version })
editJson('package-lock.json', (j) => { j.version = version; if (j.packages?.['']) j.packages[''].version = version })
editJson('src-tauri/tauri.conf.json', (j) => { j.version = version })
const cargo = readFileSync('src-tauri/Cargo.toml', 'utf8').replace(/^version = "[^"]+"/m, `version = "${version}"`)
writeFileSync('src-tauri/Cargo.toml', cargo)
const lock = readFileSync('src-tauri/Cargo.lock', 'utf8').replace(/(name = "caret"\nversion = ")[^"]+(")/, `$1${version}$2`)
writeFileSync('src-tauri/Cargo.lock', lock)

const changelog = readFileSync('CHANGELOG.md', 'utf8')
if (!changelog.includes(`## ${version}`)) console.warn(`warning: CHANGELOG.md has no "## ${version}" section yet`)

if (!process.argv.includes('--no-git')) {
  execSync('git add package.json package-lock.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock CHANGELOG.md', { stdio: 'inherit' })
  execSync(`git commit -m "release: v${version}"`, { stdio: 'inherit' })
  execSync(`git tag -a v${version} -m "Caret v${version}"`, { stdio: 'inherit' })
  console.log(`\nTagged v${version}. Now: git push && git push --tags`)
}

# Releasing Caret

Releases are built by GitHub Actions and published as GitHub Releases with installers, updater artifacts, `latest.json`
and `SHA256SUMS.txt`. Everything is driven by a git tag.

## Cut a release

```bash
# 1. add a "## X.Y.Z — YYYY-MM-DD" section to CHANGELOG.md
npm run release:bump 0.2.0        # edits package.json, package-lock.json, tauri.conf.json, Cargo.toml, Cargo.lock; commits; tags v0.2.0
git push && git push --tags       # → .github/workflows/release.yml
```

The workflow builds four targets in parallel and attaches them to a **draft** release named `Caret v0.2.0`:

| Target | Runner | Artifacts | Status |
| --- | --- | --- | --- |
| macOS Apple Silicon | macos-latest | `Caret_0.2.0_aarch64.dmg`, `Caret.app.tar.gz` + `.sig` (updater) | required |
| macOS Intel | macos-latest (`--target x86_64-apple-darwin`) | `Caret_0.2.0_x64.dmg`, updater tarball | required |
| Linux x64 | ubuntu-22.04 | `.deb`, `.rpm`, `.AppImage` + `.sig` | best effort (`continue-on-error`) |
| Windows x64 | windows-latest | `.msi`, `-setup.exe` + `.sig` | best effort |

Then `tauri-action` writes `latest.json` (the updater manifest) and a final job attaches `SHA256SUMS.txt`. Review the
draft, edit notes if needed, and **publish** it — the in-app updater only sees published releases.

CI (`ci.yml`) runs typecheck, vitest and `cargo test --lib` on every push and pull request.

## Secrets

| Secret | Purpose | Required |
| --- | --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | minisign private key that signs updater artifacts (`latest.json` signatures) | yes |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | its password | yes |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` | Developer ID Application certificate (base64 `.p12`) for real code signing | no — without them builds are ad-hoc signed |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | notarization (app-specific password) | no |
| `WINDOWS_CERTIFICATE`, `WINDOWS_CERTIFICATE_PASSWORD` | Authenticode `.pfx` (base64) | no — without it SmartScreen shows "unknown publisher" |

The updater key pair was generated with `npx tauri signer generate -w ~/.tauri/caret.key`; the public key lives in
`src-tauri/tauri.conf.json` under `plugins.updater.pubkey`. **Losing the private key means existing installs can no
longer verify updates** — keep `~/.tauri/caret.key` and its password backed up.

## Signing state

`bundle.macOS.signingIdentity` is `"-"`: every build is ad-hoc signed with the bundle identifier `dev.caret.editor` as
its identity, so macOS keeps privacy grants and caches across updates. Browser downloads still get the quarantine
attribute, which makes Gatekeeper refuse ad-hoc apps ("damaged"); the README documents the one-time `xattr` fix, and
`scripts/install.sh` / the Homebrew cask (`--no-quarantine`) avoid it entirely. Adding the Apple secrets above switches
the workflow to Developer ID signing + notarization automatically; remove the `signingIdentity: "-"` override then.

## Updater

`tauri-plugin-updater` checks `https://github.com/Fast-Editor/Caret/releases/latest/download/latest.json` on demand
(Settings → About → *Check for updates*, or the *Check for Updates…* command) and silently 15 s after launch in release
builds. Updates are downloaded by the app itself (no quarantine), verified against the public key and installed in place,
then the app relaunches. Dev builds report "updates unavailable".

## Homebrew cask

`Casks/caret.rb` in the `Fast-Editor/homebrew-lynkr` tap points at the latest DMG; bump `version` and `sha256` (from
`SHA256SUMS.txt`) after each release, or let `bump.sh` there do it.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| `A public key has been found, but no private key` during a local `npm run build` | export `TAURI_SIGNING_PRIVATE_KEY=~/.tauri/caret.key` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (or temporarily set `bundle.createUpdaterArtifacts` to false) |
| DMG step fails with `hdiutil: Resource busy` | known flaky step; re-run the job — the `.app` is complete before it |
| Linux/Windows job fails | expected until those targets are exercised; macOS artifacts still publish |
| Updater says signature invalid | the release was built with a different private key than the public key in the shipped app |

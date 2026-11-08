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
| Linux x64 | ubuntu-22.04 | `Caret_X.Y.Z_amd64.deb`, `Caret-X.Y.Z-1.x86_64.rpm`, `Caret_X.Y.Z_amd64.AppImage` + `.sig` | builds green since v0.1.0; runtime untested (`continue-on-error`) |
| Windows x64 | windows-latest | `Caret_X.Y.Z_x64_en-US.msi`, `Caret_X.Y.Z_x64-setup.exe` + `.sig` | builds green since v0.1.0; runtime untested (`continue-on-error`) |

Then `tauri-action` writes `latest.json` (the updater manifest), the `checksums` job attaches `SHA256SUMS.txt`, and the
`packaging` job rewrites the package manager manifests from it and commits them to `main` (see
[Package managers](#package-managers)). Review the draft, edit notes if needed, and **publish** it; the in-app updater
only sees published releases.

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
`scripts/install.sh` and the Homebrew cask avoid it entirely (Homebrew 7 no longer applies the quarantine attribute). Adding the Apple secrets above switches
the workflow to Developer ID signing + notarization automatically; remove the `signingIdentity: "-"` override then.

## Updater

`tauri-plugin-updater` checks `https://github.com/Fast-Editor/Caret/releases/latest/download/latest.json` on demand
(Settings → About → *Check for updates*, or the *Check for Updates…* command) and silently 15 s after launch in release
builds. Updates are downloaded by the app itself (no quarantine), verified against the public key and installed in place,
then the app relaunches. Dev builds report "updates unavailable".

## Package managers

User-facing commands and the publication status of each channel are in [INSTALL.md](INSTALL.md). This section is the
maintainer side.

### Homebrew cask

`Casks/caret.rb` in the `Fast-Editor/homebrew-caret` tap (`brew tap fast-editor/caret`) points at the latest DMG; bump `version` and `sha256` (from
`SHA256SUMS.txt`) after each release, or let `bump.sh` there do it. This is the one manifest that lives outside this
repository.

### Manifests in this repository

| File | Channel | Asset |
| --- | --- | --- |
| `packaging/winget/FastEditor.Caret.yaml`, `.installer.yaml`, `.locale.en-US.yaml` | winget (schema 1.6, id `FastEditor.Caret`) | `Caret_X.Y.Z_x64-setup.exe`, silent `/S` |
| `packaging/scoop/caret.json` | scoop, with `checkver` and `autoupdate` | `Caret_X.Y.Z_x64-setup.exe` |
| `packaging/aur/PKGBUILD`, `.SRCINFO` | AUR `caret-bin`, `provides=('caret')` | `Caret_X.Y.Z_amd64.deb` plus `LICENSE` at the tag |
| `flake.nix` | Nix, `packages.aarch64-darwin.default` and `packages.x86_64-linux.default` | `Caret_X.Y.Z_aarch64.dmg`, `Caret_X.Y.Z_amd64.AppImage` |

`scripts/update-packaging.mjs` rewrites the version and sha256 fields of all of them from a `SHA256SUMS.txt`:

```bash
gh release download v0.2.0 -p SHA256SUMS.txt --repo Fast-Editor/Caret -O /tmp/SHA256SUMS.txt
npm run release:packaging -- 0.2.0 /tmp/SHA256SUMS.txt     # regex based and idempotent; missing assets become REPLACE_WITH_SHA256
```

The release workflow's `packaging` job (`needs: checksums`, `continue-on-error: true`) does exactly that after the
checksums are attached and commits the result to `main` as `github-actions[bot]` with the message
`release: packaging manifests for vX.Y.Z`. It is best effort: if `main` is protected or an asset is missing it fails
without affecting the release, and you run the two commands above locally and commit. A tag pushed from a commit that is
not on `main` still gets the manifests, because the job checks out `main` explicitly.

### Manual steps per channel

- **winget:** the first version needs a pull request to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs):
  copy the three files to `manifests/f/FastEditor/Caret/X.Y.Z/`, run `winget validate manifests/f/FastEditor/Caret/X.Y.Z`
  and open the PR. Later versions can use `wingetcreate update FastEditor.Caret --version X.Y.Z --urls <setup.exe url> --submit`.
  The `ProductCode` in the installer manifest is the bundle identifier (`dev.caret.editor`), which is the uninstall
  registry key Tauri's NSIS template writes; confirm it on a Windows machine the first time.
- **scoop:** `scoop install <raw URL>` works from this repository without a bucket. To get a plain `scoop install caret`,
  either submit `caret.json` to [ScoopInstaller/Extras](https://github.com/ScoopInstaller/Extras) or create a
  `Fast-Editor/scoop-bucket` repository; `checkver` and `autoupdate` let the bucket's excavator bump it on its own.
- **AUR:** create an AUR account with an SSH key, then `git clone ssh://aur@aur.archlinux.org/caret-bin.git`, copy in
  `PKGBUILD` and `.SRCINFO`, regenerate `.SRCINFO` with `makepkg --printsrcinfo > .SRCINFO`, test with `makepkg -si`
  and push. Each release afterwards is the same copy, regenerate, push cycle.
- **Nix:** run `nix flake lock` once on a machine with Nix and commit `flake.lock` so `nix run github:Fast-Editor/Caret`
  does not have to resolve nixpkgs at run time; verify hashes with `nix build .#packages.aarch64-darwin.default`
  (or `x86_64-linux`). Submitting to nixpkgs is a separate decision.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| `A public key has been found, but no private key` during a local `npm run build` | export `TAURI_SIGNING_PRIVATE_KEY=~/.tauri/caret.key` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (or temporarily set `bundle.createUpdaterArtifacts` to false) |
| DMG step fails with `hdiutil: Resource busy` | known flaky step; re-run the job — the `.app` is complete before it |
| Linux/Windows job fails | they compile today but are not exercised at runtime; macOS artifacts still publish |
| macOS job fails with `security import -f pkcs12` | an Apple signing variable reached the bundler empty — the export step only forwards secrets that are set; check the secret names |
| Updater says signature invalid | the release was built with a different private key than the public key in the shipped app |

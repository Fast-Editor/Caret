# Installing Caret

Every channel that installs Caret, with the exact commands. Installers for all three platforms are built by CI from
the release tag ([RELEASING.md](RELEASING.md)); after the first install, Caret updates itself from Settings → About.

## What is published today

| Channel | Platform | Status |
| --- | --- | --- |
| Install script | macOS | published |
| Homebrew cask | macOS | published, tap `fast-editor/caret` |
| Direct downloads (`.dmg`, `-setup.exe`, `.msi`, `.deb`, `.rpm`, `.AppImage`) | all | published on every [release](https://github.com/Fast-Editor/Caret/releases) |
| In-app updater | all | published |
| Nix flake | macOS (Apple Silicon), Linux x86_64 | in this repository; works from `main`, not in nixpkgs |
| scoop | Windows | manifest in this repository; not yet in a bucket |
| winget | Windows | manifest in this repository; awaiting the first winget-pkgs submission |
| AUR (`caret-bin`) | Arch Linux | `PKGBUILD` in this repository; awaiting the first AUR publication |

The manifests live in [`packaging/`](../packaging) and [`flake.nix`](../flake.nix); a release workflow job keeps their
version and checksum fields current. Windows and Linux builds compile green but have had far less runtime testing
than macOS, and the Windows and Linux package manifests have not been exercised on a real machine yet. Please report
rough edges on [GitHub](https://github.com/Fast-Editor/Caret/issues).

## macOS

Requires macOS 12 or later. Apple Silicon and Intel builds are both released.

### Install script

```bash
curl -fsSL https://raw.githubusercontent.com/Fast-Editor/Caret/main/scripts/install.sh | sh
```

[`scripts/install.sh`](../scripts/install.sh) resolves the latest release, downloads the DMG for your CPU, copies
`Caret.app` into `/Applications` (or `~/Applications` when `/Applications` is not writable and no password can be
asked; set `CARET_INSTALL_DIR` to choose) and removes the quarantine attribute, so there is no Gatekeeper dialog.

### Homebrew

```bash
brew tap fast-editor/caret
brew trust fast-editor/caret   # Homebrew 7 refuses untrusted third-party taps until you confirm once
brew install --cask caret
```

Homebrew 7 or later no longer quarantines cask downloads, so the app opens straight away. The cask lives in the
[Fast-Editor/homebrew-caret](https://github.com/Fast-Editor/homebrew-caret) tap and is bumped after each release. The copy in the older `fast-editor/lynkr` tap is deprecated and points here; `brew upgrade` keeps working there, but new installs should use `fast-editor/caret`.

### DMG

Download `Caret_X.Y.Z_aarch64.dmg` (Apple Silicon) or `Caret_X.Y.Z_x64.dmg` (Intel) from the
[latest release](https://github.com/Fast-Editor/Caret/releases/latest) and drag Caret to Applications. Builds are
ad-hoc signed until a Developer ID is configured, so a browser-downloaded copy is quarantined and macOS reports
*"Caret is damaged and can't be opened"*. Clear it once:

```bash
xattr -dr com.apple.quarantine /Applications/Caret.app
```

or use System Settings → Privacy & Security → *Open Anyway*. Updates installed from inside the app are not quarantined.

## Windows

Requires Windows 10 or later, x64. The installer is unsigned until a certificate is configured, so SmartScreen shows
*More info → Run anyway* once.

### winget

```powershell
winget install FastEditor.Caret
```

Manifest awaiting first publication. The three manifest files (schema 1.6, package id `FastEditor.Caret`, x64 NSIS
installer, silent switch `/S`) are in [`packaging/winget/`](../packaging/winget); the command works once they have
been merged into [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs). Until then, install the manifests
directly from a checkout with `winget install --manifest packaging/winget` (needs `winget settings --enable LocalManifestFiles`).

### scoop

```powershell
scoop install https://raw.githubusercontent.com/Fast-Editor/Caret/main/packaging/scoop/caret.json
```

Installs from the manifest in this repository without adding a bucket. [`packaging/scoop/caret.json`](../packaging/scoop/caret.json)
runs the NSIS installer silently into scoop's app directory and carries `checkver` and `autoupdate` entries, so a
bucket that adopts it keeps itself current. Not yet in a scoop bucket.

### Direct download

`Caret_X.Y.Z_x64-setup.exe` (NSIS, per-user, no admin rights) or `Caret_X.Y.Z_x64_en-US.msi` from the
[latest release](https://github.com/Fast-Editor/Caret/releases/latest). Both installers register the app so the
in-app updater can replace it.

## Linux

Requires WebKitGTK 4.1 and GTK 3; sandboxing of agent commands uses bubblewrap when it is installed.

### Debian, Ubuntu, Fedora, AppImage

```bash
sudo apt install ./Caret_X.Y.Z_amd64.deb        # Debian, Ubuntu
sudo dnf install ./Caret-X.Y.Z-1.x86_64.rpm      # Fedora, RHEL
chmod +x Caret_X.Y.Z_amd64.AppImage && ./Caret_X.Y.Z_amd64.AppImage
```

The files are on the [latest release](https://github.com/Fast-Editor/Caret/releases/latest). The `.deb` installs
`/usr/bin/caret`, a desktop entry and icons. All three formats ship with updater signatures.

### AUR

```bash
yay -S caret-bin        # or paru -S caret-bin
```

Awaiting first publication. [`packaging/aur/PKGBUILD`](../packaging/aur/PKGBUILD) and `.SRCINFO` build `caret-bin`
from the released `.deb` (`provides=('caret')`, depends on `webkit2gtk-4.1`, `gtk3`, `libappindicator-gtk3`). Until the
package exists on the AUR, build it from a checkout:

```bash
cd packaging/aur && makepkg -si
```

### Nix flake

```bash
nix run github:Fast-Editor/Caret                   # run without installing
nix profile install github:Fast-Editor/Caret       # install into your profile
```

[`flake.nix`](../flake.nix) exposes `packages.aarch64-darwin.default` (the DMG unpacked into `$out/Applications`,
plus a `caret` launcher in `$out/bin`) and `packages.x86_64-linux.default` (the AppImage wrapped with
`appimageTools.wrapType2`), each pinned to a release version and sha256. It is not in nixpkgs; it works from this
repository as soon as the flake is on `main`.

## Direct downloads and checksums

Every release attaches `SHA256SUMS.txt`. Verify a download with:

```bash
curl -fsSLO https://github.com/Fast-Editor/Caret/releases/download/vX.Y.Z/SHA256SUMS.txt
shasum -a 256 -c SHA256SUMS.txt --ignore-missing     # macOS; use sha256sum on Linux
```

Updater artifacts (`*.app.tar.gz`, `*.AppImage`, `*.deb`, `*.rpm`, `*-setup.exe`, `*.msi`) also carry a minisign
`.sig` that the app checks against the public key compiled into it.

## Updating

- **In the app:** Settings → About → *Check for updates* (also checked silently 15 s after launch in release builds).
  The update is downloaded by the app itself, verified and installed in place, then Caret relaunches.
- **Homebrew:** `brew upgrade --cask caret`.
- **winget / scoop / AUR / Nix:** `winget upgrade FastEditor.Caret`, `scoop update caret`, `yay -Syu`, or rebuild from
  the flake after it has been bumped. The manifests are updated by the release workflow, so a package manager and the
  in-app updater always point at the same build.

## Uninstalling

Remove `/Applications/Caret.app` (macOS), use Apps → Installed apps or `winget uninstall FastEditor.Caret` /
`scoop uninstall caret` (Windows), or your package manager (Linux). User data stays in `~/.caret/`; delete it to start
over. See [README.md](README.md) for what lives there.

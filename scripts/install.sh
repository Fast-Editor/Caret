#!/bin/sh
# Caret installer for macOS: downloads the latest release DMG for this Mac, copies Caret.app to /Applications
# and removes the quarantine flag so Gatekeeper does not report the ad-hoc signed app as "damaged".
#   curl -fsSL https://raw.githubusercontent.com/Fast-Editor/Caret/main/scripts/install.sh | sh
set -eu
REPO="Fast-Editor/Caret"
case "$(uname -s)" in Darwin) ;; *) echo "This installer is for macOS. Linux and Windows builds: https://github.com/$REPO/releases"; exit 1;; esac
ARCH=$(uname -m); case "$ARCH" in arm64) PAT="aarch64";; x86_64) PAT="x64";; *) echo "unsupported architecture $ARCH"; exit 1;; esac
# Resolve the latest version without the GitHub API (which rate-limits anonymous callers): follow the
# releases/latest redirect to .../tag/vX.Y.Z, falling back to the updater manifest.
TAG=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest" 2>/dev/null | sed -n 's#.*/tag/##p')
[ -n "$TAG" ] || TAG=v$(curl -fsSL "https://github.com/$REPO/releases/latest/download/latest.json" | sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)
VERSION=${TAG#v}; [ -n "$VERSION" ] || { echo "could not determine the latest Caret version"; exit 1; }
URL="https://github.com/$REPO/releases/download/$TAG/Caret_${VERSION}_${PAT}.dmg"
TMP=$(mktemp -d); trap 'hdiutil detach "$TMP/mnt" -quiet 2>/dev/null || true; rm -rf "$TMP"' EXIT
echo "Downloading $(basename "$URL")…"; curl -fL --progress-bar "$URL" -o "$TMP/caret.dmg"
mkdir -p "$TMP/mnt"; hdiutil attach "$TMP/caret.dmg" -nobrowse -quiet -mountpoint "$TMP/mnt"
APP=$(find "$TMP/mnt" -maxdepth 1 -name '*.app' | head -1); [ -n "$APP" ] || { echo "no .app inside the DMG"; exit 1; }
# Destination: $CARET_INSTALL_DIR, else /Applications when writable, else (sudo if a terminal is available) else ~/Applications.
NAME=$(basename "$APP")
if [ -n "${CARET_INSTALL_DIR:-}" ]; then DIR="$CARET_INSTALL_DIR"; mkdir -p "$DIR"; DEST="$DIR/$NAME"; rm -rf "$DEST"; cp -R "$APP" "$DEST"
elif [ -w /Applications ]; then DEST="/Applications/$NAME"; rm -rf "$DEST"; cp -R "$APP" "$DEST"
elif [ -r /dev/tty ] && sudo -p "Password (to install into /Applications): " -v </dev/tty 2>/dev/null; then DEST="/Applications/$NAME"; sudo rm -rf "$DEST"; sudo cp -R "$APP" "$DEST"; sudo chown -R "$(id -u):$(id -g)" "$DEST"
else DIR="$HOME/Applications"; mkdir -p "$DIR"; DEST="$DIR/$NAME"; rm -rf "$DEST"; cp -R "$APP" "$DEST"; echo "(/Applications is not writable without a password; installed into $DIR instead)"
fi
xattr -dr com.apple.quarantine "$DEST" 2>/dev/null || true
echo "Installed $DEST. Launch it from Spotlight, or install the \`caret\` command from its command palette."

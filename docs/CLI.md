# `caret` command line

Open folders and files in Caret from any terminal, like VS Code's `code`.

```sh
caret                  # launch or focus Caret
caret .                # open the current folder as the workspace
caret src/app.ts       # open a file (its folder becomes the workspace when none is open)
caret -g app.ts:12:4   # open at line 12, column 4 — a bare app.ts:12:4 works too
caret -v | -h          # version / help
```

`-n/--new-window`, `-r/--reuse-window`, `-w/--wait` and `-a/--add` are accepted for muscle-memory compatibility and
ignored (Caret is a single-window app and does not block). Unknown flags exit 2; a missing path prints
`caret: no such file or directory` and exits 1 after opening the others.

## Install

Command palette → **Shell Command: Install 'caret' command in PATH** (also under Settings → General → Command line).

What it does, exactly what VS Code does:

1. Renders `src-tauri/cli/caret.sh` (embedded in the binary) with the bundle id, the app path and the version to
   `~/.caret/bin/caret` (mode 0755).
2. Links it as `caret` from `/usr/local/bin` when that is writable, otherwise from a user-writable
   directory already on your login shell's PATH (`~/.local/bin`, `/opt/homebrew/bin`, `~/bin`) so no
   password is needed. Only when none exists does macOS show the administrator dialog to link
   `/usr/local/bin/caret` (what VS Code does). Uninstall removes every such link and keeps the script.

The script is rewritten on every Caret start when its content is stale, so moving or updating the app never breaks
the command. **Uninstall** removes the symlink and keeps the script.

## How it works

The script resolves each argument to an absolute, symlink-resolved path and hands it to LaunchServices:

* plain paths → `open -b dev.caret.editor <paths…>` (Apple's open-documents event; the bundle declares folders and
  text/source documents in `src-tauri/Info.plist`),
* positions → `open -b dev.caret.editor "caret://open?path=<url-encoded>&line=N&col=M"` (the bundle registers the
  `caret` URL scheme).

If the bundle is not registered yet (first run after a fresh build), it falls back to `open -a <app path>`.

In the app, `tauri::RunEvent::Opened` (`src-tauri/src/core/cli.rs::on_opened`) parses the URLs into
`OpenRequest { path, line?, col? }`, pushes them on the `OpenQueue` and emits `caret://open-paths`. The UI
(`src/lib/cliOpen.ts`) drains the queue — at boot and on every poke — so a cold start and a running app behave the
same: a folder becomes the workspace; a file opens in the editor, opening its folder as the workspace first when
none is open. Arguments passed directly to the binary (`Caret.app/Contents/MacOS/caret .`) are parsed the same way
at startup.

`CARET_CLI_DRY_RUN=1 caret . a.ts:3` prints the `open` commands instead of running them (used by the Rust tests).

Command ids: `cli.install`, `cli.uninstall`, `cli.status` (hidden), `cli.open` (hidden; `{ path, line?, col? }`).

Limitations: macOS only for now (Linux/Windows fall back to starting the binary with the paths as arguments);
positions are honoured for files that exist; a brand-new file name is not created.

# `caret` command line

Open folders and files in Caret from any terminal, like VS Code's `code`.

```sh
caret                  # launch or focus Caret
caret .                # open the current folder as the workspace
caret src/app.ts       # open a file (its folder becomes the workspace when none is open)
caret -g app.ts:12:4   # open at line 12, column 4 — a bare app.ts:12:4 works too
caret -v | -h          # version / help
caret serve            # run Caret and serve its UI to browsers on your LAN / Tailscale (docs/REMOTE.md)
```

`-n/--new-window`, `-r/--reuse-window`, `-w/--wait` and `-a/--add` are accepted for muscle-memory compatibility and
ignored (which window a path opens in is decided by the rules below, and Caret does not block). Unknown flags exit 2;
a missing path prints `caret: no such file or directory` and exits 1 after opening the others.

## Which window opens it

With several Caret windows open (docs/MULTI_WINDOW.md), every path from `caret <path>`, Finder "Open with" and
`caret://open?…` is routed to one window by `route_open` in `src-tauri/src/core/cli.rs`. The rules, tried in this
order for each path:

1. **A folder that a window already shows** focuses that window (the focused one when two windows show it, else the
   oldest). Nothing is reopened.
2. **A file inside an open folder** opens in the window showing that folder; with nested folders the deepest one
   wins (`/repo/packages/app` beats `/repo` for `/repo/packages/app/src/x.ts`), then the focused window, then the
   oldest.
3. **A folder while the focused window is empty** (Welcome screen) opens in that window.
4. **Any other folder** opens in a **new window** when `window.openFoldersIn` is `"new"` (the default) and replaces
   the focused window's folder when it is `"current"`. A subfolder of an open folder (`caret packages/app` while the
   window shows the repo) is a folder request like any other, not a file inside the repo window. One folder per
   window per invocation: `caret a b` with `"current"` opens `a` in place and `b` in a new window.
5. **A loose file** (under no open folder) opens in the focused window. When Caret has no window at all (macOS after
   the last one closed), loose files share one new window.

Files given together with their folder stay with it: `caret ~/proj ~/proj/src/a.ts:12` opens `~/proj` (rule 3 or 4)
and `a.ts` in that same window, whether it is the focused window or a brand-new one. A window created for a folder
starts with the folder already open and receives the files once its UI is up. Each request is handed to exactly one
window: the backend keeps one queue per window label and a window only drains its own (`cli_take_pending_opens`).

`window.openFoldersIn` (Settings → Window, `"new"` | `"current"`) is the same setting that decides where Open Folder /
Open Recent put a folder from a window that already shows one; it only matters for rule 4.

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
the command. "Stale" means the file differs byte-for-byte from the template rendered by the build that is starting
(there is no version gate), so a launcher fix in a newer Caret reaches `~/.caret/bin/caret` the first time that build
starts, and a dev build (`cargo tauri dev`) rewrites it with its own `APP_PATH` (the bare `target/debug/caret`
binary) until a bundled Caret starts again. **Uninstall** removes the symlink and keeps the script.

## How it works

The script resolves each argument to an absolute, symlink-resolved path and hands it to LaunchServices with `open`:

* plain paths → `open <target> <paths…>` (Apple's open-documents event; the bundle declares folders and text/source
  documents in `src-tauri/Info.plist`),
* positions → `open <target> "caret://open?path=<url-encoded>&line=N&col=M"` (the bundle registers the `caret` URL
  scheme).

`<target>` picks the copy of Caret that receives the event, in this order:

1. `-a <bundle of the running Caret>`: the script asks LaunchServices which copy owns the bundle id right now
   (`lsappinfo info -only bundlepath dev.caret.editor`, with `pgrep -fl "Caret.app/Contents/MacOS/caret"` as the
   fallback). A running copy always wins, so the paths land in the window you already have.
2. `-a <APP_PATH>`: nothing is running, so the copy this script was rendered for starts, when it is still an app
   bundle on disk.
3. `-b dev.caret.editor`: the bundle id, for a first run after a fresh build or a dev-rendered script whose
   `APP_PATH` is a bare binary. If the chosen target fails, the other two are tried before the script gives up.

`CARET_CLI_RUNNING_APP=/path/Caret.app` pins step 1 (an empty value means "nothing running"); the Rust tests use it so
dry runs never depend on LaunchServices.

In the app, `tauri::RunEvent::Opened` (`src-tauri/src/core/cli.rs::on_opened`) parses the URLs into
`OpenRequest { path, line?, col? }`, runs `route_open` over the window registry and `window.openFoldersIn` (see
"Which window opens it"), creates a window for each `NewWindow` group (with its folder already open), pushes every
group on that window's `OpenQueue` and emits `caret://open-paths` to that window only, then focuses it. The UI
(`src/lib/cliOpen.ts`) drains its own queue — at boot and on every poke — so a cold start, a running app and a window
created for the request behave the same: a folder routed to a window opens there (or just focuses it when it is
already that window's folder); a file opens in the editor, opening its folder as the workspace first when the window
has none. Arguments passed directly to the binary (`Caret.app/Contents/MacOS/caret .`) are routed the same way at
startup (the `main` window is the focused, empty window at that point).

`CARET_CLI_DRY_RUN=1 caret . a.ts:3` prints the `open` commands instead of running them, including the target that
would be chosen on this machine right now (used by the Rust tests together with `CARET_CLI_RUNNING_APP`).

## Two copies of Caret

macOS lets several app bundles register the same bundle id. That happens easily with Caret: a copy dragged from the DMG
to `/Applications`, the Homebrew cask in `~/Applications`, a `cargo tauri build` under `src-tauri/target*/`, and
LaunchServices also remembers every mounted DMG (`mdfind "kMDItemCFBundleIdentifier == 'dev.caret.editor'"` and
`lsregister -dump | grep Caret.app` list them). With plain `open -b dev.caret.editor`, LaunchServices chooses one of
those copies by its own rules, and when it picks a copy that is not running you wait for a cold start and get a second
Caret *process* next to the one you were working in (not a second window of it: the window routing above only sees
the windows of one process).

The launcher therefore prefers the running copy (step 1 above), whichever path it lives at; `caret .` then arrives as an
open event in that window. To stop the ambiguity for Finder and other apps as well, keep one copy:

* Installed with Homebrew and also copied by hand: remove the hand-copied one (`rm -rf /Applications/Caret.app`,
  after quitting it) and keep `~/Applications/Caret.app`, which `brew upgrade --cask caret` maintains; or run
  `brew uninstall --cask caret` and keep the one in `/Applications` (your `~/.caret` data is untouched either way;
  only `brew zap` removes it).
* Stale registrations for ejected DMGs and deleted build directories only vanish when the LaunchServices database is
  rebuilt: `/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -kill -r -domain local -domain system -domain user`
  (safe; it re-scans the standard folders), or unregister one path with `lsregister -u <path/to/Caret.app>`. An old
  build still on disk (`src-tauri/target*/release/bundle/macos/Caret.app`) stays registered until you trash it.

`lsappinfo info -only bundlepath,pid dev.caret.editor` shows which copy is running at the moment, and
`CARET_CLI_DRY_RUN=1 caret .` shows the `open` command the launcher would run against it.

## `caret serve`

```sh
caret serve [--host 127.0.0.1|0.0.0.0] [--port N] [--no-window] [--print-url] [paths...]
```

Starts Caret and serves its UI to browsers (see [REMOTE.md](REMOTE.md)). Unlike plain `caret`, which hands paths to
the running app through LaunchServices, `serve` **execs the app binary directly**
(`Caret.app/Contents/MacOS/caret serve …`) so the URL and pairing code print in your terminal and Ctrl-C stops the
server together with the app:

```
Caret remote: http://192.168.1.20:17430  pairing code: 482913 (valid 5 min)
Caret remote: http://100.101.102.103:17430  pairing code: 482913 (valid 5 min)
Caret remote: http://127.0.0.1:17430  pairing code: 482913 (valid 5 min)
```

* `--host` binds `127.0.0.1` (default, this Mac only) or `0.0.0.0` (every interface: LAN and Tailscale);
  `--port` defaults to 17430. Both override `Settings.remote` for this process only.
* `--no-window` hides the desktop window after start-up. The webview keeps running hidden (the relay needs it), so
  the window still exists; show it again from the Dock.
* `--print-url` prints just the URL(s), one per line, without a pairing code (pair from Settings → Remote).
* Any other arguments are paths and open like `caret <path>`; the app's own `parse_open_args` skips `serve` and its
  options. If Caret is already running, `caret serve` starts a second instance; quit the first one first.

In the app, `remote::strip_serve_args` (src-tauri/src/remote/mod.rs) parses the options at start-up and
`remote::init` starts the server during setup.

Command ids: `cli.install`, `cli.uninstall`, `cli.status` (hidden), `cli.open` (hidden; `{ path, line?, col? }`).
Remote commands (`api.remote.*`) are listed in [REMOTE.md](REMOTE.md).

Limitations: macOS only for now (Linux/Windows fall back to starting the binary with the paths as arguments);
positions are honoured for files that exist; a brand-new file name is not created.

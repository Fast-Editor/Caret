# Run and Debug

Caret includes a Debug Adapter Protocol (DAP) client. Adapters are installed on demand under `~/.caret/debug/` —
never globally — and launched per session.

## What works (v0.2)

| Language | Adapter | Launch | Attach | Notes |
| --- | --- | --- | --- | --- |
| JavaScript / TypeScript (Node) | [vscode-js-debug](https://github.com/microsoft/vscode-js-debug) (`js-debug-dap` release tarball) | ✅ `program`, `runtimeExecutable` (e.g. `npm run <script>`) | ✅ `port` (default 9229) | TypeScript via source maps / `ts-node` as in VS Code; needs Node ≥ 18 |
| Python | [debugpy](https://github.com/microsoft/debugpy) (`pip install --target ~/.caret/debug/python`) | ✅ `program`, `module` | ⏳ | `justMyCode` honoured; needs `python3` |

Breakpoints (gutter click or **F9**), step over/into/out, continue/pause, restart, stop, variables (lazy tree), watch
expressions, call stack, the Debug Console (program output + REPL `evaluate`), and the current-line highlight.

Not yet: conditional/logpoint breakpoints, exception breakpoint filters, hover evaluation in the editor, `runInTerminal`
(use `"console": "internalConsole"`), multi-session UI beyond js-debug's own child sessions, Rust/C/Go adapters.

## Using it

1. Open the **Run and Debug** view (activity bar, or ⌘⇧D).
2. Pick a configuration. Caret lists `.caret/launch.json` and `.vscode/launch.json` configurations first, then
   auto-detected ones: *Run current file (Node/Python)*, one entry per `package.json` script, and *Attach to Node
   process (9229)*.
3. If the adapter for that configuration is missing, an **Install** button appears (js-debug downloads the latest
   release tarball from GitHub; debugpy is pip-installed into the Caret folder).
4. Set breakpoints by clicking the gutter, press **F5**. The floating toolbar appears at the top of the editor:
   continue/pause, step over (F10), step into (F11), step out (⇧F11), restart (⌘⇧F5), stop (⇧F5).
5. Output and the REPL live in the **Debug Console** panel tab (also `Debug: Open Debug Console`).

## launch.json compatibility

Files are JSONC (comments and trailing commas allowed). Supported variables: `${workspaceFolder}`,
`${workspaceFolderBasename}`, `${file}`, `${fileBasename}`, `${fileBasenameNoExtension}`, `${fileDirname}`,
`${fileExtname}`, `${relativeFile}`, `${cwd}`, `${env:NAME}`. Unknown variables are left as-is. `type: node` is sent to
js-debug as `pwa-node`; `type: python` as `debugpy`. All other properties are passed to the adapter unchanged, so the
VS Code documentation for each adapter applies.

## How it works

`src-tauri/src/debug/`: `protocol.rs` (Content-Length framing), `client.rs` (one connection: request/response
correlation by `seq`, events and reverse requests), `adapters.rs` (install + launch: js-debug as a TCP DAP server,
debugpy over stdio), `configs.rs` (launch.json discovery + substitution), `session.rs` (initialize → launch →
`initialized` → setBreakpoints/configurationDone; js-debug's `startDebugging` reverse request opens child connections
whose thread/frame/variable ids are connection-qualified so the UI sees one session), `commands.rs`. Events reach the
frontend as `caret://debug/event`; the store is `src/store/debug.ts`, the UI `src/components/debug/`, Monaco glyphs in
`src/lib/monaco/debugDecorations.ts`. Breakpoints persist in local storage and are pushed to every new session.

## Tests

`cargo test --lib debug::` runs the framing/config tests and a full lifecycle against
`src-tauri/tests/fixtures/fake_dap_adapter.mjs` (needs `node`). `CARET_LIVE_TEST=1 cargo test --lib debug::tests::live`
drives the real js-debug from the managed install (run **Install** once first).

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| "js-debug is not installed" | Run and Debug → Adapters → Install (needs Node and network once) |
| Breakpoint shows hollow / "not verified" | the adapter could not map the line (compiled output without source maps, or the file is not loaded yet) |
| Python: `import debugpy` fails after install | the `python3` on PATH changed; reinstall from the Adapters section |
| Nothing happens on F5 inside the editor | the editor forwards F5/F9/F10/F11 to Caret's commands; check for a conflicting keybinding in Settings → Keybindings |
| Output missing | set `"console": "internalConsole"` in the configuration — Caret does not implement `runInTerminal` yet |

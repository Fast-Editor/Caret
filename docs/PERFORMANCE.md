# Performance & memory

Caret is built to be fast and light: a single Rust core (tokio, mimalloc, LTO "fat", `codegen-units = 1`,
`panic = abort`) and the OS WebView instead of a bundled Chromium. This page records how we measure and the
numbers from the release build on an Apple Silicon Mac (macOS 25.5). Re-run the commands to refresh them.

## How we measure

```bash
# release bundle (own target dir so dev builds are not blocked)
CARGO_TARGET_DIR=$PWD/src-tauri/target-release npm run build
APP=src-tauri/target-release/release/bundle/macos/Caret.app/Contents/MacOS/caret

# cold start → UI interactive, driven through the dev bridge
CARET_AUTOMATION=1 CARET_AUTOMATION_PORT=17421 $APP &        # then poll http://127.0.0.1:17421/health
node scripts/e2e.mjs eval 'return performance.getEntriesByType("paint")'   # first-contentful-paint

# memory: core process + the WebKit helper processes that belong to it
ps -o rss= -p $(pgrep -f Caret.app/Contents/MacOS/caret)
ps -axo pid=,rss=,comm= | grep -E 'WebKit\.(WebContent|GPU|Networking)'
```

Memory is reported as RSS. On macOS the WebKit content process is shared infrastructure (it exists for any
WKWebView app); we count it anyway because it is where Monaco, xterm and React live.

## Startup (release build, 29.8 MB binary, 34 MB .app)

| Milestone | Time |
|---|---|
| Rust core up (tracing `starting Caret` → automation bridge listening) | ~0.4 s |
| WebView navigation → DOMContentLoaded | 230–285 ms |
| First contentful paint | ~330 ms |
| Launch → full workbench interactive (incl. OS WebView process spawn, session restore) | 2.3–2.9 s |
| Quick open (fuzzy file search, warm) | 8 ms |
| Project-wide search for a symbol in the demo repo (ripgrep engine in-process) | 33 ms |
| Hybrid codebase search (BM25 + embeddings) | 82 ms |
| Tab completion round trip through a local proxy | ~0.5–1.0 s (model bound) |

## Opening a folder

`workspace_open` canonicalises the path, switches the settings scope, touches the recent list and emits
`caret://workspace/changed`. The Rust listeners run inline inside that emit, so each one only hands work to a
background task: the file watcher restarts on a blocking thread (`NoCache`: no file-ID tree walk), the quick-open
index rebuilds on its own thread, the codebase index reopens its stores and waits 1.2 s before the scan + embeddings
run so a rapid folder switch cancels it, language servers are stopped, MCP configs reload. The UI reloads in tiers
(`src/lib/workspaceReload.ts`): store resets and the explorer/editor first, git status and the chat thread list after
the tree has painted (150 ms), branches/worktrees/remotes, MCP, rules, index status and chat metadata once settled
(600 ms).

The quick-open index is two-phase (`core/fs.rs`). Tier 1 walks the tree honouring ignore files and is published the
moment it finishes; it is the only thing `fs_quick_open`, `fs_list_files` (@-mentions, the agent's file list) ever
wait for. Tier 2 walks again with ignore files off but the heavy-directory skip list on (`target`, `node_modules`,
`dist`, `build`, `out`, `Pods`, `DerivedData`, `vendor/bundle`, ...) and merges the ignore-only files afterwards from
the same thread. Each walk stops at an entry cap (200k / 50k), a wall-clock budget (`indexing.quickOpenBudgetMs` 8 s,
`indexing.quickOpenIgnoredBudgetMs` 1.5 s) or when the workspace changes under it (generation ticket); a cap or
budget stop publishes `partial: true` (`fs_index_status`) and Quick Open's footer says "showing the first N files".
The log shows the split: `workspace opened ... settings_ms recent_ms emit_ms total_ms`, `file watcher started ms=`,
`quick-open index built files= partial= lock_wait_ms= ms=`, `quick-open ignored tier merged ignored= partial= ms=`,
`index stores opened`, `index run complete`.

Before this work the command was a plain (main-thread) command and the watcher's default rename cache walked the
whole tree, node_modules and target included, before the command returned. Every IPC call queued behind it, so the
window froze for as long as the walk took. Measured on this machine (Apple Silicon, macOS 25.5, debug builds driven
through the automation bridge, wall clock from the `open()` call in the UI; "before" is commit 5b84014, "after" is
this branch's core with the same frontend; a `sample` of the frozen process showed 100% of the main thread in
`workspace_open -> emit -> watcher::start -> FileIdMap::add_root -> walkdir/stat`):

| Folder | `workspace_open` round trip | Explorer tree for the new root | Git status in the status bar | `file watcher started` |
|---|---|---|---|---|
| caret-demo (7 files) | 19-26 ms before, 37-46 ms after | ~100 ms before and after | ~240 ms | 2 ms after |
| claude-code (690 tracked files, 600 MB node_modules) | 7.1 s and 8.8 s before, 33-81 ms after | 7.4 s and 9.3 s before, 90-131 ms after | blocked before, ~250 ms after | 7.1 s before, 2 ms after |
| dormer (124,818 files, 32 GB, 703 tracked) | 3 min 42 s and 2 min 13 s before (window frozen), 20-30 ms after | frozen before, 52-117 ms after | frozen before, 205-245 ms after | blocked the open before, 4-7 ms after |

Quick Open on dormer: before, a `fs_quick_open` issued right after the open returned after 133 s (queued behind the
frozen main thread; the walk itself then took 72 ms). After, tier 1 publishes 695 files 22 ms after `workspace
opened` (not partial) and tier 2 merges 354 ignore-only files 16 ms later, so a result for the new root is available
about 45 ms after the open. `target/`, `node_modules/` and friends are on the heavy-directory skip list so neither
walk enters them. Also on dormer after the change: libgit2 status 55-77 ms, `git log` (50) 11-12 ms, no language
server is spawned by the open itself, the explorer lists only the root (25 entries, no persisted expansion), and no
main-thread stall above 61 ms was sampled. The index run on an already-indexed folder is 0.3-0.4 s; a first run is
bound by the embeddings backend (42 s for 141 files through Ollama on this machine) and runs entirely off the UI
thread.

Re-measure (dev app with the bridge up): `node scripts/e2e.mjs eval "<body>"` where the body is the snippet in
`docs/PERFORMANCE.md`'s git history for this section, or simply time
`await window.__caret.stores.workspace.getState().open(path)` with `performance.now()` and watch the tracing lines
above. Note that `setInterval` gaps of ~1 s in an unfocused window are WebKit timer throttling, not a blocked main
thread; use the `requestAnimationFrame` gap or the IPC completion times as the block signal.

## Memory (release build, demo workspace)

Two runs, sampled with `ps` RSS. WebKit's JavaScript heap is garbage-collected lazily, so the content
process swings by 2× between a cold load and a settled state — report ranges, not points.

| State | Rust core | WebKit content |
|---|---|---|
| Idle, workspace open, no editors (settled) | 105–122 MB | 130–160 MB |
| Just after session restore / first load (before GC) | 122 MB | up to ~340 MB |
| Python file open | 115–122 MB | 240–346 MB |
| TypeScript file open (language server running) | 122–125 MB | 270–353 MB |
| + integrated terminal (WebGL renderer) | 122–127 MB | 310–360 MB |
| After an Ask-mode chat completed (settled) | 105 MB | 131 MB |

WebKit GPU helper ≈ 36 MB, Networking helper ≈ 9 MB. Total process family: ~250 MB settled, up to ~520 MB
with everything open before GC. For comparison, during the same session Cursor 3.27 used ~393 MB across 14
processes idle with one workspace open, and VS Code ~312 MB across 12 processes.

## What keeps it light

- **No Node, no Chromium.** The only runtime is one Rust process plus the system WebView.
- **Token estimation without BPE tables** (`src-tauri/src/tokens.rs`): a calibrated heuristic replaces a
  ~40 MB tokenizer load and ~4 MB of binary.
- **tantivy writer is lazy**: the 32 MiB indexing arena exists only while files are being indexed and is
  dropped after each commit; the index itself is memory-mapped.
- **Bounded buffers everywhere**: PTY scrollback ring (256 KiB/terminal), LRU of loaded chat sessions (8),
  Monaco models (≤ 40, background models disposed), tool output caps, search result caps.
- **Monaco code-splits itself**: language grammars and the TypeScript/CSS/HTML/JSON workers load on demand
  (the Vite config deliberately does not merge `monaco-editor` into one chunk).
- **When a real language server runs, the built-in TypeScript worker is switched off** (no duplicate
  diagnostics, no eager model sync).
- **Streams over IPC channels** (`tauri::ipc::Channel`) with PTY output coalesced in 16 ms bursts and chat
  deltas batched per animation frame.

## Known remaining costs / next optimisations

- Monaco's eager chunk is still ~2.7 MB of JS (editor core); lazy-loading the editor module until the
  first file opens would cut idle WebKit memory further.
- The TypeScript worker (6.9 MB JS) is the single largest in-page cost when editing TS without an LSP
  server installed; installing `typescript-language-server` moves that work out of the WebView process.
- xterm's WebGL renderer trades GPU-process memory for scroll speed; a `terminal.renderer` setting can
  select the DOM renderer on constrained machines.

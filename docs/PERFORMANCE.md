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

<div align="center">

# Caret

**A Cursor-class AI code editor with a Rust core.** Tauri 2 + Rust on the backend, React 19 + Monaco in the system WebView on the front. No bundled Chromium, no Node at run time.

</div>

## Why Caret

Cursor proved what an AI-native editor should feel like: Tab completions that predict your next edit, ⌘K inline edits, an agent that reads, searches, edits and runs your code with checkpoints and review, rules, memories, MCP, hooks, background agents in worktrees, Bugbot-style review. Caret rebuilds that feature set as an **independent, open-source implementation** engineered for memory efficiency:

| | Electron editors | Caret |
|---|---|---|
| UI runtime | bundled Chromium (~150–300 MB baseline) | OS WebView (WKWebView / WebView2 / WebKitGTK) |
| Backend | Node.js extension hosts | one Rust process (tokio), mimalloc |
| Search | ripgrep subprocess | `grep` + `ignore` crates in-process (same engine as ripgrep) |
| Index | separate indexer | tantivy (memory-mapped) + optional embeddings |
| Terminal | node-pty | portable-pty with bounded ring buffers |
| Git | git CLI / libgit2 via Node | libgit2 in-process (reads) + git CLI (network) |

Measured on the release build (see [docs/PERFORMANCE.md](docs/PERFORMANCE.md)): Rust core ~110 MB RSS idle with a workspace open, first paint ~330 ms, quick open 8 ms, project search 33 ms, hybrid codebase search 82 ms. The whole process family (core + WebKit helpers) with a TypeScript file and a terminal open is ~490 MB, versus ~393 MB for Cursor idle on the same machine — the remaining gap is the WebKit content process, and the optimisation plan is in the performance doc.

## Features

- **Tab** — multi-line ghost-text completions from any configured model, partial accept (⌘→), **next-edit prediction** ("Tab to jump"), context from recent edits, open files and diagnostics.
- **⌘K** — inline edit / generate with streaming inline diff and accept/reject; quick questions; **terminal ⌘K** turns natural language into shell commands.
- **Agent pane** — Agent / Ask / Plan / Debug + custom modes; streaming transcript; tool calls (read, list, grep, fuzzy file search, codebase search, edit/write/delete, shell with approvals, web search/fetch, todo list, memory, rules, skills, subagents, MCP tools, browser automation); **checkpoints**, **review changes** with per-file accept/reject, queued messages, context-window summarization, usage & cost.
- **@-mentions** — Files, Folders, Code/selection, Codebase, Git (diff, staged, branch diff, commits), Web, Docs, Terminals, Lint errors, Recent changes, Past chats, Rules, Links, Notes, MCP resources, Skills; images (vision).
- **Rules & knowledge** — `.caret/rules/*.mdc` (Always / Auto Attached / Agent Requested / Manual), legacy `.cursorrules`, `AGENTS.md`, `CLAUDE.md`, user rules; **memories**; **hooks** (`beforeSubmitPrompt`, `beforeShellExecution`, `beforeMCPExecution`, `beforeReadFile`, `afterFileEdit`, `afterAgentResponse`, `stop`…); slash **commands**; **skills** (`SKILL.md`); **subagents**; **plans**.
- **Codebase indexing** — ignore-aware walker, language-aware chunking, BM25 (tantivy) + optional embeddings (Ollama / any OpenAI-compatible endpoint), hybrid retrieval, incremental updates, **@Docs** crawler.
- **MCP** — stdio / streamable-HTTP / SSE servers from `.caret/mcp.json` (and Cursor's `.cursor/mcp.json`), tools, resources, prompts, per-tool enable, logs.
- **Background agents** — parallel sessions, optional **git worktree** isolation, chat-centric **Agents layout**.
- **Bugbot-style review** — review working tree / staged / branch / commit for real defects with jump-to-line and "Fix in chat".
- **Editor** — Monaco (VS Code's editor core), split groups, diff editor, breadcrumbs, outline, Problems, lean **LSP client** (TypeScript, rust-analyzer, Pyright, gopls, clangd, JSON/CSS/HTML/YAML/Bash/Lua/Java/Ruby/Zig/TOML), git gutter + blame, markdown & image preview, 4 themes.
- **Workbench** — explorer with git decorations, project-wide search/replace, source control (commit UI, AI commit messages, branches, history, stashes, worktrees), integrated terminal (xterm.js, splits), command palette, quick open, keybindings editor, full settings UI, status bar with live memory badge, privacy mode.
- **Providers** — Anthropic (Messages API with adaptive thinking, effort, prompt caching, refusal fallbacks), OpenAI, OpenAI-compatible (Ollama, OpenRouter, LM Studio, vLLM, proxies), Azure OpenAI, Gemini (OpenAI-compatible endpoint). Bring your own keys; nothing leaves your machine except model calls.
- **External agents (ACP)** — pick **Claude Code, Codex, Gemini CLI, OpenCode, Pi, GLM** or any Agent Client Protocol agent per chat from the composer. Pick the agent's model, effort and permission mode from the composer (ACP session config options). The agent runs locally with its own login; its streaming text, thoughts, tool calls, permission prompts, diffs (checkpointed, in the review bar) and terminals all flow through Caret's UI. Install adapters from Settings → Agents. See `docs/ACP.md`.
- **Command safety & sandbox** — every agent shell command is parsed (POSIX shell AST incl. nested `sh -c`/`eval`/`$( )`) and classified below the model (safe / caution / destructive / blocked with labels); destructive commands always ask, blocked ones never run; approved commands run under **macOS Seatbelt / Linux bubblewrap** (writes limited to workspace + temp + caches, credential folders unreadable, optional no-network); **MCP tool-description pinning** re-approves tools that silently change. See `docs/SANDBOX.md`.
- **Cursor compatibility** — reads `.cursor/rules`, `.cursor/mcp.json`, `.cursor/hooks.json`, `.cursor/commands`, `.cursor/skills`, `.cursor/agents`, `.cursorignore`, `.cursorindexingignore` so existing projects work unchanged.

## Status (2026-10-25)

Verified end to end on macOS against a local OpenAI-compatible proxy and Ollama embeddings:

- Workbench: explorer with git decorations, tabs/split groups, Monaco editing, breadcrumbs, terminal (login shell, OSC title/cwd), project search (streamed), quick open, command palette, settings UI, status bar, Agents layout.
- AI: Ask and Agent sessions with streaming, tool calls (grep/read/edit/shell), approval cards with allow/deny variants, checkpoints and pending-change review, usage + cache accounting, Tab completions (cleaned FIM), ⌘K inline edit stream, AI commit messages, Bugbot-style review.
- Intelligence: LSP client with a real `typescript-language-server` (diagnostics, hand-over from the built-in TS worker), codebase index (tantivy + Ollama `nomic-embed-text`), MCP stdio server (`@modelcontextprotocol/server-everything`: 16 tools, tool call round trip).
- Tests: 208 Rust unit tests (incl. real Seatbelt enforcement and shell-session tests), 112 frontend tests, strict TypeScript.

Not implemented (needs cloud services or OS automation): voice dictation, computer use, cloud/remote agents, design mode, extension marketplace. SSE-only MCP servers and OAuth-protected MCP servers report a clear error. Browser automation exposes DOM snapshots, not pixel screenshots.

## Configure models

Open **Settings → Models** (⌘,): paste an API key for Anthropic / OpenAI / OpenRouter / Gemini, or point the
"OpenAI-compatible (custom)" provider at any `/v1` endpoint (Ollama, LM Studio, vLLM, a proxy). "Import API
keys from environment" picks up `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`, `GEMINI_API_KEY`.
Pick a default model, a fast model for Tab, and (optionally) an embeddings provider under **Indexing & Docs**.

## Where the code lives

The project is a single repository: locally at **`~/caret`**, published at **https://github.com/Fast-Editor/Caret**
(branch `main`; the earlier Electron prototype stays in that repo's git history). Two halves talk over Tauri IPC:

```
~/caret
├── src/                      React 19 + TypeScript frontend (Vite 8, Tailwind 4, Monaco 0.57, xterm 6)
│   ├── main.tsx, App.tsx     entry + root layout (ErrorBoundary, panes, dialogs)
│   ├── types.ts              THE contract: every struct the Rust side serialises (camelCase)
│   ├── styles.css            theme tokens (Caret Dark = neutral Cursor-like palette) + globals
│   ├── lib/api.ts            typed `invoke` wrappers — the only place that names Rust commands
│   ├── lib/commands.ts       UI command bus (`runCommand('terminal.new')` …; registry in docs/UI_COMMANDS.md)
│   ├── lib/bus.ts            pub/sub topics between UI modules
│   ├── lib/tauri.ts          event bridge for `caret://…` events from Rust
│   ├── lib/settingsSchema.ts settings defaults + schema that renders the Settings UI
│   ├── lib/monaco/           Monaco setup, themes, workers, git gutter, inline completions (Tab)
│   ├── lib/lsp/              language-server client glue
│   ├── lib/cliOpen.ts        handles `caret <path>` opens coming from the CLI
│   ├── store/                zustand stores (chat, editor, workspace, terminal, review, settings, git, …)
│   └── components/           UI by area: layout, explorer, editor, chat (composer/, messages/), terminal,
│                             review, scm, search, palette, settings, statusbar, agents, dialogs
├── src-tauri/                Rust core (Tauri 2, tokio, mimalloc)
│   ├── tauri.conf.json       app id `dev.caret.editor`, bundle targets (app, dmg), dev/build commands
│   ├── Info.plist            merged into the .app: folder/document types + `caret://` URL scheme
│   ├── cli/caret.sh          template of the `caret` command-line launcher
│   ├── tests/fixtures/       fake ACP agent used by integration tests
│   └── src/
│       ├── lib.rs            module wiring, managed state, command registration (`// REGISTER:` markers)
│       ├── core/             fs, workspace, watcher, search (grep crates), git (git2 + CLI), pty + shell
│       │                     integration (OSC 133/7501/7770 blocks), lsp, settings, sessions, cli
│       ├── ai/agent/         Caret's own agent: runner loop, tools, policy/approvals, modes, rules,
│       │                     memories, checkpoints, plans, subagents, hooks, review comments
│       ├── ai/providers/     Anthropic Messages API, OpenAI-compatible (Ollama/OpenRouter/vLLM/proxies), SSE
│       ├── acp/              Agent Client Protocol client: run Claude Code, Codex, Gemini CLI, OpenCode…
│       │                     inside the same chat (protocol, process, client, bridge, registry)
│       ├── verify/           Verified Diff engine: diagnostics, lint, tests, mutation, dependency
│       │                     grounding, security rules, project check → risk tier + proof panel
│       ├── sandbox/          shell AST classifier, macOS Seatbelt / Linux bwrap wrappers, MCP tool pins
│       ├── index/            tantivy + embeddings hybrid code index, docs indexing
│       ├── mcp/              Model Context Protocol servers (stdio + HTTP)
│       ├── browser/          DOM-snapshot browser tool
│       └── dev/automation.rs localhost bridge used by scripts/e2e.mjs for scripted UI checks
├── scripts/                  e2e.mjs (bridge client), wave1-check.mjs (smoke checks)
├── docs/                     ARCHITECTURE, UI_COMMANDS, VERIFIED_DIFF, SANDBOX, TERMINAL, ACP, CLI,
│                             PERFORMANCE, INNOVATION_PLAN
├── dist/                     built frontend (output of `npm run build:web`; generated)
└── node_modules/             npm dependencies (generated)
```

Size today: ~21k lines of Rust across 112 files, ~16k lines of TypeScript across 205 files.

Conventions that keep the two halves in sync:

- Add a Rust command → register it on the matching `// REGISTER:commands:<domain>` line in `src-tauri/src/lib.rs`,
  add a wrapper in `src/lib/api.ts`, and mirror any new struct in `src/types.ts` (serde `camelCase`).
- Streams use `tauri::ipc::Channel`; push events use `caret://…` names and go through `src/lib/tauri.ts`.
- UI modules never import each other's internals; they call `runCommand(id)` or publish on the bus.

## Build and run — step by step

### 1. Prerequisites

| Tool | Version used for the current builds | Install |
| --- | --- | --- |
| macOS | Apple Silicon, macOS 15/26 | Linux/Windows build too (Tauri 2) but are untested here |
| Xcode Command Line Tools | full Xcode 26 selected | `xcode-select --install` (CLT is enough) |
| Rust (stable) | rustc / cargo 1.99 (min 1.80) | `curl https://sh.rustup.rs -sSf \| sh` |
| Node.js | 26.3 (20+ works) | nodejs.org, nvm, or Homebrew |
| npm | 11 | ships with Node |

If `cargo` is not on your PATH after installing rustup (common inside GUI-launched shells), export it first:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
# or, when rustup placed the toolchain under ~/.rustup:
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
```

Optional, only for features that call out to other tools: `git` (bundled with Xcode CLT), a language server such as
`typescript-language-server` in the project you open, `node` for the ACP adapters, Ollama for local embeddings.

### 2. Install dependencies

```bash
cd ~/caret
npm install          # installs React/Monaco/xterm/Vite and the Tauri CLI (`@tauri-apps/cli`)
```

Rust crates are fetched automatically by cargo on the first build (several hundred MB in `~/.cargo`).

### 3. Run in development

```bash
cd ~/caret
npm run dev          # = `tauri dev`: starts Vite on http://localhost:1420, compiles the Rust core in debug
                     #   mode, opens the window, hot-reloads the frontend on save, restarts Rust on change
```

First compile takes a few minutes; later runs are incremental. Useful switches:

```bash
CARET_DEVTOOLS=1  npm run dev     # open the WebView inspector
CARET_AUTOMATION=1 npm run dev    # start the localhost automation bridge on http://127.0.0.1:17420
node scripts/e2e.mjs health       # bridge: health | eval "<js>" | shot <name> | cmd <command-id> [json-args]
node scripts/wave1-check.mjs      # smoke checks (sandbox, terminal blocks, agents, verify) against the bridge
```

The bridge evaluates JavaScript inside the running app (`window.__caret` exposes `runCommand`, `api` and the
stores); keep it off for normal use. `CARET_AUTOMATION_PORT` changes the port, `CARET_AUTOMATION_URL` points
`scripts/e2e.mjs` at another instance.

### 4. Check and test

```bash
npm run typecheck                               # tsc --noEmit
npm test                                        # vitest (frontend unit tests; 112 today)
cd src-tauri && cargo test --lib                # Rust unit + integration tests (216 today; uses the fake ACP agent)
cd src-tauri && cargo test --lib verify         # one module
CARET_LIVE_TEST=1 cargo test --lib acp::tests::live -- --nocapture   # opt-in: real Claude Code round trip
```

### 5. Build a release (.app and .dmg)

```bash
cd ~/caret
export PATH="$HOME/.cargo/bin:$PATH"                 # see step 1 if cargo is not found
CARGO_TARGET_DIR=$PWD/src-tauri/target-release npm run build
```

What happens: `npm run build` runs `tauri build`, which first executes `npm run build:web` (typecheck + Vite
production bundle into `dist/`), then `cargo build --release` for the Rust core with `dist/` embedded, then the
bundler. Output:

```
src-tauri/target-release/release/bundle/macos/Caret.app          ← the app (ready to run or copy to /Applications)
src-tauri/target-release/release/bundle/dmg/Caret_0.1.0_aarch64.dmg
```

`CARGO_TARGET_DIR` is optional; it keeps the release artefacts apart from the debug target dir used by `npm run
dev` so the two do not invalidate each other's caches. A clean release build takes roughly 6–10 minutes on an
M-series Mac; incremental rebuilds after frontend-only changes take 2–3 minutes.

Known wrinkle: the DMG step (`bundle_dmg.sh`, which calls `hdiutil`) occasionally fails with "Resource busy" or
hangs. The `.app` is already complete at that point. If it hangs, `pkill -f bundle_dmg.sh` lets the build finish;
if it fails, re-run the build or ignore the DMG and use the `.app`.

### 6. Install and launch

```bash
open src-tauri/target-release/release/bundle/macos/Caret.app      # run in place, or:
cp -R src-tauri/target-release/release/bundle/macos/Caret.app /Applications/
```

Launching the bundle once (or `lsregister -f <path to Caret.app>`) registers the `caret://` URL scheme and the
folder/document types with macOS LaunchServices. If an older Caret instance is still running, quit it first —
macOS delivers "open" events to whichever instance of that bundle is already running.

### 7. Command-line launcher (`caret .`)

In the app, open the command palette (⌘⇧P) and run **Shell Command: Install 'caret' command in PATH**
(also under Settings → General). The launcher script is written to `~/.caret/bin/caret` and linked from the first
user-writable directory already on your PATH (`/usr/local/bin` when writable, else `~/.local/bin`,
`/opt/homebrew/bin`, `~/bin`); only if none exists does macOS ask for an administrator password. Then:

```bash
caret .                    # open the current folder as the workspace
caret src/app.ts:42:7      # open a file at line 42, column 7 (also `-g file:line`)
caret -v / caret -h        # version / usage
```

Details and the transport (`open -b dev.caret.editor`, `caret://open?path=…`) are in [docs/CLI.md](docs/CLI.md).

### 8. Configuration and data locations

- User: `~/.caret/` — `settings.json`, `secrets.json` (API keys), `keybindings.json`, `mcp.json`, `rules/`,
  `memories/`, `sessions/`, `checkpoints/`, `index/`, `bin/caret`, `acp/` (installed agent adapters).
- Project: `<repo>/.caret/` (rules, project settings), plus `.cursor/rules` and `AGENTS.md` are read if present.
- Settings → Models for providers and keys; Settings → Agents for external ACP agents.

### 9. Troubleshooting

| Symptom | Fix |
| --- | --- |
| `cargo: command not found` | export the rustup bin dir (step 1) |
| Vite build fails mentioning `esbuild` | `npm install` again — `esbuild` is a required dev dependency alongside Vite 8/rolldown |
| Monaco workers 404 / editor blank in dev | the worker imports must use the package export paths, e.g. `monaco-editor/editor/editor.worker.js?worker` and `monaco-editor/language/json/json.worker.js?worker` (see `src/lib/monaco/setup.ts`); the `esm/vs/...` deep paths are not exported by Monaco 0.57 |
| `npm run dev` restarts in a loop while a release build runs | don't run `tauri dev` and `tauri build` at the same time in one checkout |
| DMG step hangs or "Resource busy" | see step 5 — the `.app` is already built |
| `caret .` opens nothing | launch the new bundle once so LaunchServices knows it; quit older instances |
| External agent shows "Not installed" | Settings → Agents → Install (npm into `~/.caret/acp`) or install the tool yourself; Caret also searches Homebrew, nvm, volta, bun paths |

## Architecture

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the module map, IPC conventions and memory rules, [docs/UI_COMMANDS.md](docs/UI_COMMANDS.md) for the UI command registry. Feature docs: [VERIFIED_DIFF](docs/VERIFIED_DIFF.md), [SANDBOX](docs/SANDBOX.md), [TERMINAL](docs/TERMINAL.md), [ACP](docs/ACP.md), [CLI](docs/CLI.md), [PERFORMANCE](docs/PERFORMANCE.md).

## License

MIT. Caret is an independent project and is not affiliated with Anysphere/Cursor. Monaco Editor is © Microsoft (MIT).

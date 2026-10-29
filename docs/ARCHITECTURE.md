# Caret architecture

Caret is a Cursor-class AI code editor with a **Rust core** (Tauri 2) and a **React + Monaco**
frontend rendered by the OS WebView. No bundled Chromium, no Node runtime at run time.

```
┌─────────────────────────── WebView (React 19 + Monaco + xterm.js) ───────────────────────────┐
│  Layout: TitleBar · ActivityBar · Sidebar(Explorer/Search/SCM/Agents) · EditorArea · Panel   │
│  Chat/Agent UI · Command palette · Settings UI · Status bar · Lean LSP client → Monaco        │
└──────────────▲────────────── invoke() / Channel<T> / listen() ───────────────▲───────────────┘
               │                                                               │
┌──────────────┴────────────────────── Rust core (src-tauri) ───────────────────┴───────────────┐
│ core/   fs · search(grep crates) · git(git2 + git cli) · pty(portable-pty) · lsp relay ·      │
│         watcher(notify) · settings · sessions · workspace · sysmon                            │
│ ai/     providers(anthropic raw HTTP SSE, openai-compatible) · llm abstraction ·              │
│         agent/{runner,tools,context,rules,memories,hooks,skills,commands,subagents,checkpoints}│
│         tab(FIM + next-edit) · inline_edit(Cmd+K) · commit · review(Bugbot) · apply           │
│ index/  walker · chunker · tantivy BM25 · embeddings · hybrid search · docs crawler           │
│ mcp/    MCP client manager (stdio / streamable-http / sse)                                    │
│ browser/ agent-driven browser windows (navigate, DOM snapshot, click, type, eval)             │
└───────────────────────────────────────────────────────────────────────────────────────────────┘
```

## Conventions (binding for every module)

* **Commands** are `snake_case` `domain_action` Tauri commands. JS calls them through `src/lib/api.ts`
  with camelCase argument names (Tauri maps `filePath` → `file_path`). Every command returns
  `Result<T, CaretError>`; `CaretError` serializes as `{ code: string, message: string }`.
* **Streams** (LLM deltas, PTY output, search hits, index progress) use `tauri::ipc::Channel<T>`
  passed from JS (`new Channel<T>()`), never high-frequency events.
* **Events** (`listen`) are for low-frequency broadcasts: `caret://fs/changed`, `caret://git/changed`,
  `caret://lsp/message/{serverId}`, `caret://settings/changed`, `caret://agent/event`, `caret://index/status`,
  `caret://notify`.
* **Shared types** live in `src/types.ts` (TS) and mirror serde structs 1:1 (`#[serde(rename_all = "camelCase")]`).
  Rust is the source of truth; TS must match it.
* **State**: each Rust domain owns a state struct managed with `app.manage(...)`; commands take
  `State<'_, T>`. Registration lines live in `lib.rs` between `// REGISTER:<domain>` markers.
* **Memory rules**: stream large files, never buffer whole directory trees eagerly (lazy tree),
  bounded PTY scrollback (ring buffer), `lru` caches with hard caps, tantivy mmap index, dispose Monaco
  models on close, virtualize long lists, no polling loops (use notify/watchers).
* **Compatibility**: Caret reads its own `.caret/` project dir **and** Cursor's `.cursor/`
  (`rules/*.mdc`, `mcp.json`, `hooks.json`, `commands/`, `skills/`, `agents/`), plus `.cursorrules`,
  `.cursorignore`, `.cursorindexingignore`, `AGENTS.md`, `CLAUDE.md`.
* Global config dir: `~/.caret/` (`settings.json`, `secrets.json` (0600), `keybindings.json`,
  `mcp.json`, `rules/`, `memories/`, `sessions/`, `checkpoints/`, `index/`, `docs/`, `logs/`).

## Feature map (Cursor feature → Caret module)

| Cursor feature | Caret |
|---|---|
| Tab (autocomplete, multi-line, partial accept, next-edit jump) | `ai/tab.rs` + `lib/monaco/inlineCompletions.ts`, `nextEdit.ts` |
| Cmd+K inline edit / quick question / terminal Cmd+K | `ai/inline_edit.rs` + `lib/monaco/inlineEdit.ts`, terminal prompt bar |
| Agent / Ask / Plan / Debug / custom modes | `ai/agent/runner.rs` modes + `components/chat` |
| Agent tools (read/edit/search/terminal/web/todo/memory/MCP/browser/subagent) | `ai/agent/tools/*` |
| @-mentions (Files, Folders, Code, Codebase, Git, Web, Docs, Terminal, Lint, Recent changes, Past chats, Rules, Link) | `ai/agent/context.rs` + `components/chat/MentionPicker` |
| Checkpoints / Review changes / accept-reject per file | `ai/agent/checkpoints.rs` + `components/chat/ReviewBar` |
| Rules (Always / Auto Attached / Agent Requested / Manual), AGENTS.md | `ai/agent/rules.rs` |
| Memories | `ai/agent/memories.rs` |
| Hooks (beforeSubmitPrompt, beforeShellExecution, beforeMCPExecution, beforeReadFile, afterFileEdit, afterAgentResponse, stop) | `ai/agent/hooks.rs` |
| Commands (`/slash`), Skills (SKILL.md), Subagents | `ai/agent/{commands,skills,subagents}.rs` |
| Codebase indexing (ignore files, incremental, embeddings) + @Docs | `index/*` |
| MCP (stdio, streamable-http, sse; tools/resources/prompts) | `mcp/*` |
| External agents via the Agent Client Protocol (Claude Code, Codex, Gemini CLI, Pi, GLM, custom) driving the same chat/review/approval UI (Caret-only; Cursor has no ACP client) | `acp/*` (protocol, process, client, bridge, registry, commands) → `chat_commands.rs` routing, `components/chat/composer/AgentPicker.tsx`, `settings/pages/Agents.tsx` — docs/ACP.md |
| Command safety: shell AST + deterministic risk classifier, Seatbelt/bwrap sandbox, MCP tool pinning (Caret-only; Cursor has allow/deny lists) | `sandbox/*` → `ai/agent/policy.rs`, `tools/shell_tools.rs`, `components/chat/messages/ToolCallCard.tsx`, `settings/pages/SandboxSection.tsx` — docs/SANDBOX.md |
| Background / parallel agents, worktrees, Agents layout | `AgentManager` in runner + `components/agents` |
| Browser automation | `browser/*` |
| Bugbot-style review, commit message generation, blame | `ai/review.rs`, `ai/commit.rs`, `core/git.rs` |
| Auto-run with allow/deny lists, file-delete protection, sandboxed shell | `ai/agent/policy.rs` |
| Privacy mode, usage/cost tracking, model picker, thinking/effort | settings + runner usage accounting |
| Integrated terminal, git, search, LSP intelligence, themes, keybindings, settings UI | `core/*` + shell UI |

## UI motion rules

Motion is a shared system, not per-component improvisation (`src/styles.css`, `src/lib/motion.ts`):

- Three durations (`--dur-fast` 120 ms, `--dur-base` 180 ms, `--dur-slow` 240 ms) and one ease-out curve (`--ease`).
  Settings → Appearance → *Motion* sets `<html data-motion="off|subtle|expressive">`, which scales or zeroes them;
  `Reduce motion` and the OS preference force `off`. Anything driven by the tokens therefore degrades to instant.
- Animate `transform`, `opacity` and colours only. Pane toggles may transition width/height, never while dragging.
  Nothing runs longer than `--dur-slow`; nothing blocks input.
- Entry: `animate-fade-up` / `animate-pop` / `animate-slide-in-*` once on mount (key by id — never re-animate on
  store updates). Exit: keep the node mounted with `usePresence(open)` and toggle `.is-leaving`.
- Depth comes from tone, not lines: sidebar/panels low, editor high, floating surfaces (`surface-float`) lifted with a
  soft shadow. Hairline borders only where equal tones meet. Radii: 8 px cards/inputs/popovers, 10 px dialogs, pills round.

## Editor completion rules

- **Document sync before requests.** `LspManager` debounces `didChange` (150 ms) for diagnostics traffic, but every
  provider resolves its client through `flushFor(model)` first, so completion/hover/signature requests always run
  against the current text. Quick suggestions fire ~10 ms after a keystroke; never add a request path that skips the
  flush.
- **Ordered writes.** `LspClient.send` chains Tauri invokes; two invokes are not ordered otherwise, and a `didChange`
  overtaken by the request that follows it yields completions computed at the wrong position.
- **Tab beside the suggest widget.** Monaco only shows an inline completion while the widget is open if it *augments*
  the highlighted item (`insertText` starts with the item's text over the same range). `inlineCompletions.ts` builds
  the request as if the item were accepted and returns `item.text + completion` on the item's range. Monaco's own
  `suggest.preview` stays off so the ghost-text slot belongs to Tab; `showOnSuggestConflict: 'always'` keeps an
  already visible ghost text when the widget pops.
- **In-flight sharing.** Requests are keyed by context; a second identical call adopts the running one, token
  cancellation releases it with a 60 ms grace, and only non-empty answers are cached. The Rust side mirrors this: a
  waiter on a cancelled or failed request runs its own instead of inheriting an empty result.
- **Automation.** `window.__caret` exposes `monaco`, `lsp` (the manager) and `tab` (last round-trips) for
  `scripts/e2e.mjs eval`; type with `editor.trigger('keyboard', 'type', …)` or quick suggestions will not fire.

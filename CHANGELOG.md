# Changelog

All notable changes to Caret. The version here must match `package.json`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` (`npm run release:bump <version>` keeps them in sync).

## Unreleased

- Command palette: opening with ⌘⇧P listed nothing and nothing typed after the `>` prefix matched, because the prefix
  was scored as part of the query. The prefix is now stripped before matching and an empty query lists every command,
  recently used first.
- Menus, popovers, dialogs and toasts had no background or shadow (the floating-surface tokens were never defined per
  theme), so context menus floated transparently over code. Each theme now defines them.
- Tab bar: closed editors no longer linger as unclickable ghost tabs (the exit-animation timer was cancelled by the
  very state change that closing a tab causes, so ghosts piled up at the end of the strip and new tabs opened out of
  view).
- Quick Open, @-mentions and the file list now see files that only an ignore file hides (a repo's own `tmp/`, scratch
  folders): they form a second tier of the file index, ranked below tracked files, marked "ignored" in the list, and
  heavy directories such as node_modules, dist, build or target stay excluded. Semantic search and the AI context index
  still honour ignore files.
- Editor completions: debounced document edits are flushed to the language server before any request, and
  client→server writes are serialised, so completion lists are filtered against what you actually typed instead of
  a stale document (previously visible as an unfiltered Java list and insertions at the wrong column).
- Tab: ghost text now extends the highlighted suggest-widget item (Copilot-style augmentation) instead of
  disappearing whenever the LSP popup is open; identical in-flight requests are shared instead of cancelled, a
  cancelled request no longer poisons the next one with an empty answer, and empty answers are never cached.
  Monaco's own suggestion preview is off so the ghost text slot belongs to Tab. Request failures show as "Tab !" in
  the status bar and title bar.
- Tab: falls back to another configured provider for 45 s when the Tab model's provider errors.
- Language servers are resolved and launched with the same augmented PATH as ACP agents (Finder-launched apps get a
  minimal PATH); the Java hint explains the manual Eclipse JDT LS install (needs JDK 25).
- Fluid UI: motion tokens (`--dur-*`, `--ease`, `ui.motion` off/subtle/expressive, OS reduce-motion respected),
  presence animations for popovers, dialogs, toasts and menus, FLIP-animated tabs and sidebar panes, smooth composer
  growth, streaming caret, tool cards that fold with animated height, overlay scrollbars and 8/10/14 px radii.

## 0.1.0 — 2026-10-28

First public release.

- Cursor-class editor: Monaco editor, Tab completions with next-edit prediction, ⌘K inline edits, agent chat with
  approvals, checkpoints, review bar, rules, memories, skills, plans, hooks and subagents.
- Verified Diff: diagnostics, project check, dependency grounding, security rules, tests, mutation score and lint after
  every agent turn, with a proof panel and one-click fixes.
- Command safety: shell AST classification, macOS Seatbelt / Linux bubblewrap sandboxing, MCP tool pins.
- Semantic terminal (OSC 133 / 7501 blocks), git, LSP, hybrid code index, MCP, browser tool.
- External agents over the Agent Client Protocol (Claude Code, Codex, Gemini CLI, OpenCode, …) with per-agent
  model, effort and mode pickers.
- Auto routing: a managed Lynkr sidecar scores every request and routes it to one of your configured models;
  routing decisions are shown on each reply.
- `caret` command-line launcher (`caret .`, `caret file:line:col`).
- Neutral dark theme, hand cursors, in-app updater, multi-platform release pipeline.

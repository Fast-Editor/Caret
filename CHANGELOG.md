# Changelog

All notable changes to Caret. The version here must match `package.json`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` (`npm run release:bump <version>` keeps them in sync).

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

# Changelog

All notable changes to Caret. The version here must match `package.json`, `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` (`npm run release:bump <version>` keeps them in sync).

## Unreleased

- Usage page: the ledger is backfilled once from your saved chat sessions (and on demand with **Import past sessions**), so
  history from before this release is counted; when every request is priced at $0 the page says so and the chart shows
  tokens per day instead of an empty box; external agent histories are scanned newest first with a 1 GB cap (was 200 MB,
  which cut off part of a large Claude Code history). New command `usage_backfill`.
- Usage page: the Settings User / Workspace toggle now filters the ledger to the open folder in Workspace scope (external
  agent histories stay in User scope); the JSON button creates `.caret/settings.json` when a workspace has none yet
  instead of failing to open it.
- External agents: **Cursor** (official `cursor-agent acp`, your Cursor plan) and **Grok Build** (the Grok CLI's native
  `grok agent stdio` ACP server) join Claude Code, Codex, Gemini CLI, OpenCode, Pi and GLM. Settings → Agents now shows each agent's
  sign-in state from the vendor CLI, a **Sign in** button that runs the vendor login in a Caret terminal (ChatGPT for
  Codex, Cursor, Claude, Grok, OpenCode), **Check**, and **Add account** for a second Codex / Claude / Grok account with
  its own config home (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `GROK_HOME`).
- Commands that external agents run through Caret terminals are classified and sandboxed like the built-in agent's:
  blocked commands never run, destructive ones ask, approved ones run under Seatbelt or bubblewrap
  (`acp.terminalSafety`).
- UI zoom now uses the webview's native page zoom instead of CSS `zoom` on the root. Under CSS zoom WebKit reports
  mouse positions in a different space than element rectangles, so terminal hover, selection and link clicks landed a
  few rows away from the pointer at zoom levels other than 0.
- Terminal: file paths in output are clickable (validated against cwd and the workspace root, `:line:col` and
  `(line,col)` understood) and open in the editor; ghost-text command suggestions from history with the Tab model as
  fallback (→ / End accepts; `terminal.suggestions`); a **Fix last command** chip and *Fix Command (AI)* menu entry that
  propose a corrected command for a failed block into the ⌘K bar; persisted cross-session command history with
  lexical and natural-language search (⌘⌥H); per-theme ANSI palette editor with Windows Terminal / VS Code / iTerm2
  import and export; terminal background modes (panel, editor, app, transparent); dimmed inactive split panes;
  per-terminal colour from the list's right-click menu.
- Terminal list: clicking another terminal opened its actions menu instead of switching to it (the row was a
  click-triggered dropdown). Left click now selects; the Rename / Split / Add Output / Kill menu is on right click.
- Responsiveness: sixty read-only backend commands (settings, registries, status probes, file stat, session and
  knowledge lists) ran synchronously on the main thread and froze the whole window for the duration of their disk
  reads and process spawns; they now run on worker threads. The debugpy probe, which spawned a Python interpreter on
  every Settings or Debug render, is cached for a minute.
- Tab context menu: actions never ran because the outside-press listener closed the menu before the item's click
  fired; presses inside the menu are now ignored by that listener and only Escape closes it from the keyboard.
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
- Threads: snooze (1 hour, tonight, tomorrow, next week) and settle threads from the row menu; snoozed threads wake automatically, settled and archived threads get a 5 s Undo toast. New `chat.autoSettleDays` setting marks idle threads as settled.
- Threads: history popover and Agents sidebar now show foldable sections (Working, Pinned, Today, Yesterday, This week, Earlier, Snoozed, Settled, Archived) with persisted fold state, plus a "by project" view across every workspace; opening another project's thread offers to open that folder.
- Threads: "Regenerate title" (row menu and `chat.regenerateTitle`), message search across session files from the history search box (`chat.searchMessages`, opens the thread and scrolls to the hit), and `chat.nextThread` / `chat.prevThread` (`ctrl+alt+]` / `ctrl+alt+[`).
- Backend: `chat_snooze_session`, `chat_settle_session`, `chat_list_sessions_all`, `chat_regenerate_title`, `chat_search_messages` commands; pin/archive/snooze/settle emit the new `session-updated` event; `Session` / `SessionSummary` gain `snoozedUntil`, `settled`, `archivedAt`, `pinnedAt`. See docs/THREADS.md.
- Steer a running agent: `Alt+Enter` (or the new Queue/Steer split control next to the stop button) injects your
  message before the agent's next step inside the current turn instead of waiting for it to finish; if the model had
  just answered, the turn continues with your message. The default for `Enter` while running is switchable
  (Queue or Steer) and remembered. External ACP agents fall back to send now (front of the queue, current turn
  cancelled). New command `chat_steer`, event `steer`, UI command `chat.steer`.
- `Cmd+Shift+Enter` sends the draft in a new background thread (same agent, model, mode, thinking) and keeps you on
  the current one; a toast offers **Open**. UI command `chat.sendInNewThread`.
- Queued messages are editable (click to edit, Enter saves, Esc cancels), reorderable (up/down) and keep Send now /
  Remove. Queued items now persist the full request (model, thinking, effort, agent) so a restart no longer drops
  those choices; older items are rebuilt from the session's current model and agent. Commands `chat_queue_update`,
  `chat_queue_move`.
- Desktop notifications when a thread finishes, fails, needs approval or asks a question (Settings > General >
  Notifications: master switch, only-when-unfocused, per-kind toggles, optional sound). One notification per
  transition, subagents excluded. `agent.chimeOnFinish` is now implemented as a short Web Audio beep, as is
  `notifications.sound`. Adds `tauri-plugin-notification` and the `app_notify_desktop` command.
- Permission modes: Supervised, Auto-accept edits, Auto and Full access replace the single auto-run switch. Pick a mode per thread from the composer (shield icon) or set a default and a cap in Settings > Agent > Permissions; a workspace `.caret/settings.json` can lower the cap for a project. Approval cards show which mode was in effect. (`agent.permissionMode`, `agent.maxPermissionMode`, `chat_set_permission_mode`)
- External agents follow the thread's permission mode: Claude Code and Gemini CLI session modes (`default` / `acceptEdits` / `autoEdit` / `bypassPermissions` / `yolo`) and Codex-style approval config options are set when the session starts and whenever the mode changes. Full access skips Caret's destructive-command prompt for agent terminals; blocked commands stay blocked.
- Worktree policies (Settings > Agent > Worktrees): worktree-per-thread default, location and branch template (`{slug}`, `{model}`, `{id}`, `{date}`), cleanup rules (inactive days, archived, merged, no commits), what to do with local changes (keep / stash / discard), delete branch with worktree, optional automatic cleanup on start with a report notification. Source Control > Worktrees gets a "Clean up agent worktrees" preview and shows thread titles and branches on agent worktrees. (`chat_worktree_status`, `chat_worktree_cleanup_preview`, `chat_worktree_cleanup`)
- Scratch threads: `Chat: New Scratch Thread` starts a chat in its own throwaway git repository under `~/.caret/scratch/<id>`, even with no folder open. A banner offers "Make it a project...", which moves the folder to a directory you pick and opens it as the workspace. (`chat_create_scratch_session`, `chat_promote_scratch`)
- `agent.autoRun` is now the legacy fallback used only when no default permission mode is set (off = Supervised, allowlist = Auto-accept edits, all = Auto).
- Deleting a thread that has a worktree also deletes its branch when `worktrees.deleteBranchWithWorktree` is on and the branch has no unmerged commits.
- Threads: **Fork from here** on any message (and **Fork thread** in the chat header) copies the conversation up to
  that point into a new thread, cut exactly at the turn boundary; the fork shows a "Forked from" link back to its
  parent. **Hand off** a thread mid-conversation to another agent or model from the agent and model pickers: the
  conversation is transferred within `chat.handoffBudgetTokens` (recent messages verbatim, older ones summarised),
  a muted note records the transfer, and the new `read_thread` tool lets the agent fetch summarised-away messages
  verbatim. External (ACP) agents now receive the same budgeted history on their first prompt, including when the
  agent was connected before the first message.
- **Compare models**: shift-click or toggle "Compare models" in the model picker to pick 2 to 4 models; Send becomes
  **Send to N** and runs the prompt in one thread per model (forks of the current thread when it has history, one
  worktree each when enabled). The Agents layout shows the batch side by side with per-model usage and a
  "Continue here" button.
- Settings → Usage: tokens, cost, cache hit rate and request counts for every Caret AI feature (agent chat, external agents in chat, inline edits, Tab, review, commit messages) with Today / 7 days / 30 days / All ranges, a cost-per-day chart, a breakdown by model, provider, agent, source, day or workspace, CSV export and an *Open usage folder* button.
- Local usage ledger at `~/.caret/usage/YYYY-MM.jsonl` (append-only, written off the agent loop), with `usage_summary`, `usage_daily`, `usage_recent`, `usage_export_csv`, `usage_external` and `usage_dir` commands.
- External agents on the Usage page: read-only parsing of Claude Code transcripts, Codex rollouts (including the 5-hour and weekly rate-limit windows as progress bars) and OpenCode messages (JSON tree or `opencode.db`), capped at 200 MB per agent and cached for 60 s.
- Model prices editor (`usage.customPrices`) plus a built-in list-price table for well-known Claude, GPT-5 / Codex, o-series, Gemini and Grok ids; unknown models show *set a price*.
- Status bar item with today's Caret spend that opens the Usage page (hidden at zero).
- Settings `usage.track` and `usage.includeExternal`.
- **Caret as an MCP server (orchestrator).** Settings → MCP → *Caret as an MCP server* exposes a loopback Streamable
  HTTP endpoint (`http://127.0.0.1:17431/mcp`, bearer token stored at `~/.caret/orchestrator/token`) with tools to
  spawn, steer, read and approve Caret threads: `list_agents`, `list_threads`, `create_thread`, `send` (blocks until the
  thread settles and returns the reply, files changed, usage and any pending approval or question), `wait`,
  `read_thread`, `stop`, `pending`, `approve`, `answer`, `delete_thread`. Threads run on any configured provider or
  external ACP agent and appear live in the window. Claude Code, Codex and Cursor/Caret snippets with copy buttons,
  token show/copy/rotate, `orchestrator.enabled` / `orchestrator.port` settings, `endpoint.json` for discovery
  (docs/ORCHESTRATOR.md).
- `AgentManager` now broadcasts every agent event on an in-process channel (`subscribe_events()`) alongside the UI
  channel.
- Remote access: `caret serve [--host 0.0.0.0] [--port N] [--no-window] [--print-url]` and Settings → **Remote** serve
  the full Caret UI to a browser on your LAN or over Tailscale. The Mac keeps running everything; the browser gets
  the same Vite bundle with a shim that relays every `invoke()` (including Channels and events) through the desktop
  webview over an authenticated WebSocket, so every existing command works remotely without a second implementation.
  Browsers pair with a six digit code (five minutes, one use, five attempts per minute per address); tokens are stored
  hashed in `~/.caret/remote/devices.json` and can be revoked from the settings page. New settings `remote.enabled`,
  `remote.host`, `remote.port`; new commands `remote_status`, `remote_pairing_code`, `remote_revoke_device`,
  `remote_executor_secret`, `remote_is_serving`, `remote_set_enabled`, `remote_restart`. See docs/REMOTE.md.
- Website: new `/compare` page with a grouped Caret / T3 Code / Cursor parity table (editor, agents and subscriptions, threads and workflow, safety, remote, footprint), the measured release-build numbers, and links from the header, footer and hero.
- Website: the External agents card and the subscription FAQ now list Claude Code, Codex, Cursor, Grok Build, Gemini CLI, OpenCode and Pi, and mention the signed-in state, Sign in button and second account per provider in Settings → Agents.
- Website: the download grid shows `winget` and `scoop` commands on the Windows tab and the AUR and Nix commands on the Linux tab.
- Packaging: manifests for winget (`packaging/winget`, schema 1.6, `FastEditor.Caret`), scoop (`packaging/scoop/caret.json` with `checkver` and `autoupdate`), the AUR (`packaging/aur`, `caret-bin` from the `.deb`, `provides=('caret')`) and a Nix flake (`flake.nix`, `nix run github:Fast-Editor/Caret` on aarch64-darwin and x86_64-linux).
- Release: `scripts/update-packaging.mjs` (`npm run release:packaging`) rewrites the version and sha256 fields in every manifest from `SHA256SUMS.txt`; a new `packaging` job in the Release workflow runs it after `checksums` and commits the result to main.
- Docs: new `docs/INSTALL.md` covering every install channel and which ones are published today; README install section and `docs/RELEASING.md` "Package managers" updated to match.

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

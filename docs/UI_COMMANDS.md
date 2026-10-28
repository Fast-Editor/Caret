# UI command ids

All cross-module UI actions go through `runCommand(id, args)` (`src/lib/commands.ts`). Modules register
their own ids; nobody imports another module's components to trigger behaviour. Payloads are typed in
`CommandArgs`. The palette lists every non-hidden command; keybindings (`lib/keybindings.ts`) map keys → ids.

| Owner | Ids |
|---|---|
| shell (layout/explorer/search/scm/terminal/palette/settings) | `workspace.*`, `file.*`, `view.*` (incl. `view.ports`, `view.debugConsole`), `palette.*`, `search.*`, `terminal.*`, `git.*`, `settings.*`, `app.*`, `help.*` |
| editor | `editor.*`, `go.back` / `go.forward` (navigation stack; title-bar chevrons), `ai.tab.*`, `ai.inlineEdit`, `ai.quickQuestion`, `ai.explain`, `ai.apply` (delegates to backend), `ai.terminalCmdK` (shared with terminal) |
| debug | `debug.view` (⌘⇧D), `debug.startOrContinue` (F5), `debug.start`, `debug.stop` (⇧F5), `debug.restart` (⌘⇧F5), `debug.pause` (F6), `debug.stepOver` (F10), `debug.stepInto` (F11), `debug.stepOut` (⇧F11), `debug.toggleBreakpoint` (F9; args `{path,line}` optional), `debug.removeAllBreakpoints`, `debug.openConsole`, `debug.evaluate` / `debug.installAdapter` (hidden) |
| chat | `chat.*`, `ai.fixInChat`, `ai.review`, `ai.commitMessage` (opens scm with generated message) |
| ports (`src/components/ports/index.tsx`) | `ports.refresh` (hidden), `ports.open` (hidden, `{ port }`) — Ports panel: local listening servers, open in browser, kill |
| cli (`src/lib/cliOpen.ts`) | `cli.install`, `cli.uninstall`, `cli.status` (hidden), `cli.open` (hidden, `{ path, line?, col? }`) — see docs/CLI.md |

Default keybindings (mac; `mod` = ⌘): palette `mod+shift+p`, quick open `mod+p`, find in files `mod+shift+f`,
toggle sidebar `mod+b`, toggle panel `mod+j`, toggle terminal `` ctrl+` ``, toggle chat `mod+l` (also adds selection),
agent/new chat `mod+i`, inline edit `mod+k`, settings `mod+,`, Caret settings `mod+shift+j`, save `mod+s`, save all `mod+alt+s`,
close tab `mod+w`, split `mod+\`, go to line `ctrl+g`, go to symbol `mod+shift+o`, explorer `mod+shift+e`, scm `ctrl+shift+g`,
agents `mod+shift+a`? → use `mod+e` for background agent (Cursor), accept inline `tab`, accept word `mod+right`,
accept all chat diffs `mod+enter` (when chat focused & changes pending), reject all `mod+backspace`, new terminal `` ctrl+shift+` ``,
toggle agents layout `mod+shift+l`? → keep `mod+alt+a`, zoom `mod+=`/`mod+-`/`mod+0`, commit message `mod+alt+g`.

## Terminal blocks & program status (`src/components/terminal`)

Shell integration (zsh/bash/fish, `core/shell_integration`) emits OSC 133 prompt markers, OSC 7 cwd and the
private `OSC 7770;cmd;<base64>` command marker; CLIs may emit OSC 7501 program status. The backend turns these
into `block-start` / `block-command` / `block-end` / `status` PTY events (`PtyBlockEvent`), and exposes
`pty_blocks`, `pty_block_output`, `pty_status`. The UI mirrors blocks in `store/terminal.ts` and paints a gutter.

| Id | Args | Notes |
|---|---|---|
| `terminal.nextBlock` / `terminal.prevBlock` | – | ⌘↓ / ⌘↑ inside a terminal; selects the block's lines and scrolls to it |
| `terminal.selectBlock` (hidden) | `{ sessionId?, blockId? \| index? }` | defaults to active terminal, last block |
| `terminal.blocks` (hidden) | `{ sessionId? }` | returns `TerminalBlock[]` |
| `terminal.copyBlockOutput` | block ref | plain text (ANSI stripped) |
| `terminal.rerunBlock` | block ref | sends the command again |
| `terminal.blockToContext` | block ref + `{ text?, submit?, mode?, newSession? }` | adds a `note` context item (`$ cmd` + output + `[exit N]`) to chat; AI-gated |
| `terminal.explainBlock` | block ref | "explain / fix" prompt in Ask mode; AI-gated |
| `terminal.blockText` (hidden) | block ref | returns the block as text |
| `terminal.statusSummary` (hidden) | – | `{ terminals[], waiting, working, failed, text }` |
| `terminal.focusWaiting` | – | focuses the first terminal whose program is waiting for input |

Bus topics: `terminal.status` and `agents.terminalStatus` carry the same `TerminalStatusSummary` whenever any
terminal's status changes. `settings.terminal.aiEnabled=false` hides ⌘K, explain and add-to-chat entries.

## Agents (ACP)

- `chat.new` accepts `{ mode?, agentId? }` — `agentId` is `caret` (built-in) or an ACP agent id (`claude-code`, `codex`, `gemini`, `pi`, `glm`, or a custom id). Returns the session id.
- `settings.open` with `{ page: 'agents' }` opens the external agents page.
- Bus topics: `chat:agent-status` (per-session `AcpAgentStatus` fan-out of the `agent-status` agent event), `chat:agent-changed` (composer picker changed `{ sessionId, agentId }`), `acp:changed` (install/toggle/probe finished; pickers reload).
- Automation: `window.__caret.stores.chat` exposes the chat store for `scripts/e2e.mjs`.

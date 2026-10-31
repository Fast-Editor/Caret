# Semantic terminal

Caret's terminal groups each command with its output ("blocks"), knows which terminals are waiting for a human,
and can hand a single command's output to the agent — without parsing prompts heuristically.

## How it works

| Signal | Emitted by | Consumed by |
|---|---|---|
| `OSC 133;A` prompt start, `133;B` command start, `133;C` output start, `133;D;<exit>` command end | Caret shell integration (also any shell/tool that already emits FinalTerm markers) | Rust `BlockTracker` (block list, exit codes, output capture) **and** xterm (`parser.registerOscHandler(133)` → markers for the gutter) |
| `OSC 7;file://host/path` | shell integration | cwd for the tab and for new blocks |
| `OSC 7770;cmd;<base64 command>` (private, Caret only) | shell integration `preexec` | Rust only — stripped before xterm sees it; labels the running block |
| `OSC 7501;state=working\|blocked\|done\|error\|idle\|clear:app=<name>:msg=<base64>` (Program Status Protocol) | Claude Code, Codex, Gemini CLI, builds… | Rust → `status` event → tab dot, in-terminal pill, "N waiting for input" summary, `terminal.status` bus topic |

The Rust side (`src-tauri/src/core/pty.rs`) scans the PTY stream once (`scan_osc`), keeps the text in order,
drops only the private 7501/7770 sequences, and feeds a per-terminal `BlockTracker` (bounded by
`settings.terminal.maxBlocks`, 64 KiB per block, 2 MiB per terminal). Events reach the UI on the same channel as
terminal data: `block-start`, `block-command`, `block-end`, `status`.

## Shell integration (`src-tauri/src/core/shell_integration`)

Assets are embedded and written to `~/.caret/shell-integration/` on first spawn (rewritten only when changed):

- **zsh** — `ZDOTDIR` points at our directory; `.zshenv/.zprofile/.zshrc/.zlogin` shims source the user's files
  (`CARET_USER_ZDOTDIR`) then load `caret-integration.zsh`, and hand `ZDOTDIR` back.
- **bash** — launched with `--init-file <ours>/bash/bashrc` (which emulates login startup when we would have passed `-l`),
  then loads `caret-integration.bash` (PROMPT_COMMAND + DEBUG trap).
- **fish** — our dir is prepended to `XDG_DATA_DIRS`, so `fish/vendor_conf.d/caret.fish` autoloads.

Only applies when `settings.terminal.shellIntegration` is on and the caller did not pass custom shell args.
`CARET_TERMINAL=1` is set; `CARET_SHELL_INTEGRATION_LOADED` guards double-loading.

## Using it

- Gutter bars on the left: accent while running, green on exit 0, red otherwise. Click a bar to select the block;
  ⌘↑ / ⌘↓ jump between blocks. Right-click a block for *Select / Copy command / Copy output / Re-run /
  Add Command Output to Chat / Explain (AI)*.
- Program status: a pill appears top-right while a CLI reports `working`/`waiting`; the tab gets a dot; the panel
  header shows "1 waiting for input · 2 running" and clicking it focuses the waiting terminal
  (`terminal.focusWaiting`).
- Settings → Appearance → Terminal: shell integration, command blocks, blocks kept, program status badges, AI features.

## Verify manually

1. Open a terminal, run `ls`, `false`, `sleep 2`: bars appear (green/red/pulsing), ⌘↑ selects the last block.
2. `printf '\e]7501;state=blocked:app=demo:msg=%s\a' "$(printf 'need a key' | base64)"` → amber pill + "1 waiting for input"; `printf '\e]7501;state=clear\a'` clears it.
3. Right-click a block → *Add Command Output to Chat* → the chat draft gets a `Terminal: <cmd>` context chip.

## Customisation

Settings → Appearance & Editor → **Terminal**:

| Setting | What it does |
| --- | --- |
| Terminal font family / size | Independent of the editor font. |
| Terminal cursor style / blinking | Block, bar or underline. |
| Terminal scrollback | 500–100 000 lines kept per terminal. |
| Terminal background | `Panel` (default), `Match editor`, `App background`, or `Transparent` (the panel behind shows through). |
| Dim inactive split panes | Fades the panes of a split that do not have focus. |
| Shell integration | Injects the OSC 133 hooks. Turning it off disables blocks, suggestions and the fix-it chip. |
| Command blocks / Blocks kept per terminal | Gutter bars per command, ⌘↑/⌘↓ navigation, copy/re-run/send to chat. |
| Program status badges | OSC 7501 status from CLIs (working, waiting for input, done, failed). |
| Terminal AI features | ⌘K command generation, explain output, add output to chat, fix-it, AI suggestions. |
| Command suggestions | `Off`, `History only`, or `History + AI` (default). |

**Terminal colours** (same page, below the fields): the 16 ANSI colours for the *current* app theme. Each theme keeps
its own palette; the swatches edit live. `Import…` reads a Windows Terminal scheme (a scheme object or a whole
`settings.json`, first scheme), a VS Code `workbench.colorCustomizations` block (`terminal.ansi*` keys), or an iTerm2
`.itermcolors` file. `Paste` accepts the same formats from the clipboard, `Export` copies the palette as Windows
Terminal JSON, `Reset` returns to the theme defaults. Background and foreground always follow the app theme.

**Per-terminal colour**: right-click a terminal in the list → *Colour*. The colour tints that terminal's background
and its icon, so split panes and long-running servers are easy to tell apart. Stored per session, not persisted.

**Shells**: Settings → General → Default shell, or the chevron next to `+` for a one-off shell / `Custom shell…`.

## Clickable paths

Output is scanned for file paths: `src/server.js`, `package-lock.json`, `./dist/bundle.js`, `/abs/path`, `~/notes.md`,
with optional `:line[:col]` or `(line,col)` suffixes. A path becomes a link only if it exists, resolved first against
the terminal's current directory and then the workspace root; hovering underlines it and clicking opens the file in
the editor at that line. URLs still open in the browser. Detection lives in `src/components/terminal/pathLinks.ts`.

## Command suggestions

While you type at a prompt, Caret looks the typed prefix up in your command history (commands run in any open
terminal first, then the persisted cross-session history) and shows the rest of the best match as dim ghost text after
the cursor. Press **→** or **End** to accept, **Esc** to dismiss; keep typing to refine. With `History + AI`, when
history has nothing and you pause for 0.7 s on three or more characters, the Tab model is asked for a continuation
(your recent commands, cwd and shell are the context; the request is cancelled if you keep typing). Destructive
completions are discarded.

## Fix-it

When the last command exits non-zero, a **Fix last command** chip appears at the bottom right of the terminal (also
*Fix Command (AI)* in the block's right-click menu, or the `Terminal: Fix Last Failed Command (AI)` command). Caret
sends the command, exit code and the output tail to the model and opens the ⌘K bar with the proposed command,
editable, with a one-line explanation. **Enter** runs it, **Tab** inserts it without running, **Esc** cancels. Proposals
that would discard work (resets, force pushes, `rm -rf`) are never auto-filled; the explanation says what you would have
to decide instead.

## Cross-session history and natural-language search

Every finished command (command, cwd, shell, exit code, duration, first 2 KB of output) is appended to
`~/.caret/terminal/history.jsonl` (capped, secret-like exports and bearer tokens are redacted). **⌘⌥H** or
`Terminal: Search Command History` opens a palette over that history: one or two words search lexically; a longer
description such as *"the docker command that failed yesterday"* is turned into keywords by the model first, then
searched. **Enter** inserts the command into the active terminal, **⌘Enter** runs it. `Terminal: Clear Command History`
wipes the file.

## Keys

| Key | Action |
| --- | --- |
| ctrl+` | Toggle the terminal panel |
| ctrl+shift+` | New terminal |
| ⌘K | Generate a command from a description |
| ⌘F | Find in terminal |
| ⌘↑ / ⌘↓ | Previous / next command block |
| → / End | Accept the ghost suggestion |
| ⌘⌥H | Search command history |

All of them can be changed in Settings → Keybindings.

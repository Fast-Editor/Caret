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

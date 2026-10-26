# Command safety & sandbox

Caret treats every shell command the agent proposes as untrusted input. Two independent layers sit **below the model** — no LLM is involved in either:

1. **Classification** (`src-tauri/src/sandbox/{shell_ast,classify}.rs`): a hand-written POSIX shell parser produces a command tree (pipelines, `&&`/`||`/`;`, subshells, `$( )`/backticks, process substitution, redirects, nested `sh -c` / `eval` / `xargs` / `find -exec` / `ssh host 'cmd'` / `python -c` / `node -e` payloads, wrappers such as `sudo`/`env`/`nohup`/`timeout`). A rule-based classifier then assigns each segment a risk and stable labels.
2. **OS sandbox** (`seatbelt.rs` on macOS, `bwrap.rs` on Linux): approved commands run with kernel-enforced limits — writes only inside the workspace, temp and package caches; credential folders unreadable; network optional.

Plus **MCP tool pinning** (`mcp_pins.rs`): tool descriptions/schemas are fingerprinted on first use; a later change forces a fresh approval.

## Risk ladder

| Risk | Meaning | Policy |
|---|---|---|
| `safe` | no side effects beyond the workspace (`ls`, `npm test`, `git status`, `cargo check`) | follows auto-run setting |
| `caution` | reversible or reviewable side effects (`npm install`, `git push`, `curl`, `sudo …`, nested shells, command substitution) | follows auto-run setting; labels shown on the card |
| `destructive` | data loss or hard-to-reverse effects (`rm -rf`, `git push --force`, `git reset --hard`, `docker system prune`, `DROP TABLE`, `curl … \| sh`, secrets + network) | **always asks**, even with auto-run "All"; "allow for session / always" applies to the exact command, not the prefix |
| `blocked` | catastrophic or never legitimate from an agent (`rm -rf /`, `rm -rf ~`, system paths, `mkfs`, `dd of=/dev/…`, `shutdown`, fork bombs, `chmod -R 777 /`, writing `/etc/*`, denylist hits) | **never runs**; the agent is told why |

Labels (examples): `delete`, `delete-recursive`, `force`, `wildcard`, `outside-workspace`, `root-path`, `home-path`, `system-path`, `vcs-destructive`, `discard-changes`, `remote-code`, `secrets`, `secrets-read`, `network`, `installs`, `runs-remote-package`, `privilege`, `nested-shell`, `eval`, `command-substitution`, `batch-exec`, `infra-destructive`, `data-loss`, `container-destructive`, `shell-rc`, `writes-outside-workspace`, `kills-processes`, `disk`, `system-power`, `fork-bomb`, `denylisted`.

Evasion coverage (all tested in `classify.rs`): `\rm`, `sh -c "…"`, `bash -lc '…'`, `eval "…"`, `echo $(rm …)`, backticks, `(cd x && rm …)`, `xargs rm -rf`, `find … -exec rm`, `python3 -c "os.system('…')"`, `node -e "execSync('…')"`, `ssh host '…'`, `timeout 5 rm …`, `sudo sh -c '…'`, `~`/`$HOME` expansion, `..` escapes, globs.

Path context: `outside-workspace` is computed against the session cwd/workspace root; `/tmp` and `$TMPDIR` count as neither outside nor system.

## Where it is enforced

- `ai/agent/policy.rs` → `decide()` Shell branch: denylist → classification (`blocked` ⇒ `Deny`) → session allow → auto-run (destructive never auto-runs). `Decision::NeedsApproval` carries the `classification`.
- `ai/agent/runner.rs`: classification + `riskTier` (0/1/2) + `sandboxed` are attached to the `ApprovalRequest` and the `ToolCallRecord`, so the card shows pills whether or not approval was needed.
- `ai/agent/tools/shell_tools.rs`: `policy_for(settings, root, cwd)` → `wrap_with(...)` for foreground and background commands; `CARET_SANDBOXED=1` in the child env; strict mode errors when no backend exists.
- Hooks (`beforeShellExecution`) still run first and can veto.

## Sandbox profile (macOS Seatbelt)

Generated per command (`seatbelt::profile`), `sandbox-exec -p <profile> <shell> -lc <command>`:

- `(deny default)`, then allow process/mach/sysctl/ipc/iokit/system/pty so ordinary tooling works (git, node, python, cargo, brew-installed binaries).
- `file-read*` everywhere **except** credential locations: `~/.ssh ~/.aws ~/.gnupg ~/.azure ~/.kube ~/.config/gcloud ~/.config/gh ~/.config/op ~/.docker ~/.netrc ~/.npmrc ~/.pypirc ~/.gem/credentials ~/.git-credentials ~/.cargo/credentials* ~/.terraform.d/credentials* ~/.vault-token ~/.claude/.credentials.json ~/.codex/auth.json`, keychains, browser profiles, `*.pem *.p12 *.pfx id_rsa id_ed25519 …`, and `~/.caret` (except `~/.caret/sandbox`, `~/.caret/logs`).
- `allowReadHome=false` additionally denies `file-read-data` under `$HOME` except the workspace, toolchains/caches (`.cargo .rustup .nvm .volta .bun .pyenv .local .config .cache .npm …`) and shell rc files (login shells keep working).
- `file-write*` only under: workspace, session cwd, `/tmp`, `/private/tmp`, `/var/tmp`, `$TMPDIR` and its parent (`…/T`, `…/C`), package caches (`~/.cache ~/Library/Caches ~/.npm ~/.yarn ~/.pnpm-store ~/.cargo/registry ~/.cargo/git ~/.rustup ~/.gradle ~/.m2/repository ~/go/pkg ~/.config/configstore …`), `~/.caret/sandbox`, `~/.caret/logs`, user "extra writable paths"; plus `/dev/null`, ttys, `/dev/fd/*`. Credential dirs and shell rc files are denied for write even if a writable root contains them.
- Network: `(allow network*)` when allowed; otherwise `(deny network*)` with unix sockets permitted.

Semantics verified empirically on macOS 26 (last matching rule wins): deny-after-allow for secrets, re-allow of `~/.caret/sandbox`, write denial in `$HOME`, `connect()` ⇒ `EPERM`, `curl` fails, login `zsh`/`node`/`python3`/`git` work.

Linux (`bwrap`): `--ro-bind / /`, `--dev /dev --proc /proc`, `--bind` for each writable root, `--tmpfs` over credential dirs, `--tmpfs $HOME` + selective ro-binds when home reads are off, `--unshare-net` when network is off, `--die-with-parent`.

`sandbox::status()` probes the backend once per process by running `/usr/bin/true` under a real generated profile; Settings → Agent → Sandbox shows the result.

## Settings (`settings.sandbox`)

| Key | Default | Effect |
|---|---|---|
| `mode` | `auto` | `off` run directly · `auto` sandbox when available · `strict` refuse without sandbox |
| `allowNetwork` | `true` | `false` cuts sandboxed commands off the network |
| `allowReadHome` | `true` | `false` restricts reads under `$HOME` to workspace/toolchains/caches |
| `extraWritablePaths` | `[]` | extra writable roots (absolute or `~/`) |
| `pinMcpToolDescriptions` | `true` | re-approve MCP tools whose description/schema changed |

## MCP tool pinning

`mcp_pins::check(&tools)` runs whenever the tool catalog is built. Fingerprint = blake3(description + canonical input schema). First sighting pins (trust on first use). A different fingerprint records a pending change in `~/.caret/mcp-pins-pending.json`; `policy.rs` then returns `NeedsApproval` titled "MCP tool changed: <tool>" with before/after text, regardless of `mcpAutoRun`. Approving re-pins; Settings → Agent → Sandbox lists pending changes with an "Accept new version" button. Commands: `mcp_tool_changes`, `mcp_acknowledge_tool`.

## UI

- Approval card (`ToolCallCard.tsx` → `ApprovalBox`): risk pill, labels, per-segment risks for multi-command lines, reasons, `sandboxed`/`unsandboxed` chip, red styling and "exact command only" note for destructive, no Allow buttons for blocked.
- Finished shell cards: compact risk pill + shield icon in the header, labels under the command.
- Settings → Agent → "Sandbox & command safety": backend status, live classifier ("try a command"), pending MCP changes; schema fields for all `sandbox.*` keys.

## Tests

`cargo test --lib sandbox` — parser (operators, quoting, redirects, wrappers, nested shells, substitutions, garbage fuzz), classifier (safe set, rm variants, evasions, git, blocked set, remote code & secrets, installs, infra/db/docker, redirects, allow/deny integration, tiers), Seatbelt enforcement (real `sandbox-exec`: workspace writes ok, outside writes denied, credentials unreadable, network toggle, home restriction), bwrap arg shape, pin store flow.

## Using `sandbox::wrap` from Rust

`wrap` rebuilds the command as `sandbox-exec -p <profile> -- <program> <args…>` (or `bwrap …`), keeping program,
arguments, environment and working directory. Standard library `Command` has no getters for stdio, so **configure
`stdin`/`stdout`/`stderr` and `kill_on_drop` after wrapping**, never before. The verify engine's shell runner and the
agent's shell tool both follow this order.

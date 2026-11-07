# Caret documentation

Caret is a Cursor-class AI code editor with a Rust core (Tauri 2 + React/Monaco). This folder is the reference; the
top-level [README](../README.md) has the overview, install instructions and the step-by-step build guide.

## Install and update

- **macOS one-liner:** `curl -fsSL https://raw.githubusercontent.com/Fast-Editor/Caret/main/scripts/install.sh | sh`
- **Homebrew:** `brew install --cask fast-editor/lynkr/caret`
- **DMG:** download from the [latest release](https://github.com/Fast-Editor/Caret/releases/latest); builds are ad-hoc
  signed, so after dragging to Applications run `xattr -dr com.apple.quarantine /Applications/Caret.app` once (or use
  System Settings → Privacy & Security → *Open Anyway*).
- **Windows / Linux:** `.msi` / `-setup.exe` and `.deb` / `.rpm` / `.AppImage` on every release (built, not yet exercised at runtime).
- **Package managers:** `winget install FastEditor.Caret`, `scoop install https://raw.githubusercontent.com/Fast-Editor/Caret/main/packaging/scoop/caret.json`,
  `yay -S caret-bin`, `nix run github:Fast-Editor/Caret`. Manifests live in `packaging/` and `flake.nix`; winget, scoop and
  the AUR are awaiting first publication. Every channel and its status: [INSTALL.md](INSTALL.md).
- **Updates:** Settings → About → *Check for updates* (also checked silently after launch).

## Guides

| Document | What it covers |
| --- | --- |
| [INSTALL.md](INSTALL.md) | Every install channel with exact commands: install script, Homebrew, DMG and quarantine, winget, scoop, AUR, Nix flake, direct downloads, updater; which are published today |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Module map, IPC conventions, memory rules, how the Rust core and the React frontend talk |
| [UI_COMMANDS.md](UI_COMMANDS.md) | The UI command registry (`runCommand` ids) and bus topics |
| [PROVIDERS.md](PROVIDERS.md) | Configuring AI providers and models: Anthropic, OpenAI, Azure, OpenAI-compatible servers, Ollama, OpenRouter, Gemini; keys; embeddings |
| [AUTO_ROUTING.md](AUTO_ROUTING.md) | "Auto" model routing through the managed Lynkr sidecar: tiers, overrides, security model, troubleshooting |
| [THREADS.md](THREADS.md) | Chat threads: pin, snooze, settle, archive with undo, grouped list and group by project, message search, queue vs steer, background send, notifications, forks, provider handoff, multi-model fan-out, worktree policies and scratch threads |
| [ACP.md](ACP.md) | External agents over the Agent Client Protocol (Claude Code, Codex, Cursor, Grok Build, Gemini CLI, OpenCode, Pi…), per-agent model/effort/mode, sign-in and extra accounts |
| [ORCHESTRATOR.md](ORCHESTRATOR.md) | Caret as an MCP server: let Claude Code, Codex, Cursor or another Caret spawn, steer, read and approve Caret threads on any provider or external agent (loopback, bearer token) |
| [VERIFIED_DIFF.md](VERIFIED_DIFF.md) | The verification engine that runs after agent turns: checks, risk tiers, proof panel, build-before-test |
| [SANDBOX.md](SANDBOX.md) | Shell command classification, macOS Seatbelt / Linux bubblewrap sandboxing, MCP tool pins |
| [TERMINAL.md](TERMINAL.md) | Semantic terminal blocks (OSC 133 / 7501), shell integration, program status, customisation (colours, suggestions, fix-it, clickable paths, history search) |
| [DEBUGGING.md](DEBUGGING.md) | Run and Debug: the Debug Adapter Protocol client, js-debug and debugpy adapters, launch.json compatibility, limitations |
| [CLI.md](CLI.md) | The `caret` command-line launcher (`caret .`, `caret file:line:col`, `caret serve`), how it is installed and how it talks to the app |
| [REMOTE.md](REMOTE.md) | Remote access (`caret serve`): the full Caret UI in a browser on your LAN or over Tailscale, relayed through the desktop; pairing, tokens, limitations |
| [RELEASING.md](RELEASING.md) | Cutting releases: version bump, GitHub Actions matrix, secrets, code signing, updater, package managers (Homebrew, winget, scoop, AUR, Nix) |
| [PERFORMANCE.md](PERFORMANCE.md) | Measured memory and latency numbers and the rules that keep them low |
| [USAGE.md](USAGE.md) | Settings → Usage: the local token and cost ledger, how requests are priced, external agent histories (Claude Code, Codex, OpenCode) and their limits, CSV export |

## Where things live on disk

- User configuration: `~/.caret/` — `settings.json`, `secrets.json` (API keys), `keybindings.json`, `mcp.json`, rules, memories,
  sessions, checkpoints, the search index, `bin/caret` (launcher), `acp/` (external agent adapters), `lynkr/` (Auto routing sidecar),
  `orchestrator/` (`token` and `endpoint.json` for the MCP server that lets other agents drive Caret),
  `remote/devices.json` (hashed tokens of browsers paired through `caret serve`, mode 0600).
- Usage ledger: `~/.caret/usage/YYYY-MM.jsonl`, one line per priced request (agent chat, external agents, inline edits, Tab, review,
  commit messages); read by Settings → Usage and the status bar spend item. External agent histories under `~/.claude`, `~/.codex`
  and `~/.local/share/opencode` are only ever read.
- Project configuration: `<repo>/.caret/` (rules and project settings); `.cursor/rules` and `AGENTS.md` are read when present.

## Contributing to the docs

Each subsystem's document is the source of truth for its behaviour; update it in the same commit as the code change.
The README's build guide and this index should stay short and link here rather than duplicate.

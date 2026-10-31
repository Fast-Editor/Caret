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
- **Updates:** Settings → About → *Check for updates* (also checked silently after launch).

## Guides

| Document | What it covers |
| --- | --- |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Module map, IPC conventions, memory rules, how the Rust core and the React frontend talk |
| [UI_COMMANDS.md](UI_COMMANDS.md) | The UI command registry (`runCommand` ids) and bus topics |
| [PROVIDERS.md](PROVIDERS.md) | Configuring AI providers and models: Anthropic, OpenAI, Azure, OpenAI-compatible servers, Ollama, OpenRouter, Gemini; keys; embeddings |
| [AUTO_ROUTING.md](AUTO_ROUTING.md) | "Auto" model routing through the managed Lynkr sidecar: tiers, overrides, security model, troubleshooting |
| [ACP.md](ACP.md) | External agents over the Agent Client Protocol (Claude Code, Codex, Gemini CLI, OpenCode…), per-agent model/effort/mode |
| [VERIFIED_DIFF.md](VERIFIED_DIFF.md) | The verification engine that runs after agent turns: checks, risk tiers, proof panel, build-before-test |
| [SANDBOX.md](SANDBOX.md) | Shell command classification, macOS Seatbelt / Linux bubblewrap sandboxing, MCP tool pins |
| [TERMINAL.md](TERMINAL.md) | Semantic terminal blocks (OSC 133 / 7501), shell integration, program status, customisation (colours, suggestions, fix-it, clickable paths, history search) |
| [DEBUGGING.md](DEBUGGING.md) | Run and Debug: the Debug Adapter Protocol client, js-debug and debugpy adapters, launch.json compatibility, limitations |
| [CLI.md](CLI.md) | The `caret` command-line launcher (`caret .`, `caret file:line:col`), how it is installed and how it talks to the app |
| [RELEASING.md](RELEASING.md) | Cutting releases: version bump, GitHub Actions matrix, secrets, code signing, updater, Homebrew cask |
| [PERFORMANCE.md](PERFORMANCE.md) | Measured memory and latency numbers and the rules that keep them low |

## Where things live on disk

- User configuration: `~/.caret/` — `settings.json`, `secrets.json` (API keys), `keybindings.json`, `mcp.json`, rules, memories,
  sessions, checkpoints, the search index, `bin/caret` (launcher), `acp/` (external agent adapters), `lynkr/` (Auto routing sidecar).
- Project configuration: `<repo>/.caret/` (rules and project settings); `.cursor/rules` and `AGENTS.md` are read when present.

## Contributing to the docs

Each subsystem's document is the source of truth for its behaviour; update it in the same commit as the code change.
The README's build guide and this index should stay short and link here rather than duplicate.

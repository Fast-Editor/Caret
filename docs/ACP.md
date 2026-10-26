# External agents (Agent Client Protocol)

Caret can hand a chat session to an external coding agent that speaks the
[Agent Client Protocol](https://agentclientprotocol.com) (ACP): Claude Code, Codex,
Gemini CLI, OpenCode, Pi, GLM, or any program you add. The agent runs as a child process on your
machine with its own login and billing. Caret is the ACP *client*: it owns the UI, the
review bar, approvals, checkpoints and terminals; the agent owns the model loop.

## Using it

1. **Settings → Agents.** Builtin agents show *Not installed* until you click **Install**
   (runs `npm install <adapter>@latest` into `~/.caret/acp`, never your global prefix) or
   install them yourself with the shown command. **Connect** starts the agent and runs the
   `initialize` handshake so you can see its version, capabilities, modes and sign-in state.
2. **Composer → agent picker** (left of the mode selector). Pick *Caret* (built-in) or an
   external agent per chat. The choice is stored on the session; the agent starts on the
   first message of that session.
3. Chat as usual. Streaming text, thoughts, tool cards, plans/todos, permission prompts,
   file diffs and terminal output all render in the normal chat UI. Pending changes appear
   in the review bar with accept/reject and are checkpointed for restore.

Confirmed launch commands (October 2026):

| Agent | Command | Install |
| --- | --- | --- |
| Claude Code | `claude-agent-acp` | `npm i -g @agentclientprotocol/claude-agent-acp` (uses your Claude Code login or `ANTHROPIC_API_KEY`) |
| Codex | `codex-acp` | `npm i -g @agentclientprotocol/codex-acp` (bundles codex; ChatGPT login or `OPENAI_API_KEY`) |
| Gemini CLI | `gemini --experimental-acp` | `npm i -g @google/gemini-cli` |
| OpenCode | `opencode acp` | `brew install anomalyco/tap/opencode` or `npm i -g opencode-ai` |
| Pi | `pi-acp` | `npm i -g pi-acp` |
| GLM (Z.AI) | `glm-acp-agent` | `npm i -g glm-acp-agent` (`ZAI_API_KEY`) |

Custom agents: any executable + args + env. Caret resolves commands against PATH plus
`~/.caret/acp/node_modules/.bin`, Homebrew, nvm/fnm/volta/bun locations (GUI apps on macOS
inherit a minimal PATH).

## Model, effort and permission mode

External agents expose their own knobs through ACP **session config options**
(`session/new` → `configOptions`, changed with `session/set_config_option`, refreshed by
`config_option_update`). As soon as you pick an external agent in the composer, Caret
connects it and creates the session (`acp_prepare_session`) so the pickers appear before the
first message:

| Agent | Options it advertises (Oct 2026) |
| --- | --- |
| Claude Code | **Model** (default / sonnet / opus / haiku / …), **Effort** (low → max), **Mode** (Manual, Accept edits, Plan, Auto, Bypass) |
| OpenCode | **Model** — every model of every provider you configured in OpenCode (searchable) |
| Others | whatever they publish; agents that only publish `modes` get a Mode picker via `session/set_mode` |

Your pick is sent to the agent immediately and remembered per agent in
`settings.acp.sessionDefaults` (`{ "claude-code": { "model": "haiku", "effort": "low" } }`), then
re-applied to every new session with that agent. Caret's own model / mode / thinking pickers are
hidden while an external agent drives the chat — they only affect Caret's built-in agent.

## Mapping (ACP ↔ Caret)

| ACP | Caret |
| --- | --- |
| `initialize` (protocol v1, client caps `fs.read/write`, `terminal`) | `AcpAgentStatus` (version, capabilities, auth methods) |
| `session/new {cwd, mcpServers}` | one ACP session per Caret session (`SessionLink`), created lazily on the first prompt; your enabled MCP servers are passed when *Share MCP servers* is on |
| `session/prompt` | one Caret turn: user message + checkpoint, assistant message, `MessageEnd` with status from `stopReason` (`end_turn`→done, `cancelled`→cancelled, `refusal`→error) |
| `session/cancel` | **Stop** button (`chat_stop`) |
| `agent_message_chunk` / `agent_thought_chunk` | `TextDelta` / `ThinkingDelta` on the streaming assistant message |
| `tool_call` / `tool_call_update` | `ToolCallRecord` (`acp_<kind>`, title, status, locations, rawInput→input, rawOutput→output) with `ToolStart`/`ToolUpdate` |
| `diff` content block | checkpoint snapshot + `PendingChange` (review bar, proof panel). Folded into the `fs/write_text_file` change of the same file when the agent wrote through Caret; when the agent wrote the file itself, snippet-style `oldText`/`newText` (Claude Code's Edit tool) are expanded to whole-file before/after so accept/reject is always safe |
| `terminal` content block | command + output + exit code on the tool card |
| `plan` | session todos (`Todos` event) |
| `usage_update` | `Usage` event (`contextTokens`, cost) |
| `current_mode_update`, `available_commands_update`, `session_info_update` | `AgentStatus` event (modes), slash command list, session title |
| `session/request_permission` | `ApprovalRequest` through the same approval UI as the built-in agent. ACP option kinds map to Allow / Allow always / Deny / Deny always; Caret adds *Allow for session*, remembered per command (execute) or per tool kind |
| `fs/read_text_file`, `fs/write_text_file` | direct file IO relative to the session cwd; writes are checkpointed and tracked as pending changes |
| `terminal/create|output|wait_for_exit|kill|release` | child processes owned by Caret with a byte-limited ring buffer |

Sessions that already have history (e.g. after restarting Caret) get a short
`<previous_conversation>` block on their first prompt so the agent has continuity.

After a turn that changed files, the Verified Diff engine auto-runs exactly as it does for the built-in agent
(`verify.autoRunAfterTurn`), so the proof panel and risk tier cover external agents too.

## Code

- `src-tauri/src/acp/protocol.rs` — newline-delimited JSON-RPC 2.0 over stdio. Notifications are
  applied in arrival order before any later response is delivered; agent→client requests run concurrently.
- `src-tauri/src/acp/process.rs` — spawning, command resolution, stderr → per-agent log ring (`acp_logs`).
- `src-tauri/src/acp/client.rs` — `AgentConn` (one per agent per workspace): initialize, session/new,
  session/load, prompt, cancel, set_mode, authenticate.
- `src-tauri/src/acp/bridge.rs` — the mapping above (`run_turn`, `Bridge: IncomingHandler`).
- `src-tauri/src/acp/registry.rs` — builtin catalog, user overrides (`settings.acp.agents`), npm install.
- `src-tauri/src/acp/commands.rs` — `acp_*` Tauri commands; `chat_create_session` / `chat_send` / `chat_stop`
  route to the bridge when the session has an `agentId` other than `caret`.
- UI: `src/components/chat/composer/AgentPicker.tsx`, `src/components/settings/pages/Agents.tsx`.

## Tests

- `cargo test --lib acp::` — protocol/registry unit tests plus integration tests that drive the whole bridge
  against `tests/fixtures/fake_acp_agent.mjs` (streaming, permission allow/deny, fs write + diff dedupe,
  terminal round trip, plan → todos, cancel, refusal, auth-required → authenticate, set_mode).
- `CARET_LIVE_TEST=1 cargo test --lib acp::tests::live -- --nocapture` — real Claude Code round trip through
  `claude-agent-acp` using your own login (verified 2026-10-26: "pong" in ~23 s, 5 modes reported).

## Gaps

- `session/load` (resuming the agent's own context after a restart) is not used yet; Caret re-creates the
  ACP session and sends a transcript summary instead.
- `session/set_mode` is exposed via `acp_set_mode` and the status event, but the composer's mode selector
  still shows Caret modes; an ACP-mode picker is a follow-up.
- Agent terminals (`terminal/create`) run unsandboxed and are not classified; the approval card shows an
  *unsandboxed* chip. Wrapping them with `sandbox::wrap` + `sandbox::classify` is the next step.
- Token counts are not mapped from `usage_update` (only context size and cost), so cards show `0 in · 0 out`.
- Audio prompt blocks and `resource_link` for non-file mentions are not sent.
- The agent's slash commands (`available_commands_update`) are captured (`acp_session_info`) but not yet
  surfaced in the slash command picker.

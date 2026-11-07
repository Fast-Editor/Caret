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

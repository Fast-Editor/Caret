### Added

- Settings → Usage: tokens, cost, cache hit rate and request counts for every Caret AI feature (agent chat, external agents in chat, inline edits, Tab, review, commit messages) with Today / 7 days / 30 days / All ranges, a cost-per-day chart, a breakdown by model, provider, agent, source, day or workspace, CSV export and an *Open usage folder* button.
- Local usage ledger at `~/.caret/usage/YYYY-MM.jsonl` (append-only, written off the agent loop), with `usage_summary`, `usage_daily`, `usage_recent`, `usage_export_csv`, `usage_external` and `usage_dir` commands.
- External agents on the Usage page: read-only parsing of Claude Code transcripts, Codex rollouts (including the 5-hour and weekly rate-limit windows as progress bars) and OpenCode messages (JSON tree or `opencode.db`), capped at 200 MB per agent and cached for 60 s.
- Model prices editor (`usage.customPrices`) plus a built-in list-price table for well-known Claude, GPT-5 / Codex, o-series, Gemini and Grok ids; unknown models show *set a price*.
- Status bar item with today's Caret spend that opens the Usage page (hidden at zero).
- Settings `usage.track` and `usage.includeExternal`.

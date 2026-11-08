# Usage: tokens, cost and subscription windows

Settings → Usage shows what Caret's AI features cost, where the tokens went, and (read-only) what the external
coding agents installed on the machine have been doing. Everything is computed locally from files on disk; nothing is
sent anywhere.

## What is recorded

Every priced request made by Caret is appended to a local ledger as soon as the provider reports its token counts:

| Source     | Where it is recorded                                        | Notes |
| ---------- | ----------------------------------------------------------- | ----- |
| `agent`    | built-in agent chat, `ai/agent/runner.rs`                    | priced with the model that actually served the turn (Auto routing included); carries session id, workspace and duration |
| `acp`      | external agents run inside Caret chat, `acp/bridge.rs`        | ACP `usage_update` reports a running cost, so the ledger stores the delta against the session's previous cost; token fields are recorded when the agent sends them |
| `inline`   | Cmd+K inline edits and quick questions, `ai/inline_edit.rs`  | |
| `tab`      | Tab completions, `ai/tab.rs`                                 | one entry per completion request (cache hits are not requests) |
| `review`   | Bugbot-style review, `ai/review.rs`                          | one entry per file batch |
| `commit`   | commit message generation, `ai/commit.rs`                    | |

An entry looks like this (one JSON object per line, camelCase):

```json
{"ts":1773144000000,"source":"agent","providerId":"anthropic","modelId":"claude-opus-5-5","agentId":"caret","sessionId":"…","workspace":"/Users/me/proj","input":1200,"output":340,"cacheRead":9000,"cacheWrite":800,"thinking":0,"costUsd":0.0217,"durationMs":5400}
```

Recording is cheap and never blocks the agent loop: `ai::usage::record` hands the entry to a dedicated writer thread
through a channel; write errors are logged with `tracing::warn` and otherwise ignored. Turn it off with Settings → Usage
→ *Record usage* (`usage.track`); existing entries stay visible.

## Older sessions

The ledger only exists from the release that introduced it, so on first start Caret imports the usage stored on the
assistant messages of every saved chat session (`~/.caret/sessions/<workspace>/<id>.json`) into the ledger once
(marker file `~/.caret/usage/.sessions-backfilled-v1`). Messages that carried no cost are priced from the current
model prices. **Import past sessions** on the Usage page runs the same import again; it is idempotent (an entry
with the same session and timestamp is skipped), so it is safe after restoring sessions from a backup.

## Where things live on disk

- `~/.caret/usage/YYYY-MM.jsonl`: the ledger, one file per calendar month (local time), append-only. `CARET_HOME`
  relocates it like every other Caret file. *Open usage folder* on the page reveals it in the file manager.
- Nothing else is written. External agent histories are only read.

Deleting a month file forgets that month; the ledger has no index to rebuild.

## How requests are priced

1. Caret's own requests are priced at emit time with the model's catalog prices (`inputPricePerM`, `outputPricePerM`,
   `cacheReadPricePerM`, `cacheWritePricePerM`; cache read defaults to 10% of input, cache write to 125%). Entries from
   external agents inside Caret keep the cost the agent reported.
2. When an entry or an external record has no cost, the aggregation looks the model id up in this order:
   `usage.customPrices` (exact id first, then the longest matching prefix, case-insensitive), the model catalog (by Caret
   id or provider model name), then a built-in table of public list prices for well-known ids (current Claude Opus,
   Sonnet and Haiku, the GPT-5 family including the Codex variants, GPT-4.1 and 4o, the o-series, Gemini 2.5 and 3,
   Grok 4). Ids are normalised before matching: provider prefixes (`anthropic/`), `[1m]`, `@version`, `:tag` and
   `-latest` are dropped, so `claude-sonnet-4-5-20250929` matches `claude-sonnet-4-5`.
3. Models with no price anywhere show **set a price** instead of a number; their requests and tokens are still counted
   and the `unpriced` counter tells you how many requests a cost figure is missing.

Add or fix prices under *Model prices* on the page (USD per million tokens; cache columns optional). They are stored in
`usage.customPrices` in `settings.json`, keyed by model id or prefix.

## The page

- **Range**: Today, 7 days, 30 days, All. Ranges start at local midnight and apply to every section.
- **Cards**: tokens in (fresh + cached), tokens out (with thinking tokens where reported), cache hit rate
  (cache reads over all prompt tokens), cost, requests.
- **Cost per day**: CSS bar chart of the ledger; series longer than 120 days are bucketed.
- **Breakdown**: group by model, provider, agent, source, day or workspace.
- **External agents**: one block per agent (see below) with totals, a by-model table and subscription windows as
  progress bars with their reset time. Agents whose history directory does not exist are listed as
  *not found on this machine*.
- **Model prices**: the `usage.customPrices` editor.
- **Export CSV** writes the entries of the selected range (`ts,time,source,provider,model,agent,session,workspace,
  input,output,cache_read,cache_write,thinking,cost_usd,duration_ms`) through the save dialog, or copies the CSV to the
  clipboard when no dialog is available.
- The status bar shows today's Caret spend (for example `$0.42 today`) and opens the page; it is hidden at zero.

## External agents

Scanning is read-only and bounded: only files modified inside the selected range are opened where the format allows
it, at most 1 GB of history is read per agent (a note says when the cap was hit), and results are cached for 60
seconds per range. Turn it off with *Include external agents* (`usage.includeExternal`).

| Agent | Files read | Fields used | Limits |
| --- | --- | --- | --- |
| Claude Code | `~/.claude/projects/<project>/<session>.jsonl` (also `CLAUDE_CONFIG_DIR`) | lines with `type: "assistant"`: `timestamp`, `message.model`, `message.usage.{input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens}`; several lines share one `message.id` per API response and are counted once; `<synthetic>` models are skipped | Subscription windows are not stored on disk; use `/usage` inside Claude Code. Cost is an estimate at list prices. |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` (also `CODEX_HOME`) | `event_msg` lines whose `payload.type` is `token_count`: `payload.info.last_token_usage.{input_tokens, cached_input_tokens, output_tokens, reasoning_output_tokens}` summed per event (cached input is split out as cache reads), `payload.rate_limits.{primary,secondary}.{used_percent, window_minutes, resets_at or resets_in_seconds}`; the model comes from `turn_context` or `session_meta` | The newest `rate_limits` across all files become the windows. Every rollout file is opened for rate limits (not only those in range), within the byte cap. |
| OpenCode | `~/.local/share/opencode/storage/message/<session>/*.json` (also `XDG_DATA_HOME`), or `opencode.db` when the JSON tree is absent | assistant messages: `modelID`, `providerID`, `cost`, `tokens.{input, output, reasoning, cache.read, cache.write}`, `time.created` | Newer OpenCode versions keep messages in SQLite; Caret reads `opencode.db` through the `sqlite3` command in read-only mode and reports when the command is missing. A reported `cost` of 0 falls back to the price table. |

If a directory is missing the agent is shown as not found; if it exists but holds nothing in the range the tables are
empty.

## Commands and settings

| Command | Signature | Returns |
| --- | --- | --- |
| `usage_summary` | `(range: "today" \| "7d" \| "30d" \| "all", groupBy: "model" \| "provider" \| "agent" \| "source" \| "day" \| "workspace")` | `{ totals, rows[], from, to }`; totals and rows carry `input, output, cacheRead, cacheWrite, thinking, costUsd (null when unpriced), requests, unpriced, cacheHitRate` |
| `usage_daily` | `(range)` | `[{ day: "YYYY-MM-DD", costUsd, tokens }]`, zero-filled |
| `usage_recent` | `(limit?)` | newest entries first |
| `usage_export_csv` | `(range)` | CSV text |
| `usage_external` | `(range?)` | `{ agents: [{ id, name, available, sourcePathPattern, totals, byModel[], windows[], note? }], from, to }` |
| `usage_dir` | `()` | the ledger directory |

Settings (`settings.json`, all optional):

```json
"usage": { "track": true, "includeExternal": true, "customPrices": { "my-proxy-model": { "input": 1.0, "output": 4.0, "cacheRead": 0.1 } } }
```

Frontend access goes through `api.usage.*` in `src/lib/api.ts`; the page is `src/components/settings/pages/Usage.tsx`
with its pure helpers in `usageFormat.ts`.

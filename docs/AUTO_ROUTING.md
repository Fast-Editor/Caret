# Auto routing (Lynkr)

**Auto** in the composer's model picker used to mean "the default model". With Auto routing enabled it means:
every request is scored by [Lynkr](https://github.com/Fast-Editor/lynkr) — the open-source LLM router — and sent to
one of **your** configured models, chosen per request by complexity. Nothing about your providers or keys changes:
Lynkr runs as a managed sidecar next to Caret, and Caret executes the routed request itself.

## How it works

```
chat / ⌘K / Bugbot ──▶ Caret agent ──▶ Lynkr sidecar (127.0.0.1)  scores → tier → model
                                         │
                                         ▼ (Caret's loopback "bridge")
                                       Caret provider code ──▶ Anthropic / OpenAI / Azure / Ollama / … with YOUR keys
```

- Lynkr classifies each request into **SIMPLE / MEDIUM / COMPLEX / REASONING** (lexical scoring by default, optional
  embeddings via an Ollama provider), keeps a chat **pinned** to its first pick when *Sticky sessions* is on, and
  **climbs** to the next tier when a model errors or rate-limits.
- Each tier maps to one of the models you enabled in Settings → Models. Caret proposes the mapping automatically and
  you can pin any tier to a specific model.
- The reply's footer shows a pill such as `Auto → Claude Sonnet 5.5 · COMPLEX`; a `↷` marks a fallback (the model
  that answered differs from the router's first choice). Cost is computed with the model that actually answered.

## Enabling it

Settings → Models → **Auto routing** → *Enabled*. The first time, click **Install**: Caret installs a pinned Lynkr
release into `~/.caret/lynkr` with npm (Node 20+ required, no global install, nothing touches an existing Lynkr).
The status line shows `healthy` when the sidecar answers; until then Auto quietly uses the default model and the
picker says why ("router starting…", "router not installed").

Turning it off stops the sidecar. Quitting Caret stops it too.

## How tiers are assigned

Candidates are the enabled models whose provider is enabled and has a key, that support tools, are not excluded and
are not themselves routers (`…/auto`, `lynkr-*`). Price is `input + 3 × output` per million tokens (Ollama = free;
an unknown cloud price is replaced by the median and reported).

| Tier | Rule |
| --- | --- |
| REASONING | best thinking/effort support, then highest price, then largest context |
| COMPLEX | among the rest, highest price (preferring models with thinking) |
| SIMPLE | cheapest, preferring models marked *fast* |
| MEDIUM | the remaining model priced nearest the geometric mean of SIMPLE and COMPLEX |

Three models → MEDIUM reuses COMPLEX; two → SIMPLE is the cheap one and the others share the strong one; one → Auto
routing stays off (it needs at least two). **Overrides** (the dropdown per tier) win over the automatic pick and are
remembered even when the model is temporarily disabled.

Settings in the same section: *Sticky sessions*, *Route review and inline edits too* (Tab, commit messages and apply
always use the fast model directly), *Embeddings for routing* (an Ollama provider), *Tokens-per-minute budget*.

## What Lynkr does and does not do here

On: tier routing, session pins, tier-fallback climbing, circuit breakers, cost/budget tracking, bandit and
de-escalation learning, daily threshold calibration. Off, because Caret already owns them: memory injection,
tool-result compression, prompt cache rewriting, token-budget truncation, request rate limits, the visible
`[Lynkr]` banner. The generated configuration lives in `~/.caret/lynkr/.env`; logs in `~/.caret/lynkr/logs`.

## Security model

- The bridge listens on `127.0.0.1` with a random per-launch token; it only accepts the model ids you configured.
- Provider keys stay in `~/.caret/secrets.json` — the Lynkr configuration contains only the bridge token.
- Lynkr itself binds to all interfaces and does not authenticate requests until the patched release ships (the
  section shows a warning while that is the case). Even then, a LAN peer could only drive the router, not reach
  your keys.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| State `not-installed` | click Install; needs Node 20+ (`node --version`) and network for the first install |
| State `degraded`, "needs at least 2 enabled models" | enable another model with a usable key |
| State `error` | open **Logs**; a port clash or crashed sidecar restarts automatically up to three times, then Restart |
| Pill missing on replies | the chat used an explicit model, or the router was not healthy at that moment |
| Reply is "⚠ [Lynkr] No provider could serve this request (HTTP 502)" | every tier model failed at the provider leg (key quota exhausted, local server down, bad key). Check the models directly from the composer, then Restart |
| All replies land on the same tier and the warnings say "no price configured" | give your models prices in Settings → Models (the tier heuristic orders by price) or pin tiers with the override dropdowns |
| The sidecar install is huge | reinstall: both npm steps now pass `--omit=optional` (≈ 80 MB instead of 600 MB); `npm prune --omit=optional` inside `~/.caret/lynkr` fixes an existing install |
| Every reply lands on one tier | check the tier table — overrides or a single-provider setup can collapse tiers |
| Want the old behaviour | disable Auto routing; Auto is the default model again |

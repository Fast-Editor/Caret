# Configuring AI providers and models

Caret separates **providers** (where requests go: an API endpoint plus credentials) from **models** (what you
pick in the composer: a model id on one of those providers, with its context window, capabilities and prices).
Everything described here configures Caret's **built-in agent**, Tab completions, ⌘K, commit messages and the
code index. External agents run through ACP (Claude Code, Codex, Gemini CLI, OpenCode …) bring their **own**
logins and model lists — see [ACP.md](ACP.md).

Open **Settings → Models** (⌘,). The page has two sections: *Providers* (cards with an enable toggle, key field,
**Fetch models**, add model, edit, remove) and *Models* (a table with the default radio, enabled toggles, the
fast ⚡ marker and prices).

## Quick start

1. **Settings → Models → Anthropic** (or OpenAI / OpenRouter / Gemini): paste the API key into the card.
   Alternatively click **Import API keys from environment** — it reads the variables listed below and stores
   them for you.
2. In the *Models* table choose the **default** model (the radio button). "Auto" in the composer resolves to it.
3. Optional: mark one cheap, fast model for Tab (Settings → Tab → *Tab model*), and set an embeddings provider
   under Settings → Indexing & Docs for semantic `@Codebase` search.

That is enough for chat, agent mode, ⌘K, Tab and commit messages.

## Provider types

| Type (`kind`) | Use it for | Default base URL | Auth header | Notes |
| --- | --- | --- | --- | --- |
| `anthropic` | Claude via the first-party Messages API | api.anthropic.com (leave empty) | `x-api-key` | Adaptive thinking on the 4.6+ family, effort levels, prompt-caching breakpoints, optional **refusal fallbacks** (first-party only, Opus 5.5 / Sonnet 5.5 / Fable 5.1) |
| `anthropic-compatible` | Messages API behind a gateway (Azure AI Foundry, LiteLLM, corporate proxies) | your gateway URL | `x-api-key` or `Bearer` (selectable) | Same request format as `anthropic`; fallbacks are never requested |
| `openai` | GPT models | api.openai.com (leave empty) | `Bearer` | Reasoning models get the `developer` role, `max_completion_tokens` and `reasoning_effort` automatically |
| `azure-openai` | Azure OpenAI deployments | `https://<resource>.openai.azure.com/openai/deployments/<deployment>` | `api-key` | Set **API version** (default `2024-10-21`); the model id is the deployment name |
| `openai-compatible` | Any `/v1/chat/completions` server: LM Studio, vLLM, llama.cpp, SGLang, LiteLLM, routers such as Lynkr | `http://localhost:8080/v1` | `Bearer` (key optional) | `reasoning_effort` is sent when the model is marked *supports effort* |
| `ollama` | Local models through Ollama | `http://localhost:11434/v1` | none | Also usable as the embeddings provider |
| `openrouter` | Many vendors behind one key | `https://openrouter.ai/api/v1` | `Bearer` | Reasoning sent as `reasoning: { effort }`; add `HTTP-Referer` / `X-Title` under *Extra headers* if you want attribution |
| `gemini` | Google Gemini through its OpenAI-compatible endpoint | `https://generativelanguage.googleapis.com/v1beta/openai` | `Bearer` | Use a Google AI Studio key |

Every provider card also accepts **Extra headers** (sent with every request) and an **Auth header** override, which
is how most proxies and gateways are accommodated.

Built-in providers (`anthropic`, `openai`, `openrouter`, `gemini`, `ollama`, `local`) exist on first launch with
sensible defaults; `local` is an OpenAI-compatible placeholder pointing at `http://localhost:8081/v1`. Add as many
more as you like with **Add provider**.

## API keys and where they live

- Keys are **not** stored in `settings.json`. Each provider has an `apiKeyRef` (by default its id) and the key is
  kept under that name in `~/.caret/secrets.json`. Removing the text from the key field deletes the entry.
- **Environment fallback.** When a key is missing from the secrets file Caret reads, per provider id:
  `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`, `GEMINI_API_KEY`, `LOCAL_LLM_API_KEY` (for `local`),
  plus `BRAVE_API_KEY` / `TAVILY_API_KEY` for web search.
- **Import API keys from environment** copies `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`,
  `GEMINI_API_KEY`, `LYNKR_API_KEY` (→ `local`), `BRAVE_API_KEY`, `TAVILY_API_KEY` into the secrets file.
- GUI apps on macOS do not inherit your shell's environment. Either launch Caret from a terminal (`caret .`) so
  the variables are present, or use the import button once.

## Models

Add a model with the **+** on a provider card, or use **Fetch models** to list what the provider reports from its
`/models` endpoint and tick the ones you want. Each model has:

| Field | Meaning |
| --- | --- |
| Model id | exactly what the provider expects (`claude-opus-5-5`, `gpt-5`, `llama3.1:8b`, an Azure deployment name) |
| Display name, tags | what the picker shows; the tag `fast` (or the ⚡ flag) marks Tab-grade models |
| Context window, max output | drive the context ring, the summarize-at threshold and the output cap |
| Supports tools / vision / thinking / effort | gate agent tools, image attachments, the Thinking toggle and the Effort picker |
| Prices ($/M input, output, cache read/write) | used for the per-session cost shown in chat |
| Enabled, Default | only enabled models appear in the picker; exactly one is the default ("Auto") |

How Caret chooses a model when you did not pick one explicitly:

1. **Chat / agent**: the per-chat picker, else `agent.defaultModelId`, else the model marked default.
2. **Tab, commit messages, chat titles and other small requests** (`prefer_fast`): Settings → Tab → *Tab model*,
   else the first enabled model marked fast whose provider has a usable key, else the default model.
3. **Thinking and effort** defaults live in Settings → Agent (*Thinking* auto/on/off, *Effort* low…max) and can be
   overridden per chat from the composer. Anthropic models older than the 4.6 family never receive a `thinking`
   block (Caret does not send `budget_tokens`).

## Embeddings (semantic code search)

Settings → Indexing & Docs → **Embeddings provider**: any provider that serves `/v1/embeddings` (OpenAI, Ollama,
LM Studio, vLLM, OpenRouter). **Embeddings model** examples: `text-embedding-3-small` (OpenAI) or
`nomic-embed-text` (Ollama: `ollama pull nomic-embed-text`). Leave the provider empty to keep keyword (BM25) search
only; *Hybrid weight* mixes the two.

## Web search

Settings → Agent → *Web*: DuckDuckGo needs no key; SearXNG needs an endpoint; Brave and Tavily need the keys
named above.

## Recipes

**Anthropic only.** Paste the key into the Anthropic card. Default = Claude Opus 5.5 (or Sonnet 5.5 for speed),
Tab model = Claude Haiku 4.5 (already flagged fast). Leave *Refusal fallbacks* on unless your policy forbids it.

**Fully local with Ollama.** `ollama pull qwen2.5-coder:7b && ollama pull nomic-embed-text`. On the Ollama card
click **Fetch models**, add the coder model, set it as default and Tab model. Embeddings provider = Ollama, model
`nomic-embed-text`. No keys anywhere.

**LM Studio / vLLM / llama.cpp.** Add provider → type *OpenAI-compatible*, base URL `http://localhost:1234/v1`
(LM Studio), `http://localhost:8000/v1` (vLLM) or `http://localhost:8080/v1` (llama.cpp). Fetch models, enable
one. Keys are optional; if the server wants one, paste it and keep *Authorization: Bearer*.

**Azure OpenAI.** Add provider → *Azure OpenAI*, base URL
`https://<resource>.openai.azure.com/openai/deployments/<deployment>`, API version `2024-10-21`, key in the
card. Add a model whose id is the **deployment name**.

**Claude through Azure AI Foundry or a gateway.** Add provider → *Anthropic-compatible*, base URL = the gateway's
Messages API root, choose the auth header the gateway expects (`x-api-key` or Bearer), add the Claude model ids it
exposes.

**OpenRouter.** Paste the key into the OpenRouter card, Fetch models or keep *OpenRouter Auto*, optionally add
`HTTP-Referer` and `X-Title` as extra headers.

**A router / proxy on another port (e.g. Lynkr on 8081).** Edit the `OpenAI-compatible (custom)` card: base URL
`http://localhost:8081/v1`, key via *Import API keys from environment* (`LYNKR_API_KEY`) or pasted. Add the model
ids the router accepts; mark the cheap one `fast` for Tab.

## The same configuration in `~/.caret/settings.json`

Everything the Settings page writes is plain JSON you can edit or check in (project-level overrides go to
`<repo>/.caret/settings.json`; keys never do):

```jsonc
{
  "providers": [
    { "id": "anthropic", "kind": "anthropic", "name": "Anthropic", "apiKeyRef": "anthropic", "enabled": true },
    { "id": "lmstudio", "kind": "openai-compatible", "name": "LM Studio", "baseUrl": "http://localhost:1234/v1", "enabled": true },
    { "id": "azure", "kind": "azure-openai", "name": "Azure OpenAI", "baseUrl": "https://myres.openai.azure.com/openai/deployments/gpt-5",
      "apiVersion": "2024-10-21", "apiKeyRef": "azure", "enabled": true }
  ],
  "models": [
    { "id": "claude-opus-5-5", "providerId": "anthropic", "model": "claude-opus-5-5", "displayName": "Claude Opus 5.5",
      "contextWindow": 1000000, "maxOutput": 128000, "supportsTools": true, "supportsVision": true, "supportsThinking": true,
      "supportsEffort": true, "inputPricePerM": 4, "outputPricePerM": 20, "enabled": true, "isDefault": true },
    { "id": "qwen-coder", "providerId": "lmstudio", "model": "qwen2.5-coder-7b-instruct", "displayName": "Qwen Coder 7B",
      "contextWindow": 32000, "maxOutput": 4096, "supportsTools": true, "supportsVision": false, "supportsThinking": false,
      "supportsEffort": false, "enabled": true, "isFast": true, "tags": ["fast"] }
  ],
  "tab": { "modelId": "qwen-coder" },
  "agent": { "defaultModelId": "claude-opus-5-5", "thinking": "auto", "effort": "high" },
  "indexing": { "embeddingsProviderId": "ollama", "embeddingsModel": "nomic-embed-text" }
}
```

`~/.caret/secrets.json` is a flat map of `apiKeyRef` → key, for example `{ "anthropic": "sk-ant-…", "azure": "…" }`.

## Troubleshooting

| Symptom | Likely cause / fix |
| --- | --- |
| Picker says *No models enabled* | enable at least one model in the table; the provider must also be enabled |
| 401 / 403 | wrong key, or wrong **Auth header** for a gateway (switch between `x-api-key`, Bearer, `api-key`) |
| Key works in the terminal but not in the app | the app did not inherit your shell env; use *Import API keys from environment* |
| Azure returns 404 | base URL must include `/openai/deployments/<deployment>` and the API version must be set |
| Model listed by *Fetch models* but requests fail | the id must match exactly; some servers require the full `vendor/model` form |
| Thinking toggle missing | the model is not marked *supports thinking*; edit the model |
| Costs show as 0 | fill in the per-million prices on the model |
| Context ring wrong | fix *Context window* on the model; it also controls when old turns get summarized |

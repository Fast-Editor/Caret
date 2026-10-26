# Caret innovation plan v2 — the better list

*Research pass 2, 2026-10-25. Adds direct reads of px0, T3 Code and Antigravity plus nine research sweeps over ~70
adjoining tools (agent workbenches, control planes, terminal harnesses, indie/Asian/legacy editors, mobile,
big-vendor IDEs, 2026 launches). v1 is kept as `INNOVATION_PLAN_v1.md`. Numbers are HN points (pts) or GitHub
stars (★) unless stated; vendor claims are marked [vendor]; unverified items [?].*

## 0. The one-paragraph thesis

The market has split into **six surfaces**: classic IDEs with chat (Cursor, Windsurf), **agent-first IDEs**
(Antigravity, Cursor 3, Copilot app), **review workbenches** (px0: "IDEs are dead, long live the IDE"),
**control planes** that drive other people's agents (T3 Code 30k★, Conductor $22M, Orca 106k★, JetBrains Air),
**terminal harnesses** (Claude Code, Codex, OpenCode 221k★) and **mobile supervision** (Claude Remote Control,
Copilot remote, T3 iOS). Every one of them is converging on the same realisation the data showed in v1:
*the human's job is now verification, steering and memory, not typing*. Caret's unfair position is that it is
the only one of these that can be **all six at once** without a rewrite: a Rust core (fast, sandboxable, runs
local models), a web UI (serve it headless like px0 → browser/iPad/phone), a built-in agent **and** an Agent Client
Protocol client (drive Claude Code/Codex/any of the 46 ACP agents), MIT and bring-your-own-subscription.

## 1. What the newcomers prove

| Tool | What it proves | Number |
|---|---|---|
| **px0** (Go single binary, 1.9k★, daily releases) | Review-first beats edit-first for AI code: launch into a PR worktree, unpushed-commit review, single-hunk staging, 1-click agentic rebase, threads anchored to code/PR ranges with a working-tree snapshot per turn and one-click undo, bring-your-own harness, 20–30 MB RSS, works from a phone | fuzzy over 95k files in 6 ms |
| **T3 Code** (Theo, MIT) | A performant, open, BYO-subscription control plane wins fast: Claude Code/Codex/Cursor/Grok/OpenCode/Antigravity from one desktop/web/iOS/Android UI; one button commit→push→PR; inline diff review before push; stacked/amended PRs; thread guest sharing with live watching; forge abstraction (GitHub/GitLab/GitCafe) | 30.2k★ in ~8 months, 3–4 nightlies/day |
| **Antigravity** (Google) | **Artifacts** (task lists, plans, walkthroughs, screenshots, browser recordings) let you review *outcomes* instead of actions; persistent knowledge base; Turbo vs Strict sandboxed modes; Projects + scheduled messages; SDK with evals. Also proves the risk: Turbo-mode drive wipe, prompt-injection RCE, "bait-and-switch" 2.0 (771 pts) | launch 1,088 pts |
| **Zed Delta / DeltaDB** | Comments anchored to code *as it evolves*, between-commit history, agent conversations stored with the code; "software takes shape in the conversation, not the commit" | 679 + 529 pts |
| **Orca** | Fan one prompt to 5 agents in worktrees and **merge the winner**; Design Mode (click DOM → prompt); annotate AI diffs; mobile companion | 106k★ |
| **Whiteboard** (YC W26) | **AST-aware semantic diff** (tests/docs collapsed, big additions summarised as pseudocode), **decision log** tying trace to requirements, diagram node → jump to code | 424 pts |
| **Conductor** (ex-Melty) | Vendor-neutral orchestration + iOS + remote control is a venture-scale product; also hostage to vendors' plan terms | $22M A |
| **Positron / Theia / Nova ATC** | IDE **exposes itself as an MCP server** to outside agents; every agent prompt user-editable; "agent races" with an AI judge inside a legacy editor | — |
| **Anthropic's own review process** | Risk classifier picks 0/1/2 human reviewers; "did a human read it?" is the wrong question — "which check catches what could go wrong?" is | 30k changes/day |
| **Token-diet wave** | Harness overhead is a product bug: Claude Code sends 33k tokens before reading the prompt vs OpenCode 7k; proxies cut 60–90 % | rtk 84k★, context-mode 32k★ |
| **Knowledge/memory wave** | Codebase graphs and agent memory are the most-starred new category of 2026 | graphify 135k★, claude-mem 103k★ |
| **Sandbox wave** | Sandboxing is moving into the runtime, in Rust | Docker Sandboxes 694 pts, OpenShell 19.9k★, nono |
| **Mitchell Hashimoto's OSC 7501** | A terminal-level "program status" protocol makes an **agent inbox** possible; already in Claude Code 2.1.295 | — |

## 2. Table stakes we must match now (new since v1)

Launch straight into a PR/branch worktree · unpushed-commit review · hunk-level stage/keep/undo · agentic
rebase/merge-conflict resolution · one button commit→push→PR with generated title/body · stacked/draft/amend PRs
· multi-forge (GitHub, GitLab, Gitea, Bitbucket) · thread anchored to a code range · per-turn working-tree snapshot
+ one-click undo · artifacts panel (plan, task list, walkthrough, screenshots, recordings) · program status in
tabs/dock (OSC 7501) · mid-turn steering and prompt queueing · side chats / fork from message · conversation
search · worktree-per-thread with lifecycle hooks · Turbo/auto vs strict permission modes with a classifier ·
BYO subscription for external harnesses · remote control from phone with approve/deny · scheduled tasks/
automations · usage tab with per-thread cost · iOS-simulator/preview/browser pane · "compare models" side by side.
Caret already has the editor-side core; the gaps are mostly in the review and control-plane layers below.

## 3. The list — 22 features in 7 themes

Each entry: **Why** (evidence) · **What** · **Prior art / novelty** · **Caret build** · **Effort** (S ≤ 2 wks, M ≤ 6,
L ≤ 12 engineer-weeks) · **Metric**.

### Theme A — Review is the product

**A1. Verified Diff (proof panel)** — *Why:* review time +91–441 % [vendor], 96 % don't fully trust AI code;
≈25 % of test-passing patches admit counterexamples; 20–25 % of agent hallucinations are statically catchable;
Anthropic: pick the check that catches what can go wrong. *What:* before a diff is shown: diagnostics re-run,
dependency grounding (registry + lockfile; 19.7 % of suggested packages don't exist), hunk-scoped security rules,
tests run, **diff-scoped mutation score**, property tests with shrunk counterexamples, execution-based equivalence
for refactors; a risk tier (0/1/2 reviewers) computed from files touched, blast radius and proof coverage;
**verified facts** vs **agent claims** rendered separately. *Novelty:* Tessl "Verifiers" and Codex "Guardian" gate
PRs server-side; no editor renders proof per hunk. *Build:* `verify/` module on `core/diagnostics`, `core/lsp`,
sandboxed runner, checkpoints. *Effort:* M+M. *Metric:* % changes accepted without rework; review min/hunk.

**A2. Semantic diff + review by concern** — *Why:* "agent diffs are unreadable at scale" (Whiteboard 424 pts;
hunk 9.7k★); reviewers want "what changed and why at several abstraction levels". *What:* tree-sitter
structural diff: collapse tests/docs/moves, summarise large additions as pseudocode, group hunks by the **plan
step/intent** that produced them, entity-level blame; intra-line character diffs (px0). *Novelty:* Whiteboard
has the diff but can't edit; nobody groups by intent. *Build:* `sem`-style Rust structural diff → Monaco
decorations/folding; chat ReviewBar grouping. *Effort:* M. *Metric:* time to first accept; scroll distance.

**A3. Decision log + artifacts** — *Why:* "there is no artifact of the plan aside from the code" (HN); Antigravity
Artifacts praised; 44 % want prior decisions as context. *What:* every turn produces reviewable artifacts: plan,
task list, walkthrough, screenshots/recordings of verification, and a **decision log** linking each decision to
the requirement, the evidence and the hunks; artifacts are versioned files in `.caret/artifacts/` and attach to
the PR. *Novelty:* Antigravity (closed, vendor-hosted); Whiteboard (no edit). *Build:* runner emits artifacts;
PlanTab renders; git notes link. *Effort:* M.

**A4. Comprehension gate ("explain before merge")** — *Why:* 17-point comprehension drop (Anthropic RCT); 621-pt
"shipping code you don't understand" thread; Sphinx quiz hook 253 pts. *What:* optional per-PR gate: Caret
generates a 3-question walkthrough quiz or a guided code tour over the diff; failing it adds a reviewer or blocks
auto-merge; "explain this hunk in terms of my spec" on demand. *Novelty:* community hooks only. *Build:* review
cockpit + LSP code tour. *Effort:* S. *Metric:* opt-in rate; post-merge defects on gated vs ungated.

**A5. Review-on-diff as the feedback channel** — *Why:* Pochi/Orca/Codex browser comments: inline comments on
the agent's diff replace re-explaining in chat; Google resolves 7.5 % of all review comments with one-click
ML edits. *What:* comment on any hunk/line/preview pixel → structured follow-up to the same thread; every bot or
human comment carries a previewable **apply-fix**; per-rule acceptance stats auto-mute noisy bot rules (only
36 % of AI review comments are accepted). *Build:* DiffEditor comment widgets; chat context items. *Effort:* M.

### Theme B — Open control plane

**B1. Any agent, any plan (ACP client + BYO subscriptions)** — *Why:* ACP now has 46 agents / 100+ clients;
T3 Code 30k★ and OpenCode 221k★ on "truly open, BYO"; Air/Zed/Xcode/Android Studio all added "bring your own
agent". *What:* Caret runs its own agent **and** any ACP agent (Claude Code, Codex, Gemini, Copilot, Junie, Kiro,
Qoder, Kimi, Goose…) in the same threads UI, with the same review bar, proof panel, budgets and inbox; one-click
install from the ACP registry; Caret itself is an **ACP agent and an MCP server** (Positron pattern) so other
tools can drive it. *Novelty:* no IDE offers built-in agent + ACP parity with shared review tooling. *Build:*
Rust ACP SDK, map ACP sessions to `ChatSession`. *Effort:* M. *Metric:* % threads on external agents.

**B2. Fleet + Agent Inbox** — *Why:* "10 terminal tabs in different worktrees and trying to remember what each
bit is"; mitchellh: turn off notifications, context switching is expensive; necoder/cmux/tuios inboxes. *What:*
one inbox of agent states (working / waiting on you / done / failed) fed by OSC 7501 from every terminal and by
Caret's own sessions; batched approvals; a **crewmate card** per thread (goal, done, running, next step) read from
state, costing no tokens; dock badge and optional Live-Activity-style phone push. *Build:* `core/pty` OSC 7501,
Agents layout. *Effort:* S–M. *Metric:* approvals/hour handled; time-to-respond to "waiting on you".

**B3. Arena + merge-the-winner** — *Why:* Orca 106k★, Cursor `/best-of-n`, VS Code "Compare models", Nova "agent
races"; CodeMonkeys: selecting over an ensemble beats every member. *What:* fan a task to N agents/models in
isolated worktrees, rank by the proof panel (tests, mutation score, diagnostics, cost), show a 2–3-way semantic
diff, merge the winner or **cherry-pick hunks across candidates**. *Novelty:* hunk-level cross-candidate merge is
unshipped. *Build:* worktree sessions + A2. *Effort:* M.

**B4. Claims ledger (conflict-aware parallelism)** — *Why:* 27.7 % of agent PRs conflict; parallel merge is
"manual, chaotic"; unshipped everywhere. *What:* agents and humans declare files/symbols they intend to touch;
overlaps surface before work starts; continuous rebase previews show semantic conflicts; "ask B to rebase on A".
*Build:* index graph + git2. *Effort:* M. *Metric:* conflicts per parallel run.

**B5. Headless Caret: remote and phone** — *Why:* px0 "remote-first, zero SSH"; T3/Conductor iOS; Claude Remote
Control; Copilot remote GA; "How do you AI code from your phone?" threads. *What:* `caret serve` runs the Rust core
headless on any box and serves the existing web UI over one authenticated port; QR/one-time-link handoff to a
phone PWA for approvals, steering, diffs and the inbox; E2E relay optional. *Novelty:* same codebase as the
desktop app — no second product. *Build:* axum server + auth + WebSocket transport for the IPC layer. *Effort:* L.

**B6. Flight recorder + portable sessions** — *Why:* 91.5 % of misalignment episodes need explicit correction;
portable sessions requested (Claude Code #73639), Codex `/import` is inbound-only; replay tools exist only as OSS
experiments. *What:* record every model response/tool result; replay offline; **fork from step N** with another
model/instruction and diff outcomes; atomic rollback via filesystem snapshots (APFS clonefile/overlayfs; DeltaBox
14 ms); export/import an open JSON-Lines session with hooks/skills manifests. *Build:* response cache + snapshot
backend + Agents layout UI. *Effort:* M.

### Theme C — Codebase brain (memory that belongs to the repo)

**C1. Repo wiki + knowledge graph + memory as one local service** — *Why:* most-starred new category of 2026
(graphify 135k★, codegraph 75k★, claude-mem 103k★); Qoder's editable Repo Wiki; DeepWiki 1.4M repos; "context
management has become a bigger bottleneck than model quality". *What:* Caret maintains, per commit, a
**wiki** (editable, diffable, drift-checked), a **code graph** (defs/refs/callers/impact via tree-sitter+LSP),
and **memories** (project conventions, decisions, failures) and exposes all three to its own agent **and to every
external agent via MCP/ACP**; retrieval is graph-first (LocAgent 92.7 % file-level accuracy) with embeddings
optional. *Novelty:* pieces exist as separate repos; no editor ships the trio as one vendor-neutral service.
*Build:* `index/` already has tantivy + walker; add graph store, wiki generator, memory API, MCP server. *Effort:* L.
*Metric:* tokens per task; "assessment" repeats avoided.

**C2. Provenance and "why" navigation** — *Why:* 79 % want attribution; blame-to-prompt tools (re_gent, Entire
5.2k★); DeltaDB. *What:* prompt→hunk lineage in git notes/trailers; blame hover shows intent, rationale and the
verification that passed; ADR auto-drafted when a PR crosses module boundaries; between-commit history kept
locally. *Build:* `provenance/` + blame decorations. *Effort:* M.

**C3. Intent layer: plans as branchable artifacts, specs with drift detection, editable agent prompts** —
*Why:* "a written plan becomes a debugging surface"; Kiro #9435 (spec↔code drift) open; Theia users love
editing every prompt; Huzzah "prompts are discarded". *What:* plans/specs are first-class files with symbol
anchors, comments, branches and status; on each commit a drift job raises diagnostics on spec, docs and derived
tests; every system prompt, mode and subagent is a visible, editable, versioned file with per-change evals.
*Build:* `ai/agent/plans` + index anchors + settings UI. *Effort:* M.

### Theme D — Context and cost economics

**D1. Cost HUD with estimates and pausing budgets** — *Why:* #1 unshipped request (Claude Code #101248, Cursor
forum "technically hard"); Simon Willison: "default hard budget caps on pretty much everything"; subagents "burned
through my budget before one finished". *What:* pre-run estimate from local telemetry; budgets for $, tokens,
minutes and **scope** (files/lines); exhausting one **pauses at a checkpoint with a hand-off**, never kills;
always-visible per-thread meter (necoder) and a session cost breakdown. *Build:* runner usage + checkpoints.
*Effort:* S–M. *Metric:* $/completed task; budget-pause→resume rate.

**D2. Harness overhead inspector + token diet** — *Why:* 33k vs 7k tokens before reading the prompt (706 pts);
rtk 84k★; tool search cuts 85 % of tool-definition context; cliff compaction +5.7 pts at half the cost. *What:*
show exactly what each turn costs by component (system prompt, rules, MCP tool defs, history, tool outputs);
on-demand MCP tool loading; output pruning via a local 0.6B skimmer; truncate-don't-paraphrase compaction;
trajectory-conditioned routing (cheap first, escalate on verifier failure). *Build:* runner + providers.
*Effort:* M. *Metric:* tokens/turn at equal task success.

### Theme E — Local-first and the trust boundary

**E1. On-device engine + provable offline** — *Why:* JetBrains shipped free offline Junie Local; Sweep 1.5B
next-edit (534 pts), Zeta 2.1 open weights, VS Code on-device MAI Code Flash; Cursor can't (architectural);
air-gap demand. *What:* llama.cpp/mistral.rs in the Rust core for Tab, next-edit, "should I suggest?" and the
skimmer; confidence-gated cloud escalation; an **egress switch** in the single HTTP client so the status bar's
"Offline" is provable. *Build:* `ai/local/`. *Effort:* L. *Metric:* % requests local; p50 latency; zero egress.

**E2. Sandbox by default + risk-tiered permissions** — *Why:* Replit/Antigravity data-loss incidents; "Continue?
Y/N" fatigue (386 pts); under strict policies agents lose 18 pts and cost +167 % by grinding into timeouts;
Claude Code moved to classifier-gated Auto mode. *What:* every agent command runs in Seatbelt/bubblewrap/Landlock
with an egress allowlist; a **shell-AST classifier below the model** labels destructive ops (through `sh -c`,
pipes, `--force`); approvals are risk-tiered and batched, with pre-approved allowlists per repo; "blocked by
policy" is a first-class event the agent sees; MCP tool descriptions pinned by hash; new dependencies grounded
before install. *Build:* Rust sandbox crate (Codex-style), `policy.rs`, `core/pty`. *Effort:* M. *Metric:* zero
destructive incidents; approvals per task.

### Theme F — Runtime and visual grounding

**F1. Debugger- and profiler-grounded agents** — *Why:* debugging is 42 % of the week; 82 % say agents need
runtime context; Visual Studio's `@debugger`/`@profiler` agents are the only shipped examples; GSO shows
optimisation agents are now viable (47 % Opt@1). *What:* agent tools over the debugger (breakpoints, tracepoints,
variables, stacks), the profiler and test traces; "profile → optimise → prove ≥ x % speedup" loop; **recording
debug target** for rr/Undo/Replay/WinDbg-TTD traces with a scrubbable timeline. *Build:* DAP client in Rust +
agent tools; recorder adapters. *Effort:* L.

**F2. Verify in the running app: previews, devices, browser recordings** — *Why:* Xcode agents verifying via
Previews is "the main reliability win"; Android Studio device tools; Claude Code "Auto-verify" + preview servers;
Antigravity browser recordings. *What:* `launch.json`-style preview servers; agent captures screenshots/recordings
as proof artifacts; iOS Simulator/Android device panes; Chrome DevTools-level browser tools; **comment on the
preview** (TraeWork/Codex/Orca) → targeted follow-up; element → source jump (Onlook). *Build:* `browser/` + DevTools
protocol + preview manager. *Effort:* M–L.

**F3. Production-signal gutter + flaky-test repair** — *Why:* Sentry/Datadog/Honeycomb MCPs exist but only as
chat tools; flaky tests waste 150k dev-hours/yr at Atlassian; FlakyGuard repairs 47.6 %. *What:* error rate / p95
/ last exception per function in the gutter from your observability MCP, with "fix with telemetry context";
flaky-test detection from CI history and a repair loop with dynamic call graphs. *Build:* ports + gutter
decorations + CI adapters. *Effort:* M.

**F4. Diagram ↔ code canvas** — *Why:* Whiteboard 424 pts; "always generating diagrams showing what it did";
CodeSee's gap is open. *What:* agents draw sequence/ER/architecture diagrams from the code graph; click a node to
jump; diagrams are artifacts that diff over time. *Build:* react-flow + graph store. *Effort:* M.

### Theme G — Flow

**G1. The "autocomplete gun": local next-edit + whisper steering** — *Why:* "me whispering my train of thought
… and pointing a very fancy autocomplete gun at the code" (HN); Cursor Tab RL: 21 % fewer, 28 % better
suggestions; Sweep 1.5B / Zeta 2.1 weights; voice beats typing only when the transcript is editable. *What:*
on-device next-edit with a "should I suggest?" head; optional push-to-talk whisper (local STT) that biases the
next-edit model and the ⌘K prompt, with the transcript editable; multi-region edits with Tab-to-jump. *Build:* E1 +
`ai/tab.rs` + STT crate. *Effort:* M after E1.

**G2. Semantic terminal** — *Why:* Warp's block model (800k devs) with resentment of forced AI; OSC 133 everywhere.
*What:* OSC 133 blocks, exit-status gutter, "attach block as context", prompt jumping, OSC 7501 status, AST safety
labels (E2), hard AI-off switch. *Build:* `core/pty` + terminal component. *Effort:* S–M.

**G3. Zero-latency review ergonomics** — *Why:* px0's whole pitch (sub-ms start, 6 ms fuzzy over 95k files,
virtualised 400k-line files, 20 MB); JetBrains Air criticised for resource use. *What:* keep Caret's review and
navigation paths off the JS heap: Rust-side structural diff and fuzzy, virtualised diff rendering, lazy Monaco
load, memory budget per surface with a visible meter. *Build:* ongoing performance programme (see
PERFORMANCE.md). *Effort:* continuous. *Metric:* p50 open-diff < 50 ms on 10k-line files; idle < 200 MB total.

**G4. Live review sessions** — *Why:* Zed Delta comments anchored through edits (679 pts); T3 thread guest
sharing with live watching; Bellows "suggestions held until the host accepts"; Tuple's pairing demand. *What:*
share a thread or diff by link; viewers comment and propose hunks that queue until accepted; comments stay
anchored as the agent keeps editing; hash-linked audit trail. *Build:* B5 relay + CRDT-light anchoring. *Effort:* L.

## 4. What we deliberately skip

Cloud-hosted agent fleets (vendor economics, not ours) · design canvases / vibe-builder templates · computer use
beyond browser and simulator · voice *output* · extension marketplace. Each is saturated or off-thesis.

## 5. Roadmap v2

| Horizon | Ship | Rationale |
|---|---|---|
| **H1 · 0–3 mo** | Table stakes in §2 (PR-worktree launch, unpushed review, hunk staging, agentic rebase, artifacts panel, OSC 7501 inbox, side chats, search) · **A1** (diagnostics-in-loop, dependency grounding, security rules, mutation score, risk tier) · **A2** · **A5** · **D1** · **D2** (overhead inspector, tool search, cliff compaction) · **E2** · **G2** · **B1** (ACP client) | Closes the gap to px0/T3/Antigravity and lands the verification thesis; everything builds on existing modules |
| **H2 · 3–6 mo** | **C1** codebase brain (wiki+graph+memory as MCP) · **C2** provenance · **B2** fleet/inbox polish · **B3** arena with hunk cherry-pick · **B4** claims ledger · **B6** flight recorder · **B7** runtime isolation · **B8** durable daemon · **E1** on-device engine + offline switch · **F2** verify-in-app · **A3** decision log/artifacts · **A4** comprehension gate | Differentiators competitors can't copy fast (Rust core, local inference, persisted traces, vendor neutrality) |
| **H3 · 6–12 mo** | **B5** headless/remote/phone · **F1** debugger/profiler/recording agents · **F3** production gutter + flaky repair · **F4** diagram canvas · **C3** intent layer with spec drift · **G1** autocomplete gun · **G4** live review sessions · A1 properties + equivalence at scale · Verus "prove this fn" | Highest ceiling; needs H1–H2 telemetry and infrastructure |

**Targets after H1:** review minutes per hunk −30 %, $/completed task −40 %, 100 % of agent changes carry a proof
panel, zero destructive incidents, idle memory under 250 MB total with a TS file open.

## 7. Addendum — what the builder platforms add (Bolt, Lovable, v0, Base44, Emergent, Replit, Tempo, Subframe)

The vibe-coding builders are a leading indicator for editor UX because they iterate daily on non-expert users:

- **Deterministic, zero-token visual edits** are now standard (Replit Visual Editor, Bolt "Select", Lovable preview
  toolbar, Base44 Edit mode, v0 Design Mode): direct manipulation writes to source without a model call, and hands
  off to the agent only when the change needs reasoning. → fold into **A5/F2**: comment/select/draw on the preview,
  with deterministic edits free and agent edits budgeted.
- **Drafts** (Lovable): per-user isolated copies of a project that merge back, with roles — the multiplayer answer
  to parallel agents. → informs **B4/G4**.
- **Publish-gated security scans** (Lovable Quick/Deep Scan, Base44 scheduled scans, "block publishing with
  critical issues"). → the same gate belongs in **A1** at commit/PR time.
- **Monitoring agents with auto-fix/auto-republish and Routines** (Replit, Oct 2026; Cursor Rollouts): post-ship
  loops are becoming expected. → **F3** should include "watch this deploy and open a fix if the error rate rises".
- **Live-route canvases** (Tempo: every frame is a live iframe of a real route; Subframe; Replit Design Canvas):
  design review happens on the running app, not mockups. → **F2/F4**.
- **Self-testing agents** (Replit "tests its own work", Claude Code "Auto-verify"): verification artifacts by
  default. → **A1/A3** must be on by default, not a mode.
- **Credit burn is the universal complaint**: Trustpilot 2.1–2.8/5 for Bolt, Base44, Emergent and Replit are driven
  almost entirely by surprise spend and loops. This is the strongest external validation of **D1** (estimates +
  pausing budgets) as a top-three priority, and of showing cost per action before it runs.
- **Data-for-compute tiers** (Bolt Forge: open models at 50× usage in exchange for de-identified prompts) and
  in-house models (Base44 Base1) show pricing is a feature; Caret's answer is BYO subscription/keys plus a local
  engine (**E1**), never token resale.

## 8. Addendum — the Chinese ecosystem (TRAE, Qoder, CodeBuddy, Kimi Code, Qwen Code, ZCode, DeepSeek Harness)

Fast-moving and under-watched in the West; several ideas are not in any US tool:

- **Role-based builder teams** (TRAE SOLO Builder: frontend/backend architects, API test engineer, DevOps,
  performance expert, *compliance checker*; Qoder Experts Mode) — a named, inspectable cast instead of anonymous
  subagents. → shape **B3/B4** around named roles with visible responsibilities.
- **"Ask When Needed" mode** (Kimi Code) and Qoder **Fast Mode** (27 % fewer tokens, 3-s first response) — explicit
  interruption budgets and latency tiers. → **D1/D2**.
- **Video as a prompt** (Kimi Code) and **drive the agent from WeChat/DingTalk/Feishu/Telegram** (Qwen Code, ZCode
  Bot Control). → **B5** should accept a screen recording as context and expose a chat-bot channel.
- **Auto-memory and auto-skills** (Qwen Code), **Repo Wiki + Quest Remote** (Qoder), **NPC "AI-native Git"
  teammate with 90 % lower first-round tokens** (CodeBuddy). → confirms **C1/C2** and the token economics of
  **D2**.
- **"Everything is a plugin", let-the-agent-write-the-plugin, and a Claude Code mods compatibility layer**
  (DeepSeek Harness, 261k★ in two months) — compatibility with the dominant harness's ecosystem is a growth
  lever. → Caret should read Claude Code/Cursor/Codex skills, hooks and plugins natively (it already reads
  `.cursor/` and `.claude/skills`).
- **Peak/off-peak metering** (GLM Coding Plan, up to 92 % off-peak) — time-of-day routing is a real cost lever.
  → **D2** routing policy.
- **Trust failures are fatal**: TRAE's telemetry scandal (954 pts) and ZCode's silent workspace upload (342 pts,
  forced open-sourcing) show that **provable zero-egress and open source are features**, not marketing. → **E1**
  egress switch, telemetry-free by construction, MIT.

## 9. Addendum — ideas the 2024-era CLI pioneers left on the table (Aider, Plandex, Continue, Cline, Vibe)

Aider is maintenance-only, Plandex is dormant and Continue was absorbed by Cursor, but their distinctive
mechanics are proven and unclaimed by any desktop editor:

- **Comment triggers** (Aider watch mode): write `// make this async AI!` or `// why does this fail? AI?` in any
  file and the agent acts on save, with the comment as the prompt. → a zero-UI entry point for **G1**; pairs with
  Tab/⌘K.
- **Architect / editor model split**: one model plans the change, a cheaper one writes the edits ("separate models
  for creating code edits and validating them — that's a win"). → formalise in **D2** routing.
- **Cumulative-diff sandbox with per-file reject and plan branches** (Plandex v2): all agent edits accumulate in a
  sandbox you review and apply per file; plans can branch. → **A5/B6**.
- **Autonomy levels as a dial** (None → Basic → Plus → Semi → Full) instead of binary YOLO. → **E2** risk tiers.
- **Source-controlled AI checks in CI** (Continue's 2026 pivot; Tessl Verifiers): the same checks that gate a Caret
  diff should be runnable headless in CI from the repo. → ship **A1** as a CLI too.
- **Compaction must never forget pinned context**: "at the start of the session it reads plan.md; during compaction
  it forgets it" (HN, Oct 2026). → **D2** cliff compaction pins plan/spec/rules by construction.
- **"Discuss before apply"**: users want a mode that talks about a change without being pushed to accept it. →
  keep Ask/Plan modes first-class, with "apply" an explicit step (already Caret's model; keep it).

## 10. Addendum — the agent-workbench category up close (Conductor, Superset, Emdash, Vibe Kanban, Sculptor, HumanLayer, Zenflow, Agent Orchestrator, cmux/Orca/Corgi, Devin Desktop, Terragon…)

**The category's graveyard is the lesson.** Within ~12 months Terragon, Vibe Kanban (Bloop), Crystal, Ona, Stage
and the standalone Codex app died or were folded. Free-only tools starve; Mac-only closed tools (Conductor) leave
the door open; Electron footprint ("2 GB is too much") and mandatory accounts are the most repeated complaints.
Users cap out at 2–3 agents for real work. → Caret: open-source cross-platform core, no account, native footprint,
paid cloud/team runners later.

**Ideas worth taking (one tool each):**
- **Kanban lanes derived from state, not drag-and-drop** (Agent Orchestrator: Working / Needs you / In review /
  Ready to merge computed from session + PR + CI). → **B2** inbox lanes.
- **Logically ordered hunks** (Devin Review) and **review "chapters"** (Stage). → **A2**.
- **Cross-agent review pipeline** (Zenflow: one model plans, another builds, a third reviews). → **A1/B3** default
  "second-model adversarial review" before a diff is presented.
- **Follow the agent** (Zed: hold ⌘ to ride along the agent's cursor) and **Pairing Mode** (Sculptor: bidirectional
  sync between the agent's container and your local checkout). → new **G5 Follow & pair**.
- **Handoff** (Amp: goal-scoped context transfer instead of compaction). → **D2**.
- **Checkpoints bound to commits** (Entire: transcript + prompt + tokens per commit). → **C2**.
- **Worktree reserves** (Emdash pre-creates warm worktrees) and **shared build images** (Conductor). → **B7**.
- **Issue board → one run per task with "proof of work"** (OpenAI Symphony on Linear). → **A3**.
- **`/grill-me`** (Antigravity: the agent interrogates your requirements before building). → Plan mode default step.

**Gaps nobody solves — two new plan items:**
- **B7. Runtime isolation primitives.** "I would need ten different copies of my database"; shared migrations,
  ports and secrets break parallel agents long before git does. Caret: per-worktree environment templating
  (ports, env files, DB branch/schema per worktree via templates for Postgres/SQLite/Docker), optional container
  per thread, teardown hooks. *Effort:* M. *Metric:* parallel runs without environment collisions.
- **B8. Durable local daemon.** Sessions must outlive the editor window: the Rust core runs as a daemon; the
  desktop UI, the browser UI (B5) and the phone are clients; agents keep working when you close the laptop lid
  and reattach anywhere. Status comes from protocol (OSC 7501/ACP), never from scraping terminal text (which
  broke ccmanager when Claude's UI changed). *Effort:* M (the IPC layer already separates core and UI).
- **Harness rotation on rate limits** (weekly caps "burned through in 15 minutes"): when one provider throttles,
  continue the same thread on another configured harness/model with the handoff doc. → **D2**.

## 11. Sources (added in pass 2; v1 sources remain in INNOVATION_PLAN_v1.md)

px0: https://px0.ai/ · https://px0.ai/blog · https://px0.ai/changelog · https://github.com/px0-ai/px0 — T3 Code: https://t3.codes · https://github.com/pingdotgg/t3code · https://github.com/pingdotgg/t3code/releases — Antigravity: https://antigravity.google/ · https://antigravity.google/blog · https://en.wikipedia.org/wiki/Google_Antigravity · https://news.ycombinator.com/item?id=45967814 — Zed: https://zed.dev/blog/introducing-delta · https://zed.dev/blog/introducing-deltadb · https://zed.dev/blog/parallel-agents — Orca: https://github.com/stablyai/orca — Whiteboard: https://github.com/devdotfast/whiteboard · https://news.ycombinator.com/item?id=49833867 — Conductor: https://www.conductor.build/changelog · https://www.conductor.build/blog/series-a — Anthropic review: https://claude.dev/blog/solving-code-review-at-anthropic/ — OSC 7501: https://mitchellh.com/writing/program-status-osc7501 — token overhead: https://systima.ai/blog/claude-code-vs-opencode-token-overhead · https://github.com/rtk-ai/rtk — knowledge/memory: https://github.com/Graphify-Labs/graphify · https://github.com/thedotmack/claude-mem · https://qoder.com/blog/quest-mode · https://cognition.com/blog/deepwiki — sandboxes: https://github.com/NVIDIA/OpenShell · https://github.com/nolabs-ai/nono — ACP: https://agentclientprotocol.com/get-started/agents · https://agentclientprotocol.com/get-started/clients — Positron: https://positron.posit.co/assistant.html — Theia: https://theia-ide.org/docs/user_ai/ — Nova ATC: https://extensions.panic.com/?q=ai — Pochi: https://docs.getpochi.com — Tessl: https://tessl.io/blog — Claude Code desktop: https://code.claude.com/docs/en/desktop · https://code.claude.com/docs/en/remote-control — Codex: https://openai.com/index/codex-for-almost-everything/ — Copilot: https://github.blog/changelog/2026-06-17-github-copilot-app-generally-available · https://code.visualstudio.com/updates/v1_143 — Visual Studio agents: https://learn.microsoft.com/en-us/visualstudio/ide/copilot-specialized-agents — Xcode: https://www.apple.com/newsroom/2026/06/apple-aids-app-development-with-new-intelligence-frameworks-and-advanced-tools/ — Android Studio: https://developer.android.com/studio/gemini/agent-mode — JetBrains Air/Junie: https://www.jetbrains.com/air/ · https://junie.jetbrains.com/ — Simon Willison budgets: https://simonwillison.net/2026/Oct/3/default-hard-budget-caps/ — HN threads: 50151036, 48771515, 47320614, 49799287, 50132963 (https://news.ycombinator.com/item?id=…)

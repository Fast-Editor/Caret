# Caret innovation plan — features that solve measured problems and that nobody ships yet

*Research date: 2026-10-25. Four research passes (competitive landscape, developer pain-point data, 2025–26
research frontier, adjacent-tool innovations) feed this plan; sources are linked inline and collected at the end.
Vendor telemetry is marked [vendor]; peer-reviewed / first-party numbers are preferred.*

## 1. What the evidence says

**Authoring is no longer the bottleneck — verification, comprehension and coordination are.**

| Signal | Number | Source |
|---|---|---|
| Review time on AI-heavy teams | +91 % (2025) → median time-in-review +441 % (2026); 31 % of PRs merged with no review | Faros 2025/2026 [vendor] |
| Trust | 96 % don't fully trust AI code; 48 % trust it *only when easily verifiable*; 6.6 % for important decisions | Sonar Jan 2026; Stack Overflow 2026 |
| What developers want | 79 % rate source attribution important; 44 % want *prior decisions and rationale* as context; 82 % of leaders say agents would debug far better with runtime context | SO 2026; Undo 2026 [vendor] |
| Debugging | 42 % of the week (16.9 h) vs 25 % writing code; 45 % say debugging AI code takes longer | Undo 2026 [vendor]; SO 2025 |
| Comprehension debt | Developers using AI scored 17 points lower on a quiz about code they had just written; largest gap on debugging | Anthropic RCT Jan 2026 |
| Security | 45 % of generated samples carry OWASP Top-10 flaws; 19.7 % of suggested packages don't exist ("slopsquatting") | Veracode 2025; Spracklen et al. USENIX Sec 2025 |
| Cost | Metered-pricing backlash (Cursor Jun–Jul 2025); 23 % spend $200–500/dev/month; "pre-run estimate + budget that pauses" is the most-requested unshipped feature | TechCrunch; Gartner; Claude Code #101248, Cursor forum 160376 |
| Supervision | Only 0–20 % of work can be fully delegated; engineers describe their job as "managing Claudes"; two production-destroying agent incidents (Replit, Antigravity) | Anthropic Dec 2025/Apr 2026 |
| Context | Accuracy degrades with input length even with perfect retrieval (13.9–85 %); faithful truncation beats paraphrasing compaction (76.7 % vs 71.0 %) at 50 % lower cost | EMNLP 2025; CliffCompaction 2026 |
| Parallel agents | 27.7 % of agent PRs hit merge conflicts; no tool coordinates agents beyond worktree isolation | AgenticFlict 2026 |

**Table stakes in Oct 2026 (do not try to differentiate here):** agent command-center windows, worktree
parallelism, best-of-N across models, coordinator→subagents, plan/goal modes, hooks, skills, AGENTS.md,
plugin marketplaces, PR review bots, cloud/background agents, Slack/GitHub triggers, phone control,
computer use, voice input, design canvases, memories, model routers. Caret already covers the editor-side set.

**Requested or hinted but shipped by nobody (verified against changelogs and issue trackers):**
pre-run cost/time estimates with budgets that *pause*; portable/resumable sessions across machines and agents;
fully offline agent + automatic local↔cloud routing in a leading editor; spec↔code drift detection;
conflict-aware coordination of parallel agents on one repo; deterministic replay / fork-at-step of an agent run;
atomic rollback across multi-model runs; live human+agent CRDT editing; portable hooks/skills packages.

## 2. Design principles

1. **Evidence over claims.** Every AI output is paired with something machine-checked (diagnostics, tests,
   mutation score, counterexample, registry lookup, sandbox trace). Agent self-reports are shown as *claims*.
2. **Bounded by construction.** Cost, scope, blast radius and context are budgets the user sets before a run;
   exhausting one pauses with a hand-off, never silently degrades.
3. **Local first, cloud when worth it.** The Rust core can run small models and all verification locally; the
   cloud is an escalation, and "offline" is provable (egress switch in the HTTP layer).
4. **Memory of why.** Intent, rationale and provenance are first-class artifacts that outlive the chat.
5. **No new modality for its own sake.** Voice/design/computer-use are saturated; we invest where the data
   says the hours go.

## 3. The features (ranked by developer value × evidence × feasibility on Caret)

### F1. Verified Diff — a proof panel for every agent change
**Problem.** Reviewers can't tell what an agent actually checked; "tests pass" is weak (≈25 % of test-passing
patches admit counterexamples — SWE-Proof 2026); 20–25 % of agent hallucinations are statically catchable
before review (JetBrains 2026); 45 % of generated code has OWASP flaws; slopsquatted dependencies.
**What.** Before a diff is shown, Caret runs a local verification pipeline and renders a *proof panel* next to
the review bar: (a) diagnostics-in-the-loop (LSP/type/lint re-run on touched files, fed back to the agent and
shown as fixed/outstanding), (b) dependency grounding (every new import/package resolved against the registry
+ lockfile; unknown → blocked, CVE → flagged), (c) security scan of changed hunks (Semgrep-style rules in Rust),
(d) test evidence: which tests ran, diff-scoped **mutation score** (mutate the hunk, count killed mutants — Meta
ACH: 73 % of such tests accepted), (e) property-based "harden this change" (generate properties, run
Hypothesis/proptest/fast-check, show the *shrunk counterexample* inline — PGS: +13.4 % pass@1), (f) for
refactors, execution-based equivalence (differential tests), never LLM-asserted equivalence (models endorse
≈1 in 3 semantic drifts). The panel separates **verified facts** from **agent claims**.
**Novelty.** No editor renders a per-change trust surface; Bugbot/Copilot review are comments; Amp's "proof"
is a vendor-side video. **Caret fit.** `core/diagnostics`, `core/lsp` relay, `ai/agent/checkpoints`, sandboxed
`run_terminal_cmd`, new `verify/` module (mutation runner per language, registry client, rule engine).
**Effort.** M (6–8 weeks for a/b/c/d; e/f +4). **Metric.** % of agent changes accepted without re-work;
review time per hunk; post-merge defects.

### F2. Provenance ledger + "Why" navigation
**Problem.** 79 % want attribution; 44 % want prior decisions and rationale; comprehension gap of 17 points;
knowledge of *why* evaporates with the chat.
**What.** Every accepted hunk carries lineage: prompt, model, tool trace, verification results, session id —
stored as git notes/commit trailers (compatible with the emerging `git why` / Entire conventions). Blame hover
shows *intent and rationale*, not just the author. "Why is this here?" asks the ledger first, the model second.
When a PR crosses module boundaries Caret drafts an ADR from the session. A repo wiki (DeepWiki-style) is
regenerated per commit with Swimm-style **drift diagnostics** on save, and used as agent context.
**Novelty.** Pieces exist as CLIs/prototypes; no editor persists prompt→hunk lineage or surfaces it in blame.
Codellaborator (CHI 2025) showed provenance highlights aid attribution but were never persisted.
**Caret fit.** `ai/agent/session` already has tool traces + pending changes; add `provenance/` (git notes writer,
ledger index in tantivy), blame decoration source, wiki generator job. **Effort.** M. **Metric.** time-to-first-PR
for newcomers; "why" queries answered from ledger without a model call.

### F3. Budgeted, estimable agents (pause-not-kill) with faithful compaction
**Problem.** #1 unshipped request; spend anxiety; compaction loses context and burns tokens.
**What.** Pre-run **estimate** (tokens, $, wall-time, files likely touched) from Caret's own session telemetry
(tool-call distributions for similar prompts in this repo) shown in the composer; per-run **budgets** ($,
tokens, minutes, max files/lines changed — scope budgets, since agents "fail to constrain scope"); reaching a
budget **pauses at a checkpoint with a hand-off summary** and a resume button, never aborts mid-edit.
**Trajectory-conditioned routing** (cheap model first, escalate on verifier failure — 40–70 % cost cuts in
SWE-Router/AgentRouter) and **cliff compaction** (truncate/drop, never paraphrase; +5.7 pts at half the
cost) plus **tool search** for MCP (85 % context reduction).
**Novelty.** Only headless `--max-budget-usd` exists anywhere; Cursor staff called estimates "technically hard".
**Caret fit.** runner already tracks per-call usage, checkpoints, summarize; add estimator from session store,
budget gate in `manager.rs`, replace summarize with cliff policy, BM25 tool index for MCP defs. **Effort.** S–M.
**Metric.** $/completed task, context tokens per turn, pause→resume rate.

### F4. Local-first hybrid intelligence (provably offline)
**Problem.** Closed leaders can't run offline (Cursor: "architectural limitation"); privacy/air-gap demand;
latency. Open weights now exist: Zed Zeta2.1 (Apache-2.0, LSP-context next-edit, p50 136 ms), JetBrains
Mellum-4B; a FIM-trained 3B beats an 80B agentic model at infilling (SynConfRoute 2026).
**What.** Ship an on-device engine in the Rust core (llama.cpp / mistral.rs, Metal on Apple Silicon) for Tab,
next-edit and the "should I suggest?" head (Cursor's RL result: 21 % fewer, 28 % better suggestions);
confidence-gated escalation to the cloud model; an **egress switch** in the single HTTP client so "Offline"
in the status bar is provable, with per-feature routing and a live cost/latency meter.
**Novelty.** No leading editor offers automatic local↔cloud routing (HydraFusion hybrid unconfirmed) or a
provable offline mode. **Caret fit.** `ai/providers` + new `ai/local/` (GGUF loader, FIM/NES prompt formats,
speculative edit decoding), `ai/tab.rs` policy head. **Effort.** L (model integration + eval harness).
**Metric.** Tab acceptance, p50 latency, % requests served locally, zero egress in offline mode.

### F5. Agent Flight Recorder — deterministic replay, fork-at-step, portable sessions
**Problem.** Agents can't be debugged; 91.5 % of misalignment episodes need explicit user correction; sessions
can't move between machines/agents (Claude Code #73639; Codex `/import` is inbound-only).
**What.** Record every model response and tool result with content hashes; **replay** a run offline (no API
cost) to inspect decisions; **fork from step N** with a different instruction or model and diff the outcomes;
**atomic rollback** of multi-run experiments via filesystem snapshots (APFS clonefile / overlayfs; DeltaBox
shows 14 ms checkpoint / 5 ms rollback) layered on git worktrees; **export/import** in an open JSON-Lines
session format with hooks/skills manifests (the portable-package gap).
**Novelty.** Only experimental OSS (agent-timetravel, AgentReplay); no editor. **Caret fit.** transcript +
checkpoints already persisted; add response cache keyed by request hash, snapshot backend, fork UI in the
Agents layout. **Effort.** M. **Metric.** % runs debugged by replay rather than re-run; import success rate.

### F6. Conflict-aware parallel agents ("Claims")
**Problem.** 27.7 % of agent PRs conflict; merging parallel agent work is "manual, chaotic"; nothing coordinates
beyond worktree isolation.
**What.** A shared **claims ledger** per repo: each agent (and human) declares the files/symbols it intends to
touch (from its plan and live edits), checked against the code graph; overlapping claims raise an early
warning, suggest a split, or serialise; a **merge preview** rebases worktrees continuously and shows semantic
conflicts (same symbol edited twice) before anyone finishes; one-click "ask agent B to rebase on A".
**Novelty.** Unshipped (only community prototypes). **Caret fit.** worktree sessions, index graph
(tree-sitter/LSP refs), git2 for rebase previews. **Effort.** M. **Metric.** conflict rate across parallel runs.

### F7. Runtime-grounded debugging — time travel for agents
**Problem.** Debugging is 42 % of the week; 82 % say agents need runtime context; agents localise only 11 % of
errors from raw traces (TRAIL) but do well with structured state.
**What.** (a) **Recording debug target**: attach rr / Undo / Replay / WinDbg-TTD recordings with a scrubbable
timeline and agent tools for reverse-dataflow queries ("where did this value come from?"); (b) a lightweight
built-in recorder for everything else: OpenTelemetry spans + structured logs + test-run traces captured by the
Rust core and shown as a waterfall keyed to source lines; (c) **production-signal gutter** (error rate, p95,
last exception per function) from Sentry/Datadog/Honeycomb MCP with "fix with telemetry context"; (d) **flaky
test repair** from CI history + dynamic call graph (FlakyGuard: 47.6 % repaired, 51.8 % accepted); (e) **replay
the last real request** against the handler you are editing (Darklang/Keploy pattern).
**Novelty.** Time-travel exists only as MCP add-ons; no editor has a native timeline or signal gutter.
**Caret fit.** `browser/`-style window for timelines, `core/pty` for recorders, ports for MCP, agent tools.
**Effort.** L (phase b/c/d first, a last). **Metric.** time-to-root-cause; % fixes made with runtime context.

### F8. Spec ↔ code drift and living intent
**Problem.** Kiro #9435 open; "living specs" are marketing; docs drift (56 % say keeping docs current is the
biggest challenge; stale docs now poison agents).
**What.** Caret plans/specs link to code regions (symbol anchors, not line numbers); on each commit a drift
job (deterministic symbol mapping + LLM check of changed regions) raises **drift diagnostics** on the spec, the
doc and the test that derive from it; properties/tests derived from spec clauses stay traceable; "update the
spec from the code" and "update the code to the spec" are one-click agent tasks with the proof panel attached.
**Novelty.** Unshipped. **Caret fit.** `ai/agent/plans`, index, diagnostics store. **Effort.** M.

### F9. Semantic terminal with a safety AST
**Problem.** Agents run commands you can't scan; destructive-command incidents; Warp proved demand for blocks
(800k devs) but users resent forced AI.
**What.** OSC 133 **blocks** with exit-status gutter, "attach block as context", prompt jumping; every agent
command is parsed into a shell AST **below the model**: destructive operations are labelled in the approval
card (`rm -rf` through `sh -c` wrappers, `git push --force`, pipes to shells), the denylist is enforced
deterministically, and "blocked by policy" is a first-class event the agent can see (agents otherwise grind
into timeouts — Permission Denied 2026); OSC 7501 program-status so tabs show working / waiting / done; a hard
AI-off switch for the terminal.
**Novelty.** No editor terminal has blocks or AST-level safety. **Caret fit.** `core/pty` already tracks OSC 0/2/7;
add 133/7501 parsing, a shell parser crate, policy hook. **Effort.** S–M.

### F10. Review cockpit — review by concern, not by file
**Problem.** Review is the bottleneck; AI review comments are accepted only 36 % of the time; Google's
one-click ML-suggested edits resolve 7.5 % of all review comments.
**What.** Agent diffs are grouped by **concern** (the plan step / intent that produced them) with the proof
panel per concern; every comment (human or bot) carries a previewable **apply-fix** diff; bot comments are
triaged with per-rule acceptance stats that auto-mute noisy rules; **edit-likelihood highlighting** (TOCHI 2025:
highlighting tokens likely to need editing speeds review; low-probability highlighting does not) trained on
Caret's own accept/edit telemetry; an agent-authored **code tour** per PR (Trailblazer/CodeTour) walks the
reviewer through the change; stacked-diff awareness in the SCM panel.
**Novelty.** Per-concern grouping, apply-fix on every comment and edit-likelihood surfaces are unshipped.
**Caret fit.** chat review bar, `editor.openDiff`, SCM view, telemetry store. **Effort.** M.

### F11. Sandbox-by-default and supply-chain gates (platform)
OS-level sandbox for every agent command (Seatbelt on macOS, bubblewrap+Landlock on Linux, restricted tokens on
Windows) with an egress allowlist proxy; MCP tool descriptions pinned by hash and diffed on change (prompt-
injection vectors hit Cursor hardest in the 2026 study); dependency installs only after registry grounding.
**Effort.** M. **Metric.** zero destructive incidents; blocked-event rate.

### F12. Graph-native agent tools and faithful context (platform)
Expose the code graph (defs/refs/callers/impact via tree-sitter + LSP) as structured tools (`refs`, `traverse`,
`impact`, NL search); structured low-level tools raise cross-run consistency up to 4.7× and give 11 % more access
to relevant files; a local 0.6B skimmer prunes 23–54 % of agent tokens without hurting success (SWE-Pruner).
**Effort.** S–M (index + LSP already exist). **Metric.** tokens/turn, tool-call count, task success.

## 4. Roadmap

| Horizon | Deliverables | Why first |
|---|---|---|
| **H1 · 0–3 months** | F1 a–d (diagnostics-in-loop, dependency grounding, security scan, mutation score), F3 (estimates, budgets that pause, cliff compaction, tool search), F9 (semantic terminal + safety AST), F11 (sandbox default), F12 (graph tools), F10 v1 (per-concern review + apply-fix) | Directly attacks the measured bottleneck (review/verification) and the #1 unshipped request; all build on existing Caret modules |
| **H2 · 3–6 months** | F2 (provenance ledger, blame-why, wiki + drift), F4 (on-device Tab/NES + offline switch), F5 (flight recorder, fork, portable sessions), F6 (claims ledger), F7 b–d (built-in recorder, production gutter, flaky repair) | Differentiators no competitor can copy quickly (they lack a Rust core / local engine / persisted traces) |
| **H3 · 6–12 months** | F7 a (rr/Undo timeline), F8 (spec↔code drift), F1 e–f at scale (properties + differential equivalence), best-of-N with verifiers + snapshot rollback, Verus-backed "prove this function" for Rust, F10 edit-likelihood model | Highest ceiling; needs the telemetry and infrastructure from H1–H2 |

**What we deliberately do not build:** cloud-hosted agents, voice output, design canvas, computer use,
extension marketplace. They are saturated and don't move the measured numbers.

## 5. How we'll know it worked

Per-repo dashboard (local, private): review time per hunk, % changes with proof, mutation score on AI-written
tests, $/completed task, tokens/turn, local-vs-cloud ratio, conflicts per parallel run, time-to-root-cause,
"why" questions answered from the ledger. Targets for H1: review time −30 %, $/task −40 %, zero destructive
incidents, 100 % of agent changes shipped with a proof panel.

## Sources (selection)

Pain points: Faros https://www.faros.ai/research/ai-acceleration-whiplash · DORA 2025 https://cloud.google.com/blog/products/ai-machine-learning/announcing-the-2025-dora-report · Sonar https://www.sonarsource.com/company/press-releases/sonar-data-reveals-critical-verification-gap-in-ai-coding/ · Stack Overflow 2026 https://survey.stackoverflow.co/2026/ai/data · Undo 2026 https://undo.io/research-report-2026/ · Anthropic skills RCT https://www.anthropic.com/research/AI-assistance-coding-skills · Veracode https://www.veracode.com/resources/analyst-reports/2025-genai-code-security-report/ · Spracklen et al. (package hallucination) https://arxiv.org/abs/2406.10279 · Linux Foundation Oct 2026 https://www.linuxfoundation.org/press/new-lf-research-survey-shows-generative-ai-cuts-open-source-coding-time-in-half-but-25-of-incoming-pull-requests-are-considered-ai-slop · Cursor pricing https://techcrunch.com/2025/07/07/cursor-apologizes-for-unclear-pricing-changes-that-upset-users/ · Atlassian flaky tests https://www.atlassian.com/blog/how-we-build/taming-test-flakiness-how-we-built-a-scalable-tool-to-detect-and-manage-flaky-tests

Landscape: Cursor 3.0 https://cursor.com/changelog/3-0 · Zed parallel agents https://zed.dev/blog/parallel-agents · Copilot HydraFusion https://github.blog/changelog/2026-09-30-hydrafusion-in-vs-code-and-the-github-copilot-app/ · Kiro changelog https://kiro.dev/changelog/ · Claude Code changelog https://code.claude.com/docs/en/changelog · budget request https://github.com/anthropics/claude-code/issues/101248 · portable sessions https://github.com/anthropics/claude-code/issues/73639 · Cursor estimate thread https://forum.cursor.com/t/pre-execution-cost-estimate-confirmation-for-expensive-operations/160376 · Kiro drift issue https://github.com/kirodotdev/Kiro/issues/9435 · AgenticFlict https://arxiv.org/html/2604.03551v1 · ACP RFDs https://agentclientprotocol.com/rfds/session-fork

Research: SWE-Proof https://arxiv.org/abs/2609.21190 · Property-Generated Solver https://arxiv.org/abs/2506.18315 · Meta ACH https://arxiv.org/abs/2501.12862 · EquiBench https://arxiv.org/abs/2502.12466 · LocAgent https://arxiv.org/abs/2503.09089 · Devil in the Interface https://arxiv.org/abs/2608.11386 · SWE-Pruner https://arxiv.org/abs/2601.16746 · CliffCompaction https://arxiv.org/abs/2609.26779 · Anthropic tool search https://www.anthropic.com/engineering/advanced-tool-use · SWE-Router https://arxiv.org/abs/2607.00053 · DeltaBox https://arxiv.org/abs/2605.22781 · Codellaborator https://dl.acm.org/doi/10.1145/3706598.3713357 · Uncertainty highlighting https://arxiv.org/abs/2302.07248 · How Coding Agents Fail Their Users https://arxiv.org/abs/2605.29442 · TRAIL https://arxiv.org/abs/2505.08638 · FlakyGuard https://arxiv.org/abs/2511.14002 · JetBrains IDE-catchable errors https://blog.jetbrains.com/ai/2026/05/stop-sending-ide-catchable-ai-code-errors-to-review/ · Zeta2.1 https://zed.dev/blog/zeta2-1 · Cursor Tab RL https://cursor.com/blog/tab-rl · Prompt injection in AI dev tools https://arxiv.org/abs/2603.21642 · Permission Denied https://arxiv.org/abs/2608.02670

Adjacent tools: Warp block model https://www.warp.dev/blog/block-model-behind-warps-agentic-development-environment · OSC 133 https://vtdn.dev/docs/osc/osc133/ · Google Critique ML edits https://research.google/blog/resolving-code-review-comments-with-ml/ · Pernosco/rr https://robert.ocallahan.org/2024/10/debt-workshop.html · Undo MCP https://undo.io/products/live-recorder/ · DeepWiki https://cognition.com/blog/deepwiki · Swimm https://swimm.io/blog/sync-dont-sink-why-we-built-swimm-for-dev-teams · Entire CLI https://github.com/entireio/cli · Keploy https://keploy.io/compare/speedscale · Renovate merge confidence https://docs.renovatebot.com/merge-confidence/

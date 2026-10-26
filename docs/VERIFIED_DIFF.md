# Verified Diff (proof panel engine)

Every agent turn that edits files is followed by an automatic verification run. The report is
facts first, model claims second: the engine never asks a model whether the change is correct.

## Pipeline

```
scope ─► checks (sequential, one run at a time per app) ─► risk tier ─► facts vs. claims ─► report
```

**Scope kinds** (`VerifyScope.kind`): `session` (pending changes of a chat, optionally a subset of
`changeIds`), `change-set`, `staged`, `working-tree`, `branch` (vs `base`, default `main`/`master`),
`files`. Each touched file carries before/after text, the added/removed lines and a unified diff.

**Checks** (`VerifyCheck.kind`, in order; each has `status` pending|running|pass|warn|fail|skipped|error,
`summary`, `findings[]`, `durationMs`, optional `command`/`output`/`metrics`):

| kind | what it proves | metrics | fails when |
|---|---|---|---|
| `diagnostics` | language-server diagnostics on touched files (waits ~1s for re-validation) | – | any error-severity diagnostic |
| `project-check` | whole-project type/compile check: `tsc --noEmit`, `cargo check`, `ruff`/`pyflakes`, `go vet` (or `verify.projectCheckCommand`) | `errors_touched`, `errors_elsewhere` | errors in touched files (pre-existing errors elsewhere → warn) |
| `dependencies` | every new package exists on npm/crates.io/PyPI/Go proxy, is in the lockfile, is not a near-miss of a popular name, has no OSV advisories | `deps`, `resolved`, `missing`, `vulnerable` | package not found (critical) or near-miss (high) |
| `security` | ~55 regex rules over added lines (secrets, injection, TLS off, weak crypto, CORS, CSRF, path traversal, curl\|sh …) + semgrep if installed | `lines_scanned`, `critical`, `high`, `medium` | critical/high finding |
| `tests` | runs the detected test command (`npm/pnpm/yarn/bun test`, `cargo test`, `pytest`, `go test`, or `verify.testCommand`) and parses the summary | `passed`, `failed`, `total` | any failure / non-zero exit |
| `mutation` | diff-scoped mutation score: mutates only added lines (≤ `verify.mutationMaxMutants`), re-runs tests, restores files (Drop guard; never clobbers user edits) | `mutants`, `killed`, `survived`, `score` | score < 0.3 (warn < 0.6) |
| `lint` | eslint / ruff / clippy on touched files (60 s cap, warn-only) | – | never (warn) |

Finding severities: `critical` > `high` > `medium` > `low` > `info`. Findings carry `path`, `line`,
`rule`, `suggestion`, `evidence`, and `fixable` when `verify_apply_fix` can rewrite the line.

**Risk tier** (`report.riskTier` 0/1/2 with `riskReasons`): 2 when protected paths (auth, payments,
migrations, CI, infra, lockfiles with code changes) are touched, a dependency is missing, or
critical security findings exist; 1 for large diffs, failed tests, new dependencies; 0 otherwise.

**Facts vs. claims**: `report.facts` are sentences the checks proved ("12 tests pass", "0 type errors");
`report.claims` are assertions extracted from the agent's final message ("I tested…", "this is safe")
so the UI can show which claims have no supporting fact.

## Commands and events

| command | purpose |
|---|---|
| `verify_run(scope, onEvent)` → reportId | start a run; streams `VerifyEvent` (`report`, `check`, `done`, `error`) on the channel |
| `verify_get(reportId)` | full report |
| `verify_list(sessionId?, limit)` | recent reports for the workspace (newest first) |
| `verify_latest(sessionId)` | the session's last report |
| `verify_cancel(reportId)` | cancel a running report |
| `verify_apply_fix(reportId, findingId)` | apply a rule's safe rewrite to the finding's line (goes through the session change registry when the session is loaded) |

Events: `caret://verify/changed` (`{ reportId, sessionId? }`) after every run, and
`AgentEvent::Verification { sessionId, report }` on the chat channel so the proof panel updates in
place. `ChatSessionView.lastVerificationId` points at the newest report.

Reports persist under `~/.caret/verify/<workspace-hash>/<report-id>.json` (index keeps 50).
Registry/OSV lookups are cached in `~/.caret/verify/registry-cache.json` (7 d hit, 1 h miss, 24 h OSV)
and are skipped entirely in privacy mode.

## Settings (`settings.verify`)

`autoRunAfterTurn` (true), `runTests` (true), `mutationTesting` (true), `mutationMaxMutants` (12),
`dependencyGrounding` (true), `securityRules` (true), `projectCheckCommand`, `testCommand`,
`timeoutSecs` (300), `blockMergeOnFail` (false), `useSemgrepIfAvailable` (true).

## Review-on-diff commands (`api.review.*`)

`chat_review_comments`, `chat_add_review_comment`, `chat_resolve_review_comment`,
`chat_delete_review_comment`, `chat_send_review_comments` (renders unresolved comments as a
structured follow-up turn with ±3 lines of context and submits it), `chat_review_groups(sessionId, by?)`
with `by` = `intent` (default, one group per turn titled by the assistant's opening line), `tool-call`,
`file-kind` (types → implementation → tests → docs → config → lockfiles) or `file`.

## Compiled test suites (build-before-test)

When a project's tests import compiled output (`dist/`, `build/`, `out/`, `lib/`) and `package.json` has a `build`
script, the engine runs `<pm> run build && <pm> test` for the tests check **and for every mutant**. Without this,
mutations of `src/` never reach the test run and every mutant would falsely "survive". Override with
`settings.verify.preTestCommand` (any shell command run before the test command) or disable by setting
`testCommand` explicitly.

# How Caret's feature inventory was derived (REA + bundle analysis)

Caret is a **clean-room** implementation. To decide *what* a Cursor-class editor must do we inventoried the
locally installed Cursor **3.27.6** (`/Applications/Cursor.app`, commit `f7119520…`, VS Code fork) with
[REA — Reverse Engineer Anything](https://github.com/morluto/rea) (`rea-agents` 6.8.0) plus string mining of the
shipped bundles. No Cursor source, prompts or assets were copied; only the resulting *feature and architecture
map* informed Caret's design, and every Caret module is an independent implementation.

## REA runs

| Command | Target | Result |
|---|---|---|
| `rea inspect-desktop-application /Applications/Cursor.app --detail summary` | app bundle | 6 481 entries examined, 67 files probed, evidence `ev_e0689831…`. **0 runtime candidates** — REA's Electron detector keys on `resources/app.asar`; Cursor ships an *unpacked* `Contents/Resources/app/` directory, so the signal is absent (a detector gap worth reporting upstream). |
| `rea analyze-javascript-application …/extensions/cursor-always-local` | 4.2 MB ext | application + semantic graph, 270 modules, 7 relevant files, 4.16 MB text parsed, 22 532 semantic unknowns (minified bundle) |
| `rea analyze-javascript-application …/extensions/cursor-mcp` | 1.6 MB ext | 730 modules, 2 parsed JS files, 40 000 unknowns |
| `rea analyze-javascript-application …/extensions/cursor-shadow-workspace` | 0.4 MB ext | 103 modules, 19 151 unknowns |
| `rea analyze-javascript-application …/extensions/cursor-agent-worker` | 1.8 MB ext | analysis did not complete (see `/tmp/cursor-re/cursor-agent-worker.err`) |

Evidence files (hundreds of MB) live in `/tmp/cursor-re/`; summaries were projected with
`rea inspect-analysis-view` (`{"source":{"kind":"evidence-file",…},"view":{"kind":"summary"}}`). REA's own
limitation notes apply: static, inert parsing of minified bundles — relationships are candidates, not proof of
runtime behaviour.

## What the bundle layout says about Cursor's architecture

```
Cursor.app/Contents/Resources/app/
  out/vs/workbench/workbench.desktop.main.js   40 MB  classic VS Code workbench + Cursor contribs
  out/vs/workbench/workbench.glass.main.js     47 MB  "Glass" — newer agents-first UI shell
  out/vs/workbench/workbench.anysphere-ui-automations.js  8.7 MB
  out/vs/glass/                                        glass UI runtime
  molten/ (73 MB)   standalone Electron app: electron-main · renderer · engine · pty-host · canvas · tray
  extensions/cursor-*  (21 first-party extensions)
  resources/helpers/   cursorsandbox · crepectl · cursor-update-supervisor · node
  product.json         agentHarnessCdnUrl, marketplace.cursorapi.com gallery, cursorTrustedExtensionAuthAccess
```

First-party extensions (size → role): `cursor-retrieval` 81 MB (indexing/retrieval, `.cursorignore` /
`.cursorindexingignore` languages, grep client), `cursor-agent-host` 55 MB (agent orchestration, remote inference
route), `cursor-agent-exec` 34 MB (tool execution with user permissions), `cursor-local-agent-runtime` 13 MB
(private inference host), `cursor-resolver` 5.8 MB (background-composer remote authority), `cursor-always-local`
4.2 MB (commit-message generation, experiments), `cursor-computer-use` 3 MB, `cursor-mcp` 1.6 MB, `cursor-agent-worker`,
`cursor-browser-automation` (MCP server for the built-in browser), `cursor-shadow-workspace`, `cursor-worktree-textmate`
(syntax-only highlighting for `.cursor/worktrees/**`), `cursor-deeplink`, `cursor-commits`, `cursor-checkout`,
`cursor-ndjson-ingest` (writes `.cursor/debug.log`), `cursor-socket`, `cursor-file-service`, `cursor-explorer`,
`cursor-polyfills-remote`, `cursor-resolver-helper`.

NLS module ids in `out/nls.keys.json` name the UI surfaces: `contrib/composer/*` (the agent pane, prompt input
trays, transcript tool runtime, realtime voice, browser editor, chimes), `contrib/agents/*` (agents sidebar + quick
access), `browser/actions/agentLayoutQuickMenu`, `contrib/cursorMarkdownPlanEditor` (Plan mode editor),
`contrib/scm/browser/editorBugbotViewPane` (Bugbot), `contrib/cursorBlame`, `terminalContrib/cursorCliPrompt`,
`terminalContrib/cursorPromptBar` (terminal ⌘K), `terminalContrib/cursorExternalAgentStatus`,
`contrib/cursorAiFeedback`, `contrib/cursorOrigin` (copy code links), `services/agent/*`, `services/agentData/*`,
`platform/agentHarness/*`, `platform/mcpProcess/*`.

## Feature signals mined from `workbench.desktop.main.js` (41.5 M chars)

Settings keys: `cursor.cpp.enablePartialAccepts`, `cursor.cpp.disabledLanguages`, `cursor.composer.shouldChimeAfterChatFinishes`,
`cursor.composer.queueMessageDefaultBehavior`, `cursor.composer.subagentModel`, `cursor.composer.usageSummaryDisplay`,
`cursor.composer.textSizeScale`, `cursor.general.globalCursorIgnoreList`, `cursor.general.indexRepositoriesForInstantGrep`,
`cursor.general.gitGraphIndexing`, `cursor.terminal.usePreviewBox`, `cursor.terminal.enableAiChecks`,
`cursor.agentIdeUnification.*`, `cursor.browserTabEnabled`, `cursor.browserView.*` (navigate / takeScreenshot / getConsoleLogs /
getNetworkRequests / sendCDPCommand / newHeadlessTab…), `cursor.hooks.initializeUserHooks`,
`cursor.rules.convertLegacyAgentAppliedRules`, `cursor.agentsWindow.*`, `cursor.agentHost.updateMode`.

Config conventions (occurrence counts): `.cursor/rules` + `alwaysApply` (40), `.cursorrules`, `AGENTS.md`, `CLAUDE.md`,
`.cursor/skills` / `SKILL.md` (78), `.cursor/agents`, `.cursor/commands`, `.cursor/mcp.json` (11), `.cursorignore` (50),
`.cursorindexingignore`, `.cursor/worktrees` (10), `.cursor/plans` (12), `.cursor/environment.json`.

Hook events: `beforeSubmitPrompt` (37), `afterAgentResponse`/`afterAgentThought` (24), `beforeReadFile` (11),
`afterFileEdit` (8), `beforeShellExecution` (6), `beforeMCPExecution` (6).

Concept frequency: worktree 1 427 · allowlist/denylist 1 132 · Bugbot 580 · checkpoint 576 · sandbox 571 ·
Cloud Agent 197 · terminal prompt bar 189 · next-edit/cursor prediction 101 · YOLO (auto-run) 100 · parallel agents /
best-of-N 69 · computer use 51 · privacy mode 47 · memories 28 · notepads 25 · compaction/summarize 234 · queue 2 621.

Tool vocabulary present in the bundle: `read`, `view`, `create`, `write`, `diff`, `shell`, `ls`, `glob`, `grep`, `task`,
`subagent`, `skill`, `compact`, `web_search`, `web_fetch`, `switch_mode`, `mcp_tool`, `list_mcp_resources`,
`read_mcp_resource`, `create_plan`, `todo_write`, `ask_user`/`ask_question`, `read_lints`, `delete_file`,
`glob_file_search`, `browser_navigate`, `computer_use`, `generate_image`, `cloud_agent`.

Modes: Agent, Ask, Plan, Debug (plus custom); "Local" and "Browser" appear as additional mode labels.

## Mapping to Caret

| Cursor signal | Caret implementation |
|---|---|
| Tab / `cursor.cpp.*`, next-edit prediction, partial accepts | `src-tauri/src/ai/tab.rs`, `src/lib/monaco/inlineCompletions.ts`, `nextEdit` widget |
| ⌘K prompt bar, terminal prompt bar | `ai/inline_edit.rs`, `InlineEditWidget`, terminal ⌘K bar |
| Composer / agent pane, queue, chimes, usage summary | `ai/agent/runner.rs` + `components/chat` |
| Agents sidebar, agents window/layout, worktrees, background composer | `AgentManager`, `AgentsSidebar`, `AgentsLayout`, git worktree sessions |
| Checkpoints, review changes, accept/reject | `ai/agent/checkpoints.rs`, `ReviewBar`, diff editor |
| Rules (`alwaysApply`, globs, description), AGENTS.md, legacy rules | `ai/agent/rules.rs` |
| Memories, hooks, commands, skills, subagents, plans | `ai/agent/{memories,hooks,commands,skills,subagents,plans}.rs` |
| Retrieval extension, ignore files, Merkle hashing, embeddings | `index/*` (tantivy + vectors + manifest hashes) |
| MCP extension, `.cursor/mcp.json` | `mcp/*` (rmcp client) |
| Browser automation, browserView commands | `browser/*` (Tauri windows + injected snapshot script) |
| Bugbot view pane | `ai/review.rs`, `ReviewPanel` |
| Commit message generation, blame | `ai/commit.rs`, `core/git.rs` + editor blame decorations |
| Allowlist/denylist, YOLO, sandbox, privacy mode | `ai/agent/policy.rs`, settings `agent.*`, `privacy.*` |
| Compaction / summarize | `ai/agent/summarize.rs` |
| Voice, computer use, cloud agents, design mode | **not implemented** (require cloud services / OS automation); listed as beta flags |

## Reproducing

```bash
npx -y rea-agents@latest inspect-desktop-application /Applications/Cursor.app --detail summary \
  --evidence_output /tmp/cursor-re/desktop.evidence.json --json
npx -y rea-agents@latest analyze-javascript-application \
  /Applications/Cursor.app/Contents/Resources/app/extensions/cursor-mcp --json > /tmp/cursor-re/cursor-mcp.evidence.json
echo '{"source":{"kind":"evidence-file","path":"/tmp/cursor-re/cursor-mcp.evidence.json"},"view":{"kind":"summary"}}' > view.json
npx -y rea-agents@latest inspect-analysis-view view.json --json
```

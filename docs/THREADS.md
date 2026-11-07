# Threads: lifecycle, search and navigation

A chat session in Caret is a *thread*. Threads live under `~/.caret/sessions/<workspace-hash>/<id>.json`
(summaries in `index.json`) and show up in two places: the history popover in the AI pane header (⌘⇧H) and the
Agents sidebar (⌘⇧A). Both render the same grouped, foldable list described here.

## Lifecycle

Every thread carries a small set of lifecycle fields (`Session` / `SessionSummary` in
`src-tauri/src/ai/agent/session.rs`, mirrored by `ChatSession` / `ChatSessionSummary` in `src/types.ts`):

| Field | Meaning |
| --- | --- |
| `pinned`, `pinnedAt` | Kept in the Pinned section (most recently pinned first). |
| `snoozedUntil` | Unix ms. While in the future the thread sits in the collapsed Snoozed section; when it passes the thread wakes automatically. |
| `settled` | "Done": moved to the collapsed Settled section. Set by hand or by the auto-settle setting. |
| `archived`, `archivedAt` | Hidden unless the list's *archived* toggle is on. |

All of these are changed through the row menu (the `...` button on a thread) and every change is persisted at once
and announced to the UI with the `session-updated` agent event, so other views (sidebar, popover, pane header) update
without a reload.

- **Pin / Unpin.** Pinned threads stay near the top of the list.
- **Snooze.** Presets: *1 hour*, *Tonight 18:00* (becomes *Tomorrow 18:00* once that has passed), *Tomorrow 09:00*,
  *Next week (Mon 09:00)*. A snoozed row shows a small alarm-clock icon with its wake time ("in 45m", "Tomorrow 09:00",
  "Mon 09:00"). *Unsnooze* appears while a thread is snoozed. Expired snoozes are cleared whenever the list is loaded
  (`list_sessions` / `list_sessions_all`); the wake is written back to disk.
- **Settle / Unsettle.** Marks a thread as finished. Settling shows a "Thread settled" toast with **Undo** for 5 seconds.
- **Archive / Unarchive.** Archiving shows a "Thread archived" toast with **Undo** for 5 seconds.
- **Sending a message** to a snoozed or settled thread wakes it and clears `settled`: activity always wins.

### Auto-settle

`chat.autoSettleDays` (Settings → Agent → Threads, default `0` = off) marks threads as settled when they have had no
activity for that many days. Pinned, snoozed, archived and running threads are never auto-settled, and settling never
touches `updatedAt`. The check runs when the thread list is loaded.

## The grouped list

`groupSessions(sessions, opts)` in `src/components/chat/util.ts` is a pure function (unit-tested in `util.test.ts`)
that produces ordered sections; empty sections are omitted:

1. **Working**: threads whose status is busy (`running`, `awaiting-approval`, `awaiting-input`, `summarizing`);
   threads waiting for you come first.
2. **Pinned**
3. **Today**, **Yesterday**, **This week**, **Earlier** (by last activity)
4. **Snoozed** (collapsed by default, soonest wake first; the header shows the next wake time while collapsed)
5. **Settled** (collapsed by default)
6. **Archived** (only when the *archived* toggle is on)

Precedence when a thread qualifies for several sections: archived, working, snoozed, settled, pinned, day bucket. So a
snoozed thread hides even if pinned, and a settled thread goes to Settled even if pinned.

Every section header folds; the fold state is persisted (`threadFolds` in the ui store, `localStorage` key `caret.ui.v1`)
together with the *archived* and *by project* toggles.

### Group by project

The *by project* toggle (history popover) or the folder icon (Agents sidebar) switches to one group per workspace:
the current workspace first, then projects by most recent activity, each containing the lifecycle sections above. The
data comes from `chat_list_sessions_all`, which walks every `index.json` under `~/.caret/sessions`; summaries carry
their `workspaceRoot`, and the group title is the folder's basename. Project groups fold with the key `project:<root>`.

Selecting a thread from another project asks whether to open that folder (through the normal `workspace.openFolder`
command); once the new workspace's thread list arrives the thread is selected. Declining leaves everything as is.

## Regenerate title

Row menu → **Regenerate title**, or the `chat.regenerateTitle` command (palette: "Chat: Regenerate Thread Title").
It clears `titleGenerated`, asks the session's model (or the default) for a fresh 3 to 6 word title and emits the usual
`title` event. It needs at least one assistant reply; otherwise the command reports an error.

## Message search

Typing at least 3 characters in the history popover search box filters thread titles and previews immediately and,
after a 200 ms debounce, runs `chat_search_messages`: a case-insensitive substring search over the text of user and
assistant messages in every session file of the current workspace (every workspace when *by project* is on; archived
threads only when *archived* is on). Hits appear in a **Messages** section under the thread matches with the thread
title, role and a ~160 character snippet around the match. Clicking a hit opens the thread and scrolls to the message
(rows carry `data-message-id`; the message list widens its window if the message is far back and flashes it briefly).

`chat.searchMessages` (palette: "Chat: Search Messages in All Threads", optional `{ query }` argument) opens the popover
focused on the search box.

## Keyboard

| Command | Default | Effect |
| --- | --- | --- |
| `chat.nextThread` | `ctrl+alt+]` | Select the next thread in the visible grouped order (collapsed sections skipped). |
| `chat.prevThread` | `ctrl+alt+[` | Select the previous thread. |
| `chat.history` | `mod+shift+h` | Open the history popover. |
| `chat.searchMessages` | none | Open the popover focused on search. |
| `chat.regenerateTitle` | none | Regenerate the active thread's title. |

Rebind them in Settings → Keybindings or `~/.caret/keybindings.json`.


## Queue, steer, background threads and notifications

While an agent turn is running you have three ways to get another message in, and the app tells you when a thread
needs you.

### Queue vs steer

| | Queue | Steer |
|---|---|---|
| When it runs | After the current turn finishes (and after anything queued before it) | Before the agent's next model round, inside the current turn |
| Keyboard while running | `Enter` (default) | `Alt+Enter` (`Option+Enter` on macOS) |
| Shown as | Editable rows under the chat | A "Steering..." row until the runner injects it, then a normal user message |
| Checkpoint | Opens a new turn and checkpoint | Belongs to the running turn (same turn id, no new checkpoint) |
| External (ACP) agents | Same | Falls back to *send now*: front of the queue, current turn cancelled |

Steer is for "also do X" or "stop, use Y instead" corrections: the runner drains the steer buffer at the top of
every round, appends the text as a user message to the chat and to the model transcript, and if the model had
just produced a final answer with no tool calls it does not end the turn but runs one more round with your message.
When the transcript currently ends with a user message (tool results) the steered text is appended to it so tool
results stay first and roles keep alternating; otherwise it starts a new user message. Anything still in the steer
buffer when a turn ends (for example a steer that raced the final round) is moved to the front of the queue so it
is never lost. The steer buffer is in memory only; the queue is persisted with the session.

#### The modifier and the default

Next to the stop button while a turn runs there is a small split control labelled **Queue** or **Steer**: its main
button performs that action, the chevron opens a menu to flip the default (persisted as `steerByDefault` in the ui
store) and to *Send in a new thread*. `Enter` always does the default and `Alt+Enter` does the other one, so with
Steer as default `Alt+Enter` queues. The tooltip spells out both.

#### Send in a new thread

`Cmd+Shift+Enter` (`Ctrl+Shift+Enter`) sends the draft in a new background session that carries the draft's agent,
model, mode, thinking/effort and worktree choice. You stay on the current thread; an in-app toast "Started in a new
thread" offers **Open**. Commands: `chat.sendInNewThread` and `chat.steer` (both accept `{ text?, sessionId? }` and
default to the active session's draft).

### Editable queue

Queued rows can be edited in place (click the text; `Enter` saves, `Shift+Enter` inserts a newline, `Esc` cancels,
blur saves), reordered with the up/down arrows, sent immediately (**Send now** interrupts the current turn and runs
that item next) or removed. Each queued item now persists the full request it was submitted with (model, thinking,
effort, agent), so a restart no longer loses those choices; items saved by older builds are rebuilt from their text,
context and the session's current model and agent when they drain.

Backend: `chat_queue_update(session_id, queued_id, text, context?)`, `chat_queue_move(session_id, queued_id, index)`
(index is clamped), `chat_steer(req)`; events `queue` (full list) and `steer` (pending steer items; empty once
drained).

### Notifications (Settings > General > Notifications)

| Setting | Default | Meaning |
|---|---|---|
| `notifications.desktop` | on | Master switch for OS notifications |
| `notifications.onlyWhenUnfocused` | on | Skip notifications while the Caret window has focus |
| `notifications.onFinish` | on | A thread finished (running to idle; cancelled turns do not notify) |
| `notifications.onError` | on | A thread failed (`message-end` error, `error` event or error status; one notification per failure) |
| `notifications.onApproval` | on | A command, edit or tool call waits for your approval (body = the approval title) |
| `notifications.onQuestion` | on | The agent asked a question (body = the question) |
| `notifications.sound` | off | Short two-tone Web Audio beep when a thread finishes, independent of the desktop notification |
| `agent.chimeOnFinish` | off | Same beep (older setting, now implemented) |

Notifications are per session and coalesced: the backend emits several events for one transition (for example
`message-end` error followed by `session-status` error), and only one notification fires per kind per session
inside a short window. Subagent sessions never notify. The title is the thread title and the body is one line
(finished: the start of the final answer; failed: the error; approval: the request title; question: the question).

Implementation: `tauri-plugin-notification` (Rust side only, `notification:default` capability) behind the
`app_notify_desktop(title, body)` command (`api.app.notifyDesktop`); `src/lib/desktopNotifications.ts` watches the
raw agent event stream from `chatBridge` and holds the pure, unit-tested decision logic (`AgentNotifier`).

Note: unsigned development builds (`npm run tauri dev`) may not display notifications on macOS, which only shows
banners for apps with a bundle identifier it trusts; the bundled app (`dev.caret.editor`) does. The in-app toasts and
the chime work in both.

## Forks, provider handoff and multi-model fan-out

Three thread operations share one piece of machinery: a budgeted view of the conversation
(`src-tauri/src/ai/agent/handoff.rs`).

### The handoff budget

`chat.handoffBudgetTokens` (default 12000, Settings > Agent > Behaviour) bounds how much of a thread travels when
it moves to another agent or model. Tokens are estimated as characters / 4. `plan_handoff(messages, budget)` walks
the UI messages from the newest backwards: the most recent messages that fit the budget travel verbatim, everything
older is compressed into one `<conversation_summary>` block. When anything overflows, a quarter of the budget (200 to
1500 tokens) is reserved for that block. The newest message is always kept (clipped in the middle when it alone is
over budget). UI-only system notes (handoff notes, lint follow-ups) are never transferred; compaction summaries are.

The summary is written by the summariser model with the same prompt as transcript compaction. When no model
resolves or the call fails, a deterministic digest (one line per compacted message) is used instead, so a handoff
never blocks on the LLM.

### Forks

`chat_fork_session(sessionId, messageId?)` clones a session into a new idle thread titled `Fork: <title>` with
`parentSessionId` = source and `forkOfMessageId` = the cut point. It is not a subagent. Queue, worktree, checkpoints,
pending changes, pin and usage are reset; the restore links on messages are dropped because file snapshots belong to
the parent.

With a `messageId` the fork keeps messages up to and including that one. The LLM transcript is cut exactly when the
turn's `transcript_marks` are intact: after the last message of a turn the cut is the next turn's mark (or the end);
after a user message or a mid-turn assistant message the transcript before the turn is kept and the retained part of
the turn is re-added as text. When the marks are gone (compaction cleared them) the transcript is rebuilt from the
kept messages through the budget plan. The fork keeps the marks of its retained turns so regenerate and edit work in
it.

UI: **Fork from here** in the hover bar of every user and assistant message, **Fork thread** in the chat header, and
the `chat.fork` command. A fork shows a "Forked from <parent>" link under the header that selects the parent.

### Provider handoff

`chat_handoff_session(sessionId, agentId?, modelId?)` moves an idle thread (error when running):

- built-in to ACP agent: `agentId` is set. The ACP bridge sends the budgeted history with the first prompt of a fresh
  external session (`SessionLink.history_pending`, also set when the session was prepared eagerly before the first
  message, which previously skipped the history). The history block uses the session's last compaction or handoff
  summary for the compacted part.
- ACP to built-in: the LLM `transcript` is rebuilt from `messages` (summary exchange first when over budget, then the
  verbatim messages as text turns; consecutive same-role messages are merged), `transcript_model` and the marks are
  cleared, and the external agent's live session is dropped so a later return starts fresh with the transferred
  history.
- model-only change on the built-in agent: `model_id` is set and `ensure_transcript_model` drops the previous
  model's thinking blocks.

Every handoff appends a system message rendered as a muted divider ("Handed off to Codex. Transferred 14 messages,
2 summarised.", "Switched to GPT-5."); when something was summarised the note carries the expandable summary. The
command emits `SessionReset` so the UI reloads the thread.

UI: picking another agent in the composer's agent picker on a thread with messages opens a confirmation ("Hand off
this thread to X?"). Picking another model in the model picker on a built-in, idle thread with messages hands off
directly (no confirmation). Empty, busy or external-agent threads just change the draft as before.

### `read_thread` tool

Built-in, read-only tool (`tools/thread_tools.rs`): `read_thread(offset?, limit?)` returns the earlier messages of
the current thread verbatim, oldest first (default 20, max 50 per page), excluding the turn in flight. It exists so
an agent that received a summary (after compaction or a handoff) can fetch the omitted history instead of guessing;
the system prompt mentions it when the tool is active.

### Multi-model fan-out

`chat_fan_out(sessionId?, text, modelIds, useWorktree?, mode?, context?, images?, thinking?, effort?)` creates one
session per distinct model (forks of `sessionId` when it has messages, fresh sessions otherwise), sets `model_id`,
`fan_out_group` (one uuid for the batch) and the title `<title or first line> · <model display name>`, creates a
worktree per session when requested, emits `SessionCreated` for each and submits the same prompt to all of them so
they stream concurrently. Returns the session ids in model order. Session summaries carry `fanOutGroup` so lists can
badge them.

UI: in the model picker, shift-click a model or press **Compare models** to select 2 to 4 models; the composer's
send button becomes **Send to N**. In the Agents layout, a thread that belongs to a fan-out group opens the
**Compare** view: 2 to 4 read-only columns with the model name, status and usage/cost footer and a **Continue here**
button that makes that thread active. The chat header shows a "Compare N" chip for such threads.

## Worktree policies and scratch threads

### Worktree policies (`settings.worktrees`)

| Key | Default | Effect |
|---|---|---|
| `perThreadDefault` | `false` | New threads start in their own git worktree. The composer's worktree pill reflects it; if the folder is not a git repo the thread is created without a worktree. |
| `location` | `.caret/worktrees` | Where agent worktrees live: relative to the workspace root, absolute, or `~/...`. In-repo locations are added to `.git/info/exclude`. |
| `branchTemplate` | `caret/{slug}` | Placeholders `{slug}` (thread title + 4 id chars, or `agent-<id8>`), `{model}` (slugified model id, `auto` when none), `{id}` (8 chars), `{date}` (YYYY-MM-DD). The result is sanitized to a valid ref name; a collision gets `-<id4>` appended. |
| `cleanupInactiveDays` | `14` | A worktree qualifies for cleanup when its thread's `updatedAt` is older than N days. `0` turns the policy off. |
| `cleanupSettled` | `true` | Worktrees of archived threads qualify (this branch has no `settled` flag; archived is the settled state). |
| `cleanupMerged` | `true` | Worktrees whose branch is merged into the default branch qualify (`origin/HEAD`, else main/master/develop/trunk). |
| `cleanupNoCommits` | `true` | Worktrees whose HEAD equals the recorded base commit qualify (`Session.worktreeBase`, recorded at creation; legacy worktrees fall back to "no commits beyond the default branch"). |
| `onLocalChanges` | `keep` | `keep` skips dirty worktrees, `stash` runs `git stash push --include-untracked` inside the worktree (the stash stays in the repository) before a forced removal, `discard` force-removes. |
| `deleteBranchWithWorktree` | `true` | Delete the branch with the worktree when it has no commits beyond the default branch. Branches with unmerged commits (or unknown state) are kept and reported. Also applies when a thread is deleted (`git branch -d`). |
| `autoCleanup` | `false` | On app start (and when a folder opens), run preview + cleanup once per workspace when any policy is on, and post an in-app notification with the report. |

Running threads never qualify. A worktree whose folder is missing always qualifies (stale registration: `git worktree prune`).

Commands (all `api.chat.*`):

- `chat_worktree_status(session_id) -> { path, branch, hasCommitsBeyondBase, dirty, mergedIntoDefault, lastActivity }`
- `chat_worktree_cleanup_preview() -> [{ sessionId, title, path, branch, reasons[], dirty }]` for the open workspace
- `chat_worktree_cleanup(session_ids) -> { removed[], skipped: [{ sessionId, reason }], notes[] }`. Removed threads keep their
  transcript; `worktreePath` and `branch` are cleared and the cwd falls back to the workspace root (the UI reloads via
  `session-reset`).

UI: Source Control > Worktrees shows agent worktrees with the thread title, the branch as a chip and an "agent" badge,
plus a "Clean up agent worktrees" button (header icon and footer button) that opens the preview with checkboxes
(dirty worktrees unchecked by default) and a Remove button. Settings > Agent > Worktrees holds the policies.

### Scratch threads

A scratch thread is a chat with its own throwaway repository: `chat_create_scratch_session(mode, model_id, title, agent_id)`
creates `~/.caret/scratch/<id>/` (honours `CARET_HOME`), runs `git init` plus an initial empty commit on `main`, roots the
session (`workspaceRoot` = `cwd` = that folder), sets `Session.scratch = true` and persists it under the `"global"`
session key so it works with no folder open and is listed in every workspace. Deleting a scratch thread removes its folder.

`chat_promote_scratch(session_id, target_dir)` moves the folder to `target_dir` (must not exist or be empty; `~` is
expanded; rename with a copy fallback across devices), rewrites `workspaceRoot`/`cwd`, moves the session file to the new
workspace key and the checkpoints from `~/.caret/checkpoints/global/<id>` to `.../<new key>/<id>`, clears `scratch` and
returns the view. The frontend then opens `target_dir` as the workspace (`api.workspace.open`) and re-selects the thread
once the workspace-change reload finishes (`useChat.pendingSelect`).

UI: command `chat.newScratch` ("Chat: New Scratch Thread", palette; accepts `{ mode?, agentId?, title? }`, returns the
session id) and a banner at the top of the chat pane for scratch threads ("Scratch thread in ~/.caret/scratch/<id>" with a
"Make it a project..." button that opens the folder picker and calls promote).

## Backend reference

Commands (`src-tauri/src/ai/agent/chat_commands.rs`, wrappers in `api.chat.*`):

| Command | Arguments | Returns |
| --- | --- | --- |
| `chat_snooze_session` | `sessionId`, `until?: u64` (ms; omitted or past = unsnooze) | `SessionSummary` |
| `chat_settle_session` | `sessionId`, `settled: bool` | `SessionSummary` |
| `chat_list_sessions_all` | `includeArchived?: bool` | `SessionSummary[]` across all workspaces |
| `chat_regenerate_title` | `sessionId` | the new title |
| `chat_search_messages` | `query`, `includeArchived?`, `allWorkspaces?`, `limit?` (max 500) | `MessageSearchHit[]`: `sessionId`, `sessionTitle`, `workspaceRoot`, `messageId`, `role`, `snippet`, `updatedAt` |

`chat_pin_session` and `chat_archive_session` now also stamp `pinnedAt` / `archivedAt` and emit `session-updated`.

Event: `AgentEvent::SessionUpdated { session: SessionSummary }` (`{ type: 'session-updated', session }` on the wire).
The reducer (`src/store/chatReducer.ts`) patches the summary and mirrors the lifecycle fields onto the loaded session.

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

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

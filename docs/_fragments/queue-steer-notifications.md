# Queue, steer, background threads and notifications

While an agent turn is running you have three ways to get another message in, and the app tells you when a thread
needs you.

## Queue vs steer

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

### The modifier and the default

Next to the stop button while a turn runs there is a small split control labelled **Queue** or **Steer**: its main
button performs that action, the chevron opens a menu to flip the default (persisted as `steerByDefault` in the ui
store) and to *Send in a new thread*. `Enter` always does the default and `Alt+Enter` does the other one, so with
Steer as default `Alt+Enter` queues. The tooltip spells out both.

### Send in a new thread

`Cmd+Shift+Enter` (`Ctrl+Shift+Enter`) sends the draft in a new background session that carries the draft's agent,
model, mode, thinking/effort and worktree choice. You stay on the current thread; an in-app toast "Started in a new
thread" offers **Open**. Commands: `chat.sendInNewThread` and `chat.steer` (both accept `{ text?, sessionId? }` and
default to the active session's draft).

## Editable queue

Queued rows can be edited in place (click the text; `Enter` saves, `Shift+Enter` inserts a newline, `Esc` cancels,
blur saves), reordered with the up/down arrows, sent immediately (**Send now** interrupts the current turn and runs
that item next) or removed. Each queued item now persists the full request it was submitted with (model, thinking,
effort, agent), so a restart no longer loses those choices; items saved by older builds are rebuilt from their text,
context and the session's current model and agent when they drain.

Backend: `chat_queue_update(session_id, queued_id, text, context?)`, `chat_queue_move(session_id, queued_id, index)`
(index is clamped), `chat_steer(req)`; events `queue` (full list) and `steer` (pending steer items; empty once
drained).

## Notifications (Settings > General > Notifications)

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

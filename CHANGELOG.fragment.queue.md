- Steer a running agent: `Alt+Enter` (or the new Queue/Steer split control next to the stop button) injects your
  message before the agent's next step inside the current turn instead of waiting for it to finish; if the model had
  just answered, the turn continues with your message. The default for `Enter` while running is switchable
  (Queue or Steer) and remembered. External ACP agents fall back to send now (front of the queue, current turn
  cancelled). New command `chat_steer`, event `steer`, UI command `chat.steer`.
- `Cmd+Shift+Enter` sends the draft in a new background thread (same agent, model, mode, thinking) and keeps you on
  the current one; a toast offers **Open**. UI command `chat.sendInNewThread`.
- Queued messages are editable (click to edit, Enter saves, Esc cancels), reorderable (up/down) and keep Send now /
  Remove. Queued items now persist the full request (model, thinking, effort, agent) so a restart no longer drops
  those choices; older items are rebuilt from the session's current model and agent. Commands `chat_queue_update`,
  `chat_queue_move`.
- Desktop notifications when a thread finishes, fails, needs approval or asks a question (Settings > General >
  Notifications: master switch, only-when-unfocused, per-kind toggles, optional sound). One notification per
  transition, subagents excluded. `agent.chimeOnFinish` is now implemented as a short Web Audio beep, as is
  `notifications.sound`. Adds `tauri-plugin-notification` and the `app_notify_desktop` command.

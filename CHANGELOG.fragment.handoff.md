- Threads: **Fork from here** on any message (and **Fork thread** in the chat header) copies the conversation up to
  that point into a new thread, cut exactly at the turn boundary; the fork shows a "Forked from" link back to its
  parent. **Hand off** a thread mid-conversation to another agent or model from the agent and model pickers: the
  conversation is transferred within `chat.handoffBudgetTokens` (recent messages verbatim, older ones summarised),
  a muted note records the transfer, and the new `read_thread` tool lets the agent fetch summarised-away messages
  verbatim. External (ACP) agents now receive the same budgeted history on their first prompt, including when the
  agent was connected before the first message.
- **Compare models**: shift-click or toggle "Compare models" in the model picker to pick 2 to 4 models; Send becomes
  **Send to N** and runs the prompt in one thread per model (forks of the current thread when it has history, one
  worktree each when enabled). The Agents layout shows the batch side by side with per-model usage and a
  "Continue here" button.

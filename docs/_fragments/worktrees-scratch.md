# Worktree policies and scratch threads

Fragment for the threads doc. Backend: `src-tauri/src/ai/agent/worktrees.rs`, `scratch.rs`, commands at the end of
`chat_commands.rs`. Frontend: `src/components/scm/Worktrees.tsx` (cleanup dialog), `src/components/chat/ScratchBanner.tsx`,
`chat.newScratch` in `src/lib/chatBridge.ts`.

## Worktree policies (`settings.worktrees`)

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

## Scratch threads

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

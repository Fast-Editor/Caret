### Added

- Permission modes: Supervised, Auto-accept edits, Auto and Full access replace the single auto-run switch. Pick a mode per thread from the composer (shield icon) or set a default and a cap in Settings > Agent > Permissions; a workspace `.caret/settings.json` can lower the cap for a project. Approval cards show which mode was in effect. (`agent.permissionMode`, `agent.maxPermissionMode`, `chat_set_permission_mode`)
- External agents follow the thread's permission mode: Claude Code and Gemini CLI session modes (`default` / `acceptEdits` / `autoEdit` / `bypassPermissions` / `yolo`) and Codex-style approval config options are set when the session starts and whenever the mode changes. Full access skips Caret's destructive-command prompt for agent terminals; blocked commands stay blocked.
- Worktree policies (Settings > Agent > Worktrees): worktree-per-thread default, location and branch template (`{slug}`, `{model}`, `{id}`, `{date}`), cleanup rules (inactive days, archived, merged, no commits), what to do with local changes (keep / stash / discard), delete branch with worktree, optional automatic cleanup on start with a report notification. Source Control > Worktrees gets a "Clean up agent worktrees" preview and shows thread titles and branches on agent worktrees. (`chat_worktree_status`, `chat_worktree_cleanup_preview`, `chat_worktree_cleanup`)
- Scratch threads: `Chat: New Scratch Thread` starts a chat in its own throwaway git repository under `~/.caret/scratch/<id>`, even with no folder open. A banner offers "Make it a project...", which moves the folder to a directory you pick and opens it as the workspace. (`chat_create_scratch_session`, `chat_promote_scratch`)

### Changed

- `agent.autoRun` is now the legacy fallback used only when no default permission mode is set (off = Supervised, allowlist = Auto-accept edits, all = Auto).
- Deleting a thread that has a worktree also deletes its branch when `worktrees.deleteBranchWithWorktree` is on and the branch has no unmerged commits.

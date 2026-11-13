# Multiple windows

Status: in progress (2026-11-13). This document is the design the implementation follows; sections marked *done*
describe shipped behaviour.

## Behaviour

- **File → New Window** (Cmd+Shift+N) opens an empty window (Welcome + recent folders). Every window is a full Caret:
  explorer, editor, terminals, chat, git, search, LSP, Quick Open, all scoped to the folder that window has open.
- **Open Folder / Open Recent** in a window that already has a folder: the folder opens in a *new* window when
  `window.openFoldersIn` is `"new"` (default) and replaces the current one when it is `"current"`. Holding Alt while
  choosing a recent folder does the opposite of the setting. An empty window always opens the folder in place.
- **`caret <path>`, Finder "Open with", `caret://open?…`**: a folder that is already open in a window focuses that
  window; a file inside an open folder opens there; anything else goes to an empty window if one is focused, or to a
  new window (folders) or the focused window (loose files).
- **Window menu** lists the open windows (folder name, checkmark on the focused one) plus Minimize / Zoom /
  Bring All to Front; Cmd+` cycles windows (macOS standard).
- Closing the last window keeps Caret running on macOS (Dock click or Cmd+N reopens a window), quits elsewhere.
- Two windows may show the same folder; they share the watcher, the index and the language servers for it.
- Window titles are `<folder> — Caret` (just `Caret` when empty); traffic lights stay in the overlay title bar.

## Architecture

One process, N `WebviewWindow`s (labels `main`, `w2`, `w3`, …), one frontend bundle per window. Rust state that was
implicitly "the current workspace" becomes either **per window** (what folder a window shows, its pending CLI
opens) or **per root** (anything that is really about the folder: watcher, quick-open index, codebase index, LSP
servers, MCP servers, worktrees). Windows showing the same root share the per-root state through a reference count.

### Rust

`core/windows.rs` (new) — `WindowRegistry` (managed):

```rust
pub struct WindowEntry { pub label: String, pub root: Option<PathBuf>, pub created_at: Instant }
pub struct WindowRegistry { entries: DashMap<String, WindowEntry>, focused: RwLock<Option<String>>, next_id: AtomicU32 }
impl WindowRegistry {
    pub fn register(&self, label: &str);                 // on window creation (main at setup)
    pub fn unregister(&self, label: &str) -> Option<PathBuf>; // on WindowEvent::Destroyed; returns the root it had
    pub fn set_root(&self, label: &str, root: Option<PathBuf>) -> Option<PathBuf>; // returns previous root
    pub fn root_of(&self, label: &str) -> Option<PathBuf>;
    pub fn labels(&self) -> Vec<String>;
    pub fn labels_for_root(&self, root: &Path) -> Vec<String>;   // windows showing this folder
    pub fn window_containing(&self, path: &Path) -> Option<String>; // a window whose root is `path` or an ancestor of it
    pub fn set_focused(&self, label: &str); pub fn focused(&self) -> Option<String>;
    pub fn next_label(&self) -> String;                  // "w2", "w3", …
    pub fn is_last_window_for_root(&self, label: &str) -> bool;
}
```

`core/workspace.rs` — `WorkspaceState` keeps its name but reads through the registry:

- `root_for(label) -> Option<PathBuf>`, `require_root_for(label)`, `info_for_label(label)`.
- `root()` / `require_root()` (the old implicit accessors) resolve to the **focused** window's root and log at debug
  level; they stay only so unmigrated call sites compile. Every command that depends on the workspace takes
  `window: tauri::Window` and uses `window.label()`. Long-running work (agent runs, verify, index) captures the root
  when it starts and never calls `root()` again.
- `open_workspace(app, label, path)` / `close_workspace(app, label)`: update the registry, workspace-scope settings for
  that root, the recent list, the window title; then `events::emit_rust(WORKSPACE_CHANGED, WorkspaceChange { label,
  info, prev_root })` for Rust listeners and `events::emit_to(label, WORKSPACE_CHANGED, info)` for that window.
- Commands: `workspace_open(window, path)`, `workspace_current(window)`, `workspace_close(window)`; new
  `window_new(app, path: Option<String>) -> String`, `window_list() -> Vec<WindowSummary { label, root, name,
  focused }>`, `window_focus(label)`, `window_close(label)`.

`events.rs`:

- `emit` stays for app-wide events (settings, router, agent events, global notifications).
- `emit_to(app, label, event, payload)` for one window; `emit_for_root(app, root, event, payload)` for every window on
  that root (FS_CHANGED, GIT_CHANGED, INDEX_STATUS, LSP_STATUS, LSP_DIAGNOSTICS, MCP_CHANGED, RULES_CHANGED,
  DOCS_CHANGED, REVIEW_CHANGED).
- `MENU` goes only to the focused window (two windows must not both run File → Save). `OPEN_PATHS` goes to the window
  chosen by the CLI router. `NOTIFY` raised inside a command goes to the calling window; background notifications
  stay app-wide.
- Rust-side listeners use a Rust-only channel name (`caret-internal://workspace/changed`) carrying `{ label, info,
  prev_root }`, so a window switching folders tells them which root lost a window and which gained one.

Per-root resources (`watcher`, `fs` quick-open index, `index::IndexManager`, `mcp::McpManager`, `lsp`,
`ai::agent::worktrees`): a `RootRefs` helper (`core/windows.rs`) counts windows per root; the resource starts on the
first window for a root and stops when the last one leaves. Each keeps a `DashMap<PathBuf, …>` instead of a single
slot. Commands that read them take `window` and look up the window's root.

Window creation (`core/windows.rs::create_window`): `WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html"))`
with the same options as the `main` entry in `tauri.conf.json` (title bar overlay, hidden title, dark theme,
background colour, min size, drag-drop off), positioned cascaded from the focused window, registered before it is
shown. If a path is given, `open_workspace(app, label, path)` runs right after creation so `workspace_current` already
answers when the frontend boots (the window also receives WORKSPACE_CHANGED, whichever comes first wins idempotently).
`WindowEvent::Focused(true)` → `registry.set_focused`; `WindowEvent::Destroyed` → unregister, release the root, and on
non-macOS exit when no windows remain. `RunEvent::Reopen` (macOS Dock click with no windows) → `window_new(None)`.
`capabilities/default.json` lists `"w*"`.

CLI routing (`core/cli.rs`): `route_open(reqs, &registry, setting) -> Vec<(target: Target, reqs)>` where
`Target = Window(label) | NewWindow(path)`; pure and unit-tested. `OpenQueue` is keyed by label; `cli_take_pending_opens(window)`
returns only that window's requests. `on_opened` and `cli_open` call the router, create windows as needed, and emit
`OPEN_PATHS` to the target label.

### Frontend

Each window runs the same bundle; stores are per window automatically (separate webview). Changes:

- `api.workspace.*` unchanged (the calling window is implicit). New `api.window.{new, list, focus, close}`.
- `file.newWindow` → `api.window.new(null)`. `workspace.openFolder` / `openRecent` honour `window.openFoldersIn` and
  the Alt modifier. Window menu entries come from `window_list` (the native menu is rebuilt on `caret://windows/changed`).
- Editor session persistence key: `${root}::${label}` with a fallback read of the old `${root}` key, so two windows on
  one folder keep separate tab sets.
- `cliOpen.ts` is unchanged in shape; the backend only hands a window its own requests.
- Settings: `window.openFoldersIn: "new" | "current"` (mirrored in `settingsSchema.ts` and Rust `WindowSettings`).
- Automation bridge: `scripts/e2e.mjs eval --window w2 …` targets a label (`dev/automation.rs` takes `?window=`).

## Out of scope for this pass

Drag a tab out into a new window; per-window layout state (panel sizes are still shared through localStorage);
`caret serve` remote clients follow the `main` window only.

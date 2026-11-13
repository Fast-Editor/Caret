# Multiple windows

Status: shipped 2026-11-13 (phase 0 foundation + phase 1 per-folder scoping). Verified live in the dev build: a
second window on another folder opened in ~160 ms; Quick Open, the codebase index, git status, the TypeScript
server and the thread list in each window answered for that window's folder; closing the second window stopped its
watcher, index and MCP servers while the first window kept its own. Remaining gaps are listed under "Out of scope".

## Behaviour

- **File → New Window** (Cmd+Shift+N) opens an empty window: the Welcome view with Open Folder…, a "New window" link,
  the shortcut list and the recent folders (the explorer sidebar shows its own Open Folder + Recent list; the chat pane
  works without a folder). *done* — Every window is a full Caret: explorer, editor, terminals, chat, git, search, LSP,
  Quick Open, all scoped to the folder that window has open (per-root scoping of the Rust services: phase 1).
- **Open Folder / Open Recent** in a window that already has a folder: the folder opens in a *new* window when
  `window.openFoldersIn` is `"new"` (default) and replaces the current one when it is `"current"`. An empty window
  always opens the folder in place. *done* — The Welcome tab's Recent list follows the setting on click and does the
  opposite on **Alt-click** (its tooltip says which); every recent row (Welcome view and Welcome tab) has a hover
  "Open in New Window" action that always opens a new window. *done* — The Open Recent… picker still reports no
  modifiers, so Alt does nothing there.
- **Explorer → right-click a folder (or the empty area) → Open in New Window** opens that folder as its own window
  (`workspace.openInNewWindow`). *done*
- **Closing a window** (red button, File → Close Window, `window_close` from another window) with unsaved files shows
  one dialog listing every dirty file: **Save All** saves them (untitled buffers get a Save As each) and closes the
  window, **Don't Save** closes it, **Cancel** keeps it; a failed or cancelled save keeps the window. The window's
  terminals (PTYs) are killed before it is destroyed. A window with nothing dirty closes at once. The frontend always
  vetoes Tauri's close request and destroys the window itself (`src/lib/windowClose.ts`), so the Rust `Destroyed`
  handler sees exactly one close. *done*
- **Window: Switch Window…** (palette, `window.switch`) lists the open windows — folder name (or "Empty window"), the
  folder path, the label and a "current" marker, current window last — and focuses the chosen one; **Window: Close
  Other Windows** (`window.closeOthers`) asks each other window to close, each running its own unsaved-files dialog.
  *done*
- **`caret <path>`, Finder "Open with", `caret://open?…`**: a folder that is already open in a window focuses that
  window; a file inside an open folder opens there; anything else goes to an empty window if one is focused, or to a
  new window (folders) or the focused window (loose files). Phase 1; today the request goes to the focused window (a
  new one is created when none exists).
- **Window menu** lists the open windows (folder name, checkmark on the focused one) plus Minimize / Zoom /
  Bring All to Front. *done* — Cmd+` cycles windows (macOS's own "move focus to next window" shortcut; Caret binds
  nothing for it, so a user binding of `mod+backtick` would not conflict with anything Caret-side).
- Closing the last window keeps Caret running on macOS (Dock click or Cmd+Shift+N reopens a window), quits elsewhere. *done*
- Two windows may show the same folder; they share the watcher, the index and the language servers for it.
- Window titles are `<folder> — Caret` (just `Caret` when empty); traffic lights stay in the overlay title bar, the
  title bar's command center shows the folder name (tooltip: name and path) and `document.title` mirrors the native
  title for assistive tech (`windowTitle` in src/lib/windowing.ts). *done*

## Architecture

One process, N `WebviewWindow`s (labels `main`, `w2`, `w3`, …), one frontend bundle per window. Rust state that was
implicitly "the current workspace" becomes either **per window** (what folder a window shows, its pending CLI
opens) or **per root** (anything that is really about the folder: watcher, quick-open index, codebase index, LSP
servers, MCP servers, worktrees). Windows showing the same root share the per-root state through a reference count.

### Rust

`core/windows.rs` *done* — `WindowRegistry` (managed):

```rust
pub struct WindowEntry { pub label: String, pub root: Option<PathBuf>, pub created_at: Instant }
pub struct WindowRegistry { entries: DashMap<String, WindowEntry>, focused: RwLock<Option<String>>, next_id: AtomicU32 }
impl WindowRegistry {
    pub fn register(&self, label: &str);                 // on window creation (main at setup); idempotent
    pub fn unregister(&self, label: &str) -> Option<PathBuf>; // on WindowEvent::Destroyed; returns the root it had
    pub fn contains(&self, label: &str) -> bool; pub fn len(&self) -> usize; pub fn is_empty(&self) -> bool;
    pub fn set_root(&self, label: &str, root: Option<PathBuf>) -> Option<PathBuf>; // returns previous root; unknown label → None
    pub fn root_of(&self, label: &str) -> Option<PathBuf>;
    pub fn entry(&self, label: &str) -> Option<WindowEntry>; pub fn entries(&self) -> Vec<WindowEntry>; // creation order
    pub fn labels(&self) -> Vec<String>;
    pub fn labels_for_root(&self, root: &Path) -> Vec<String>;   // windows showing this folder
    pub fn roots(&self) -> Vec<PathBuf>;                 // distinct open roots
    pub fn window_containing(&self, path: &Path) -> Option<String>; // deepest root that is `path` or an ancestor; focused, then oldest on ties
    pub fn set_focused(&self, label: &str); pub fn focused(&self) -> Option<String>; // unregistered labels are ignored
    pub fn next_label(&self) -> String;                  // "w2", "w3", …
    pub fn is_last_window_for_root(&self, label: &str) -> bool; // false for a window without a folder
}
pub struct RootRefs;                                     // managed; windows per root
impl RootRefs { pub fn acquire(&self, root: &Path) -> bool /* first */; pub fn release(&self, root: &Path) -> bool /* last */; pub fn count(&self, root: &Path) -> usize; pub fn roots(&self) -> Vec<PathBuf>; }
pub struct WindowSummary { pub label: String, pub root: Option<String>, pub name: Option<String>, pub focused: bool }
pub fn summaries(registry: &WindowRegistry) -> Vec<WindowSummary>;
pub fn focused_window(app: &AppHandle) -> Option<WebviewWindow>; // focused → main → oldest; None only when no Caret window exists
pub fn focused_label(app: &AppHandle) -> Option<String>;
pub fn window_title(root: Option<&Path>) -> String;      // "<folder> — Caret" / "Caret"
pub fn create_window(app: &AppHandle, path: Option<PathBuf>) -> Result<String>;
pub fn focus_window(app: &AppHandle, label: &str) -> Result<()>;
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent); // Builder::on_window_event
// commands: window_new(app, path: Option<String>) -> String, window_list(app) -> Vec<WindowSummary>, window_focus(app, label), window_close(app, label)
```

`core/workspace.rs` *done* — `WorkspaceState` keeps its name but reads through the registry (it holds an
`AppHandle`, attached at setup, and no root):

- `root_for(label) -> Option<PathBuf>`, `require_root_for(label)`, `info_for_label(label)`.
- `root()` / `require_root()` / `info()` (the old implicit accessors) resolve to the **focused** window's root (an
  empty focused window answers `None`); with no focused window known they fall back to `main`'s root, then to the only
  open root. Logged at trace level. They stay only so unmigrated call sites compile. Every command that depends on the
  workspace takes `window: tauri::Window` and uses `window.label()`. Long-running work (agent runs, verify, index)
  captures the root when it starts and never calls `root()` again.
- `open_workspace_for(app, label, path) -> Result<WorkspaceInfo>` / `close_workspace_for(app, label)`: canonicalize +
  `is_dir`, `registry.set_root`, `RootRefs` release of the previous root and acquire of the new one, workspace-scope
  settings (focused window only, until phase 1 makes them per root), the recent list, the window title; then
  `events::emit_internal_workspace_changed(app, WorkspaceChange { label, info, prev_root, first_for_root,
  last_for_prev_root })` for Rust listeners and `events::emit_to(app, label, WORKSPACE_CHANGED, info)` for that
  window. `release_workspace_for_destroyed(app, label)` is the same release path for a window that is going away
  (nothing is sent to its webview). `open_workspace(app, path)` / `close_workspace(app)` are thin wrappers that target
  the focused window for callers that have no window yet (CLI, phase 1 removes them).
- Commands: `workspace_open(app, window, path)`, `workspace_current(ws, window)`, `workspace_close(app, window)`,
  `workspace_ensure_project_dir(ws, window)`; `workspace_recent`, `workspace_remove_recent` unchanged.

`events.rs` *done*:

- `emit` stays for app-wide events (settings, router, agent events, global notifications).
- `emit_to(app, label, event, payload)` for one window; `emit_for_root(app, root, event, payload)` for every window on
  that root (FS_CHANGED, GIT_CHANGED, INDEX_STATUS, LSP_STATUS, LSP_DIAGNOSTICS, MCP_CHANGED, RULES_CHANGED,
  DOCS_CHANGED, REVIEW_CHANGED) plus Rust `app.listen` listeners, which are not per window (`root_target_matches` is
  the pure filter).
- `MENU` goes only to the focused window (two windows must not both run File → Save). *done* — `OPEN_PATHS` goes to
  the window chosen by the CLI router (phase 1). `NOTIFY` raised inside a command goes to the calling window;
  background notifications stay app-wide (phase 1).
- Rust-side listeners use a Rust-only channel (`INTERNAL_WORKSPACE_CHANGED = "caret-internal://workspace/changed"`,
  emitted to `EventTarget::App` so it never reaches a webview) carrying `WorkspaceChange { label, info, prev_root,
  first_for_root, last_for_prev_root }`, so a window switching folders tells them which root lost a window and which
  gained one. Subscribe with `events::on_workspace_change(app, |app, change| …)`; the handler runs inline inside the
  emitting command, so hand real work to a background task.
- The frontend's `on()` (src/lib/tauri.ts) subscribes with the current window's label as target. Tauri's default
  target (`Any`) receives every targeted emit, which would make `emit_to` / `emit_for_root` reach every window.

**Phase 0 compatibility.** Six Rust modules still `app.listen(WORKSPACE_CHANGED)` and parse `Option<WorkspaceInfo>`
as "the" workspace: `core/watcher.rs`, `core/fs.rs`, `core/lsp.rs`, `mcp/manager.rs`, `ai/agent/worktrees.rs`,
`index/manager.rs`. `open_workspace_for` / `close_workspace_for` therefore still broadcast `WORKSPACE_CHANGED` app-wide
when the changing window is the focused one (or the only one), so single-window behaviour is unchanged and those
services follow the folder most recently opened in the focused window. Phase 1 moves each of them onto
`on_workspace_change` + `RootRefs` and deletes the app-wide emit.

Per-root resources (`watcher`, `fs` quick-open index, `index::IndexManager`, `mcp::McpManager`, `lsp`,
`ai::agent::worktrees`): `RootRefs` counts windows per root; the resource starts on the first window for a root
(`WorkspaceChange.first_for_root`) and stops when the last one leaves (`last_for_prev_root`). Each keeps a
`DashMap<PathBuf, …>` instead of a single slot. Commands that read them take `window` and look up the window's root.

Window creation (`core/windows.rs::create_window`) *done*: `WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html"))`
with the same options as the `main` entry in `tauri.conf.json` (title bar overlay, hidden title, dark theme,
background colour `#161618`, 1440×920, min 800×500, drag-drop off), positioned +28/+28 from the focused window,
registered before it is built. If a path is given, `open_workspace_for(app, label, path)` runs right after creation so
`workspace_current` already answers when the frontend boots (the window also receives WORKSPACE_CHANGED, whichever
comes first wins idempotently). `WindowEvent::Focused(true)` → `registry.set_focused` + Window menu rebuild;
`WindowEvent::Destroyed` → release the root, unregister, rebuild the menu, and on non-macOS exit when no windows
remain. On macOS `RunEvent::ExitRequested { code: None }` (last window closed) is prevented so the app stays in the
Dock; `RunEvent::Reopen` with no visible window shows the focused window or creates one. `capabilities/default.json`
lists `"w*"`. The Window menu (`menu.rs::rebuild_window_items`) is rebuilt on create / destroy / root change / focus;
its items are `window.focus:<label>` check items.

CLI routing (`core/cli.rs`, phase 1): `route_open(reqs, &registry, setting) -> Vec<(target: Target, reqs)>` where
`Target = Window(label) | NewWindow(path)`; pure and unit-tested. `OpenQueue` is keyed by label; `cli_take_pending_opens(window)`
returns only that window's requests. `on_opened` and `cli_open` call the router, create windows as needed, and emit
`OPEN_PATHS` to the target label. Today `on_opened` broadcasts `OPEN_PATHS` and focuses the focused window (creating
one when none exists).

### Frontend

Each window runs the same bundle; stores are per window automatically (separate webview). Changes:

- `api.workspace.*` unchanged (the calling window is implicit). New `api.window.{new, list, focus, close}`. *done*
- `file.newWindow` → `api.window.new(null)`. `workspace.openFolder` honours `window.openFoldersIn` (pure rule:
  `opensInNewWindow` in src/lib/windowing.ts; `{ newWindow }` overrides it) and `workspace.openRecent` goes through
  it; hidden `workspace.openInNewWindow`, `window.list`, `window.focus`. *done* — Alt modifier on Open Recent: pending
  (the picker reports no modifiers). Window menu entries come from `window_list` (the native menu is rebuilt in Rust).
- Editor session persistence key: `${root}::${label}` with a fallback read of the old `${root}` key, so two windows on
  one folder keep separate tab sets (src/lib/windowing.ts). *done*
- `cliOpen.ts` is unchanged in shape; the backend only hands a window its own requests (phase 1).
- Settings: `window.openFoldersIn: "new" | "current"` (mirrored in `settingsSchema.ts` and Rust `WindowSettings`). *done*
- Automation bridge: `scripts/e2e.mjs eval --window w2 …` targets a label (`dev/automation.rs` takes `?window=`). Phase 1.

## Out of scope for this pass

Drag a tab out into a new window; per-window layout state (panel sizes are still shared through localStorage);
`caret serve` remote clients follow the `main` window only.

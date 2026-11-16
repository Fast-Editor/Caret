# Warren sandboxes

Status: in progress (2026-11-16). Warren (https://github.com/veerareddyvishal144/warren) gives every agent thread a
persistent Linux computer: a gVisor container (or microVM) with a shell, a home directory that survives sleep, exec and
files APIs behind one API key. Caret uses it as a **remote command executor**: the files you edit stay on your Mac and
go through the usual pending-changes review, but the commands the agent runs (installs, builds, tests, scripts,
`rm -rf`) run inside the thread's Warren instance on a synced copy of the workspace. That is how "full access" can be
safe: nothing the agent runs touches the machine Caret is on.

## Behaviour

- **Settings → Sandboxes** (nav id `sandboxes`, aliases `warren` and `sandbox`), three sections:
  - *Connection*: Enable toggle, API URL, *Connect via* (hint: `127.0.0.1:7337` for an SSH tunnel), an API key field
    that calls `warren_set_key` and never shows the stored key (the row only says whether one is stored), and *Test
    connection* → `warren_status`, shown as `Warren <version> · dev mode · wallet $5.00 · 1/3 awake` or the error. The
    section badge reads Off / No key / Connected / Unreachable.
  - *Defaults for new instances*: template, vCPUs / memory / disk, sleep when idle, idle timeout, remote root, sync
    excludes (one comma-separated line; stored as the `syncExcludes` list), auto-approve sandboxed commands, default
    for new threads. All bound to `settings.warren.*` through the settings store, so User/Workspace scope and
    "modified" dots work like everywhere else.
  - *Instances*: every instance in the workspace (name + id, status badge toned by state, template, shape as
    `2 vCPU · 4 GB · 4 GB`, last activity as relative time, disk used) with a per-row terminal button and a menu with
    Start / Stop / Sleep / Restart (greyed out when the state does not allow them), Open terminal and Delete (confirm).
    *New instance* creates one with the defaults (optional name). The list refreshes every 5 s while the page is on
    screen and on `caret://warren/changed`. Below it, *Run a command* picks an instance, runs one command through
    `warren_exec` and shows stdout, stderr, exit code and duration. With Warren disabled or no key stored the section
    shows an empty state instead.
- **Per-thread sandbox**: the composer has a Sandbox picker (cloud icon, `Local` or `Warren · <id>`) next to the
  permission picker; the palette command *Sandbox: Choose where commands run…* (`sandbox.pick`) opens it for the active
  thread. It is disabled with the tooltip "Enable Warren in Settings → Sandboxes" until `warren.enabled` is on. The menu
  offers *Local (this Mac)*, *Warren: new instance* (named after the folder, defaults from Settings) and *Warren:
  existing instance…* (sub-list, running and waking first, then sleeping and stopped). Binding shows a spinner until the
  instance is running. A bound thread also shows a status line (`ready · pushed 2m ago · pulled never`, or the error),
  *Sync now (push)*, *Pull changes*, *Open terminal*, *Unbind (keep instance)* and *Unbind and delete instance* (confirm).
  From then on every `run_terminal_cmd` of that thread runs in the instance: Caret pushes the workspace files that
  changed since the last push (gitignore-aware, plus `warren.syncExcludes`), runs the command in the synced copy under
  `<remoteRoot>/<folder>`, then pulls back files the command created or changed as pending changes (so `npm install`
  output stays remote, but a generated file or a formatter's rewrite shows up in the review list like any other edit).
  The chat store keeps the binding on the loaded thread (`ChatSession.sandbox`); `session-updated` events carry the
  binding's status and sync times while a command runs remotely.
- **Commands in a sandbox skip Caret's destructive-command prompt** (`warren.autoApproveCommands`, default on). The
  denylist still blocks. Tool cards show where a command ran: a small cloud icon in the collapsed header and a
  `[warren <id>]` badge in the expanded card's footer, both from the tool call's `executor` field (`warren:<id>`;
  nothing is shown for local runs).
- **Open terminal** opens the instance's browser terminal (ttyd on port 7681) in a Caret browser tab through a signed URL.
- A thread's instance sleeps on its own (`autoSleep`); the first command after that wakes it. Unbinding a thread
  offers to delete the instance; threads that are deleted with a bound instance leave the instance in place (it is
  listed on the Sandboxes page).

## Settings (`warren`, Rust `WarrenSettings`, mirrored in `settingsSchema.ts`)

| Key | Default | Meaning |
|---|---|---|
| `enabled` | `false` | Master switch; the Sandbox picker and the page's instance view need it. |
| `apiUrl` | `http://api.warren.localhost:7337/v1` | Sandbox API base (Warren's `WARREN_API_URL`). |
| `connectVia` | `null` | `host:port` to open TCP connections to instead of the URL's host, keeping the logical `Host` header. For an SSH tunnel: `127.0.0.1:7337`. Instance hosts (`<id>.<base>`) go through the same address. |
| `apiKeyRef` | `"warren"` | Secret id in `secrets.json` holding the `sk_live_…` key. |
| `template` | `"warren-base"` | Template for new instances. |
| `shape` | `{ cpu: 2, memory: 4, disk: 4 }` | vCPU, GB RAM, GB disk for new instances. |
| `autoSleep` | `true` | New instances sleep when idle. |
| `idleTimeoutSeconds` | `900` | Idle timeout for new instances. |
| `remoteRoot` | `"/home/agent/work"` | Parent of synced workspaces inside an instance. |
| `syncExcludes` | `["node_modules", "target", "dist", "build", ".next", ".venv", "__pycache__", ".cache"]` | Directory names never pushed or pulled, on top of `.gitignore`. `.git` is pushed (so `git status`/`diff` work remotely) but never pulled. |
| `autoApproveCommands` | `true` | Sandboxed commands bypass Caret's Ask gate (Block holds). |
| `defaultForNewThreads` | `false` | New threads start bound to a fresh Warren instance. |

## Commands (Rust `warren_*`, TS `api.warren.*`)

| Command | Args | Result |
|---|---|---|
| `warren_status` | | `WarrenStatus { configured, reachable, version?, devMode?, workspace?: { id, name, walletMicros, awakeLimit, awakeCount }, error? }` |
| `warren_set_key` | `key: string` | stores the secret under `apiKeyRef`, returns `WarrenStatus` |
| `warren_list_instances` | | `WarrenInstance[]` |
| `warren_create_instance` | `opts: { name?, template?, shape?, autoSleep?, idleTimeoutSeconds? }` | `WarrenInstance` (status `provisioning`; the UI polls) |
| `warren_instance_action` | `id, action: "start" \| "stop" \| "sleep" \| "restart" \| "delete"` | `{ id, status }` |
| `warren_get_instance` | `id` | `WarrenInstance` |
| `warren_exec` | `id, command, timeoutSecs?` | `WarrenExecResult { exitCode, stdout, stderr, durationMs, truncated }` |
| `warren_terminal_url` | `id, ttlSecs?` | `{ url, expiresAt }` (signed URL for port 7681) |
| `warren_logs` | `id, tail?` | `{ logs }` |
| `warren_bind_thread` | `sessionId, instanceId?: string \| null` (`null` creates one) | `SandboxBinding` |
| `warren_unbind_thread` | `sessionId, deleteInstance: bool` | `ChatSessionView` |
| `warren_sync` | `sessionId, direction: "push" \| "pull"` | `SyncReport { direction, files, bytes, deleted, ms }` |

`WarrenInstance` (camelCase of Warren's instance object, subset): `id, name, template, templateRevision, status,
statusReason, resources { cpu, memory, disk }, url, defaultPort, mountPath, autoSleep, idleTimeoutSeconds, createdAt,
updatedAt, lastActivityAt, diskUsedBytes, runtime`.

`SandboxBinding` (on `Session`, `ChatSessionView` and `SessionSummary` as `sandbox?`): `{ provider: "warren",
instanceId, instanceName, remoteRoot, lastPushAt?, lastPullAt?, pushedFiles, status: "ready" | "syncing" | "running" |
"error", error? }`. Events: `caret://warren/changed` (instance list changed), `SessionUpdated` when a binding changes.

## Rust layout

- `src-tauri/src/warren/mod.rs`: `WarrenState` (managed; caches a client per settings fingerprint), `pub fn client_for(settings) -> Result<WarrenClient>`, `init(app)`.
- `warren/types.rs`: serde types above plus Warren wire shapes (`CreateInstanceRequest`, `ExecRequest`, `ExecResult`, `SignedUrl`, `FileEntry`, `Billing`, `Workspace`, `Template`). Unknown fields are ignored on read; requests send only set fields (Warren rejects unknown fields).
- `warren/client.rs`: `WarrenClient::new(api_url, api_key, connect_via)`; `health`, `workspace`, `billing`, `templates`, `instances`, `instance`, `create_instance`, `delete_instance`, `start/stop/sleep/restart`, `exec(id, &ExecRequest)`, `signed_url(id, port, ttl)`, `logs`, `wait_running(id, timeout)`; instance-host calls `files_list/read/write/delete/move/download(id, path)` build `http(s)://<id>.<base>[:port]/v1/files…` and send `X-Warren-Key`. With `connect_via` the client pins both the API host and every instance host to that address (reqwest `resolve`/`resolve_to_addrs`), so the `Host` header stays logical. Errors map Warren's `{ error: { code, message } }` to `CaretError` with the Warren code preserved.
- `warren/sync.rs`: `Manifest` (relative path → size, mtime ms), `scan(local_root, excludes) -> Manifest` (gitignore-aware via the `ignore` crate, `.git` included, excludes applied by directory name), `plan_push(prev, now) -> PushPlan { upload, delete }`, `push(client, binding, local_root, plan) -> PushReport` (tar.gz the changed files, `PUT …/files/content?path=.caret-sync/push.tgz`, `exec`: extract into `remoteRoot`, apply deletions, `touch ~/.caret-sync/marker`), `pull(client, binding, local_root, pushed) -> PullReport { changed: Vec<(rel, bytes)>, deleted: Vec<rel> }` (`exec find … -newer marker` filtered by excludes and `.git`, remote `tar czf`, download, extract in memory; deletions = pushed files missing from the remote listing). Pure planning functions are unit-tested; the client calls go through a small trait so tests use a fake.
- `warren/executor.rs`: `CommandExecutor` trait (`run(spec, cancel) -> CmdResult`, `kind()`, `describe()`), `LocalExecutor` (today's `run_shell_with`), `WarrenExecutor { client, binding, local_root }` (push → exec `cd <remote cwd> && <command>` with the request timeout → pull → hands pulled files to the tool layer as pending changes). `ToolContext.executor` is chosen by the runner from the session's `sandbox` binding.
- `warren/commands.rs`: the commands above; registered under `// REGISTER:commands:warren` in `lib.rs`.

## Frontend layout

- `src/lib/api.ts` `warren.*`, `src/types.ts` (`WarrenStatus`, `WarrenInstance`, `WarrenExecResult`, `SandboxBinding`, `SyncReport`, `Settings.warren`), `src/lib/settingsSchema.ts` (DEFAULTS mirror + fields on a `sandboxes` page).
- `src/components/settings/pages/Sandboxes.tsx` (nav id `sandboxes`, aliases `warren`, `sandbox`): connection, status, defaults, instances table with actions, "Run a command" box, "Open terminal".
- `src/components/chat/composer/SandboxPicker.tsx`: Local / Warren (new or existing instance); bound state shows the instance id, sync status, Sync now, Open terminal, Unbind. `src/store/chat.ts` keeps `sandbox` on the loaded thread (from `ChatSessionView`).
- Tool cards (`src/components/chat/messages/Parts.tsx`) show `[warren <id>]` from `ToolOutput.executor`.

## Testing

- Rust unit tests: client URL/Host/header construction with and without `connectVia`, error mapping, manifest scan and push/pull planning (temp dirs), executor command mapping (`cd` + quoting), binding serialisation.
- Rust live tests (ignored unless `WARREN_API_URL`/`WARREN_API_KEY` are set): health, create → exec → files → delete against a real daemon.
- Vitest: settings defaults mirror, picker state helpers, table formatting.
- End to end in the dev build against the development daemon (see "Development setup").

## Development setup

The Warren development host is the Azure VM `azvm` (`ssh azvm`), checkout `/data/warren`, daemon started with
`WARREN_BIN=/data/warren/target/debug/warrend scripts/dev-up.sh --no-build` (data under `/data/warren/data`, log
`/data/warren/warrend-dev.log`). From the Mac: `ssh -N -L 7337:127.0.0.1:7337 azvm`, then Caret settings `apiUrl =
http://api.warren.localhost:7337/v1`, `connectVia = 127.0.0.1:7337`, key from `POST /v1/signup` (dev mode grants $5).
Instances run under Docker on the VM; `*.warren.localhost` resolves there, and through the tunnel the `Host` header
does the routing.

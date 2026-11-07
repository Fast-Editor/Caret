# Remote access: `caret serve`

Use Caret from a browser on another device: a laptop on the same Wi-Fi, an iPad, or any machine on your Tailscale
tailnet. The Mac running Caret keeps owning everything (files, terminals, git, agents, API keys); the browser is a
thin client that gets the full Caret UI over an authenticated WebSocket relay.

```sh
caret serve --host 0.0.0.0          # prints: Caret remote: http://192.168.1.20:17430  pairing code: 482913 (valid 5 min)
caret serve --host 0.0.0.0 --port 18000 --no-window ~/code/project
caret serve --print-url             # just the URL(s), for scripts
```

Or from the app: Settings → **Remote** → *Enable remote access*, pick the bind address and press **Show pairing
code**. The same page lists the URLs, the connected browsers and the paired devices.

## How it works

```
browser (any device)                         Mac running Caret
┌───────────────────────────┐   HTTP + WS    ┌──────────────────────────────────────────────────────┐
│ Caret UI (same Vite dist) │ ─────────────► │ remote::server (hyper)   /, /assets, /__remote/shim.js │
│ + /__remote/shim.js       │ ◄───────────── │ remote::relay (hub)      /ws?role=client&token=…       │
│   window.__TAURI_INTERNALS__              │           ▲ loopback WS  /ws?role=executor&secret=…    │
│   invoke() → {t:"invoke"} frames          │           ▼                                          │
└───────────────────────────┘                │ desktop webview: src/lib/remoteExecutor.ts           │
                                             │   runs the real Tauri command, streams Channels back │
                                             └──────────────────────────────────────────────────────┘
```

1. **Static app.** The server serves the very frontend Tauri embeds (`frontendDist`), so the remote UI is always the
   same version as the desktop. `index.html` is rewritten to load `/__remote/shim.js` first.
2. **Shim.** `src-tauri/remote/shim.js` implements `window.__TAURI_INTERNALS__` (`invoke`, `transformCallback`,
   `unregisterCallback`, `convertFileSrc`, window metadata) and the plugin internals `@tauri-apps/api` reads
   (`__TAURI_EVENT_PLUGIN_INTERNALS__`, `__TAURI_OS_PLUGIN_INTERNALS__`). `isTauri()` is true, so the ordinary
   frontend boots unchanged. Each `invoke()` becomes `{ t: "invoke", id, cmd, args }` on the socket; `Channel`
   arguments serialise to `"__CHANNEL__:<id>"` exactly as in Tauri's IPC.
3. **Relay.** `remote::relay::Hub` forwards client frames to the single executor as
   `{ t: "invoke", client, id, cmd, args, options }` and routes `{ t: "reply", client, id, ok, value | error }` and
   `{ t: "cb", client, callback, payload }` back to the right client. A client disconnect sends
   `{ t: "client_gone", client }` to the executor. With no executor connected the client receives
   `{ ok: false, error: "Caret desktop is not connected" }`. Frames are capped at 32 MB; the server pings every 20 s.
4. **Executor.** `src/lib/remoteExecutor.ts` runs inside the desktop webview and connects to
   `ws://127.0.0.1:<port>/ws?role=executor&secret=<per-process secret>` (the secret comes from the
   `remote_executor_secret` command and is never served over HTTP; the socket is accepted from loopback only). It
   replaces channel placeholders with real `Channel`s, maps `plugin:event|listen` / `unlisten` onto `listen()` per
   client, calls `invoke(cmd, args)` and replies. It refuses `remote_executor_secret`, `remote_pairing_code`,
   `remote_revoke_device`, `remote_set_enabled` and `remote_restart`, so a remote session cannot pair more devices.

Because the relay goes through the desktop webview, every existing Tauri command works remotely without a second
implementation, and the desktop's state (open workspace, chat sessions, terminals) is what the browser sees.

## Pairing and tokens

* `GET /` without a token serves the pairing page. Settings → Remote → **Show pairing code** (or the `caret serve`
  banner) gives a six digit code: valid five minutes, single use, at most five attempts per minute per IP (HTTP 429
  after that, 403 for a wrong or expired code).
* `POST /pair { code, device }` returns a random 256-bit token. The page stores it as the `caret_token` cookie
  (`SameSite=Lax`) and in `localStorage`, then reloads into the app. `/?token=…` also works once and sets the cookie.
* Only the SHA-256 of each token is kept, in `~/.caret/remote/devices.json` (mode 0600):
  `{ id, name, token_hash, created_at, last_seen, user_agent }`. **Revoke** on the settings page deletes the record
  and closes that device's sockets.
* `GET /health` → `{ ok, name: "caret", version, paired }` needs no token and is safe to poll.

## Settings and commands

`Settings.remote` (`~/.caret/settings.json`):

```json
{ "remote": { "enabled": false, "host": "127.0.0.1", "port": 17430 } }
```

`caret serve --host/--port` override the settings for that process. Commands (`api.remote.*`): `remote_status`,
`remote_pairing_code`, `remote_revoke_device(id)`, `remote_executor_secret`, `remote_is_serving`,
`remote_set_enabled(enabled)`, `remote_restart`. The backend emits `caret://remote/status` whenever the server, the
executor or the client set changes.

## Tailscale recipe

1. Install Tailscale on the Mac and on the device you want to use; sign both into the same tailnet.
2. On the Mac: Settings → Remote → bind address **0.0.0.0**, enable, **Show pairing code** (or
   `caret serve --host 0.0.0.0`). The URL list includes the tailnet address (`http://100.x.y.z:17430`).
3. On the other device open that URL, enter the code, done. Nothing is exposed beyond your tailnet; Tailscale
   encrypts the link, which makes up for the plain-HTTP transport.

Without Tailscale, prefer `--host 127.0.0.1` plus an SSH tunnel (`ssh -L 17430:127.0.0.1:17430 mac`), or stay on a
network you trust.

## Limitations

* **The desktop app must keep running and its window must exist.** The executor lives in the main webview; with
  `--no-window` the window is hidden but not closed. Closing Caret ends every remote session.
* **Plain HTTP.** No TLS is built in; use Tailscale, an SSH tunnel or a reverse proxy with certificates.
* **One desktop, one executor.** A browser sees the desktop's workspace and sessions; two people working in the same
  Caret see and edit the same state. Opening a different folder in the browser changes it for the desktop too.
* **Some window-level actions stay on the desktop.** Commands such as opening devtools, hiding or dragging the window
  are turned into no-ops in the browser; native menus, the window title and macOS traffic lights belong to the
  desktop. Zoom in the browser uses CSS zoom. Native file dialogs open on the Mac, not in the browser.
* The clipboard uses the browser's clipboard API when available (secure context or `localhost`); otherwise copy and
  paste go through the desktop clipboard.
* Anyone with a paired token has the same power as the person at the keyboard. Pair devices you own, revoke the ones
  you stop using, and keep the port off the public internet.

## Files

| Where | What |
| --- | --- |
| `src-tauri/src/remote/mod.rs` | `caret serve` argv, `RemoteManager` (start/stop/status), Tauri commands, embedded asset source |
| `src-tauri/src/remote/server.rs` | hyper server: routes, static assets with shim injection, WebSocket upgrade, ping loop, LAN URLs |
| `src-tauri/src/remote/relay.rs` | the frame router between clients and the executor |
| `src-tauri/src/remote/pairing.rs` | pairing codes (TTL, one-time, rate limit) and `devices.json` |
| `src-tauri/remote/shim.js`, `pair.html` | browser shim and pairing page (embedded with `include_str!`) |
| `src/lib/remoteExecutor.ts` | desktop executor (channel/event mapping, reconnect, command filter) |
| `src/components/remote/index.tsx` | starts the executor in the desktop; shows the "Remote session" strip in a browser |
| `src/components/settings/pages/Remote.tsx` | the settings page |

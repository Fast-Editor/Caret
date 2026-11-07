- Remote access: `caret serve [--host 0.0.0.0] [--port N] [--no-window] [--print-url]` and Settings → **Remote** serve
  the full Caret UI to a browser on your LAN or over Tailscale. The Mac keeps running everything; the browser gets
  the same Vite bundle with a shim that relays every `invoke()` (including Channels and events) through the desktop
  webview over an authenticated WebSocket, so every existing command works remotely without a second implementation.
  Browsers pair with a six digit code (five minutes, one use, five attempts per minute per address); tokens are stored
  hashed in `~/.caret/remote/devices.json` and can be revoked from the settings page. New settings `remote.enabled`,
  `remote.host`, `remote.port`; new commands `remote_status`, `remote_pairing_code`, `remote_revoke_device`,
  `remote_executor_secret`, `remote_is_serving`, `remote_set_enabled`, `remote_restart`. See docs/REMOTE.md.

# Language servers

Caret talks to standard LSP servers over stdio. The list lives in settings under `lsp.servers` (Settings → JSON); the
built-in entries cover TypeScript/JavaScript, Rust, Python, Go, C/C++, JSON, CSS, HTML, YAML, Bash, Lua, Java, Ruby,
Zig and TOML. A server starts lazily the first time a file of its language is opened and stops when the folder changes.

## Where the binary comes from

For each server `command`, in order:

1. An explicit path (`~/tools/bin/rust-analyzer`) is used as given.
2. `<workspace>/node_modules/.bin/<command>`: a repository that pins its own language server wins over a global one.
3. `~/.caret/lsp/node_modules/.bin/<command>`: servers Caret installed itself (below).
4. The augmented PATH (Homebrew, `~/.local/bin`, `~/.cargo/bin`, nvm/fnm/volta/bun/pnpm) and then plain `which`.

If a saved explicit path no longer exists (a deleted checkout, a moved toolchain) Caret falls back to the built-in
command name for that server and logs a warning, so the language does not silently lose its server.

## One-click install

Servers published on npm carry an `npm` field (TypeScript: `typescript-language-server typescript`, Python: `pyright`,
JSON/CSS/HTML: `vscode-langservers-extracted`, YAML: `yaml-language-server`, Bash: `bash-language-server`). When a file
needs one of them and no binary is found, Caret shows a notification with **Install**; the status bar's LSP popover has
the same button. The install runs `npm install <packages>@latest` with `~/.caret/lsp` as the prefix (Node.js 18+ and
npm must be on the PATH), then starts the server for the open files. Nothing is installed globally and the workspace
is not touched.

Set `lsp.offerInstall` to `false` to stop the prompt; the status-bar button stays. Servers that are not on npm
(rust-analyzer, gopls, clangd, jdtls, …) show their install hint instead.

## Troubleshooting

- The status bar shows `LSP n/m`; open it to see each server's state, the resolved binary and errors.
- The log (`~/.caret/logs`) has `lsp: installing language server`, `lsp: language server installed`, and
  `lsp: configured server path is gone; using the default command`.
- `lsp_configs` (with the workspace root) reports `installed`, `resolved` and `installable` per server; the automation
  bridge exposes it as `window.__caret.api.lsp.configs(root)`.

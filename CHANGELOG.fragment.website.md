# Changelog fragment: website, compare page and packaging

Bullets to fold into CHANGELOG.md under the next version.

- Website: new `/compare` page with a grouped Caret / T3 Code / Cursor parity table (editor, agents and subscriptions, threads and workflow, safety, remote, footprint), the measured release-build numbers, and links from the header, footer and hero.
- Website: the External agents card and the subscription FAQ now list Claude Code, Codex, Cursor, Grok Build, Gemini CLI, OpenCode and Pi, and mention the signed-in state, Sign in button and second account per provider in Settings → Agents.
- Website: the download grid shows `winget` and `scoop` commands on the Windows tab and the AUR and Nix commands on the Linux tab.
- Packaging: manifests for winget (`packaging/winget`, schema 1.6, `FastEditor.Caret`), scoop (`packaging/scoop/caret.json` with `checkver` and `autoupdate`), the AUR (`packaging/aur`, `caret-bin` from the `.deb`, `provides=('caret')`) and a Nix flake (`flake.nix`, `nix run github:Fast-Editor/Caret` on aarch64-darwin and x86_64-linux).
- Release: `scripts/update-packaging.mjs` (`npm run release:packaging`) rewrites the version and sha256 fields in every manifest from `SHA256SUMS.txt`; a new `packaging` job in the Release workflow runs it after `checksums` and commits the result to main.
- Docs: new `docs/INSTALL.md` covering every install channel and which ones are published today; README install section and `docs/RELEASING.md` "Package managers" updated to match.

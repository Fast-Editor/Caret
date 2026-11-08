export const site = {
  name: "Caret",
  tagline: "A Cursor-class AI code editor with a Rust core.",
  description:
    "Tauri 2 + Rust on the backend, React 19 + Monaco in the system WebView. No bundled Chromium, no Node at run time. Bring your own keys; nothing leaves your machine except model calls.",
  url: process.env.NEXT_PUBLIC_SITE_URL ?? "https://caret-lynkrteam.vercel.app",
  repo: "https://github.com/Fast-Editor/Caret",
  releases: "https://github.com/Fast-Editor/Caret/releases",
  latest: "https://github.com/Fast-Editor/Caret/releases/latest",
  issues: "https://github.com/Fast-Editor/Caret/issues",
  lynkr: "https://github.com/Fast-Editor/lynkr",
  productHunt:
    "https://www.producthunt.com/products/caret-9?embed=true&utm_source=badge-featured&utm_medium=badge&utm_campaign=badge-caret-12",
  productHuntBadge:
    "https://api.producthunt.com/widgets/embed-image/v1/featured.svg?post_id=1296357&theme=dark&t=1793218129641",
  installCommand: "curl -fsSL https://raw.githubusercontent.com/Fast-Editor/Caret/main/scripts/install.sh | sh",
  brewCommand: "brew tap fast-editor/caret && brew trust fast-editor/caret && brew install --cask caret",
  quarantineCommand: "xattr -dr com.apple.quarantine /Applications/Caret.app",
  wingetCommand: "winget install FastEditor.Caret",
  scoopCommand: "scoop install https://raw.githubusercontent.com/Fast-Editor/Caret/main/packaging/scoop/caret.json",
  aurCommand: "yay -S caret-bin",
  nixCommand: "nix run github:Fast-Editor/Caret",
};

export const nav = [
  { href: "/docs", label: "Docs" },
  { href: "/compare", label: "Compare" },
  { href: "/changelog", label: "Changelog" },
  { href: "/download", label: "Download" },
];

export const stats = [
  { value: "~110 MB", label: "Rust core RSS, idle with a workspace open" },
  { value: "~330 ms", label: "first paint" },
  { value: "8 ms", label: "quick open" },
  { value: "33 ms", label: "project search" },
];

export const comparison = [
  { aspect: "UI runtime", electron: "Bundled Chromium, 150–300 MB baseline", caret: "OS WebView (WKWebView, WebView2, WebKitGTK)" },
  { aspect: "Backend", electron: "Node.js extension hosts", caret: "One Rust process (tokio), mimalloc" },
  { aspect: "Search", electron: "ripgrep subprocess", caret: "grep + ignore crates in-process, the ripgrep engine" },
  { aspect: "Index", electron: "Separate indexer", caret: "tantivy, memory-mapped, optional embeddings" },
  { aspect: "Terminal", electron: "node-pty", caret: "portable-pty with bounded ring buffers" },
  { aspect: "Git", electron: "git CLI or libgit2 via Node", caret: "libgit2 in-process for reads, git CLI for network" },
];

export type Feature = { icon: string; title: string; body: string };
export const features: Feature[] = [
  { icon: "sparkles", title: "Tab", body: "Multi-line ghost-text completions from any configured model, partial accept, next-edit prediction, context from recent edits, open files and diagnostics." },
  { icon: "wand", title: "⌘K everywhere", body: "Inline edit or generate with a streaming diff you accept or reject. Quick questions on a selection. In the terminal, natural language becomes a shell command." },
  { icon: "bot", title: "Agent pane", body: "Agent, Ask, Plan and Debug modes with streaming tool calls, approvals, checkpoints, per-file review, queued messages, context summarisation and live cost." },
  { icon: "at", title: "@-mentions", body: "Files, folders, code, codebase search, git diffs and commits, web, docs, terminals, lint errors, past chats, rules, MCP resources, skills and images." },
  { icon: "book", title: "Rules and knowledge", body: ".caret rules, .cursorrules, AGENTS.md and CLAUDE.md, memories, hooks, slash commands, skills, subagents and plans." },
  { icon: "search", title: "Codebase index", body: "Ignore-aware walker, language-aware chunking, BM25 with tantivy plus optional embeddings via Ollama or any OpenAI-compatible endpoint, hybrid retrieval." },
  { icon: "plug", title: "MCP", body: "stdio, streamable HTTP and SSE servers from .caret/mcp.json or Cursor's config, with tools, resources, prompts, per-tool enable and logs." },
  { icon: "users", title: "External agents", body: "Run Claude Code, Codex, Cursor, Grok Build, Gemini CLI, OpenCode, Pi or any Agent Client Protocol agent per chat, with its model, effort and permission mode in the composer." },
  { icon: "route", title: "Auto routing", body: "A managed Lynkr sidecar scores each request and picks one of your own models per tier, with session pins and fallback. Every reply shows which model answered." },
  { icon: "shield", title: "Command safety", body: "Every agent shell command is parsed and classified before it runs. Destructive commands always ask, blocked ones never run, approved ones run under Seatbelt or bubblewrap." },
  { icon: "bug", title: "Run and Debug", body: "A Debug Adapter Protocol client with breakpoints, stepping, variables, watch and a debug console for Node, TypeScript and Python, launch.json compatible." },
  { icon: "terminal", title: "Semantic terminal", body: "Command blocks, program status, clickable paths, ghost-text suggestions from history and the model, a fix-it chip on failed commands, and cross-session history search." },
  { icon: "code", title: "Editor and LSP", body: "Monaco with split groups, diff editor, breadcrumbs and outline, plus a lean LSP client for TypeScript, Rust, Python, Go, C and C++, Java and more." },
  { icon: "git", title: "Source control", body: "Commit UI with AI messages, branches, history, stashes, worktrees, git gutter and blame, and a Bugbot-style review of the working tree, staged changes, a branch or a commit." },
  { icon: "cursor", title: "Cursor compatible", body: "Reads .cursor rules, mcp.json, hooks, commands, skills, agents and ignore files, so existing projects work unchanged." },
];

export const faq = [
  {
    q: "macOS says Caret is damaged and can't be opened.",
    a: "Builds are ad-hoc signed until a Developer ID is configured, so a browser-downloaded copy is quarantined. Run `xattr -dr com.apple.quarantine /Applications/Caret.app` once, or use System Settings → Privacy & Security → Open Anyway. The install script and the Homebrew cask skip the dialog, and later updates install from inside the app.",
  },
  {
    q: "Where do my keys and code go?",
    a: "Keys live in ~/.caret/secrets.json on your machine. Caret calls the model providers you configure and nothing else; there is no Caret server in the path. Ollama and other local servers work fully offline.",
  },
  {
    q: "Does it support VS Code extensions?",
    a: "Not yet. Language support comes from a built-in LSP client, debugging from DAP adapters, and tools from MCP servers and ACP agents. An extension host without Chromium is planned.",
  },
  {
    q: "Can I use my Claude Code or Codex subscription?",
    a: "Yes, and your Cursor and Grok Build subscriptions too. Pick Claude Code, Codex, Cursor, Grok Build, Gemini CLI, OpenCode, Pi or any ACP agent from the composer. The agent runs locally with its own login: Settings → Agents shows the signed-in state of each one with a Sign in button, and supports a second account per provider, for example a work plan next to a personal one. Its text, tool calls, permission prompts, diffs and terminals flow through Caret's UI.",
  },
  {
    q: "What about local models?",
    a: "Any OpenAI-compatible server works as a provider, including Ollama, LM Studio and vLLM, for chat, Tab completions and embeddings.",
  },
  {
    q: "Windows and Linux?",
    a: "Installers for both are attached to every release and built by the same workflow as macOS, but they have had far less runtime testing. Expect rough edges and please report them.",
  },
];

export type ParityStatus = "yes" | "partial" | "no";
export type ParityRow = { group: string; feature: string; caret: ParityStatus; t3: ParityStatus; cursor: ParityStatus; note?: string };

/** Feature parity against T3 Code and Cursor, rendered on /compare. Rows describe shipped behaviour. */
export const parity: ParityRow[] = [
  // Editor
  { group: "Editor", feature: "Code editor with LSP", caret: "yes", t3: "no", cursor: "yes", note: "T3 Code is a harness control surface without an editor by design." },
  { group: "Editor", feature: "Tab completions", caret: "yes", t3: "no", cursor: "yes" },
  { group: "Editor", feature: "Inline edit (Cmd K) with streaming diff", caret: "yes", t3: "no", cursor: "yes" },
  { group: "Editor", feature: "Debugger (DAP)", caret: "yes", t3: "no", cursor: "yes" },
  { group: "Editor", feature: "Terminal with command blocks and AI fix-it", caret: "yes", t3: "partial", cursor: "partial" },
  { group: "Editor", feature: "Codebase index with local embeddings", caret: "yes", t3: "no", cursor: "yes" },
  // Agents and subscriptions
  { group: "Agents and subscriptions", feature: "Built-in agent with your own API keys", caret: "yes", t3: "no", cursor: "partial", note: "Cursor routes through its own backend." },
  { group: "Agents and subscriptions", feature: "Claude Code subscription", caret: "yes", t3: "yes", cursor: "no" },
  { group: "Agents and subscriptions", feature: "ChatGPT subscription via Codex", caret: "yes", t3: "yes", cursor: "no" },
  { group: "Agents and subscriptions", feature: "Cursor subscription", caret: "yes", t3: "yes", cursor: "yes" },
  { group: "Agents and subscriptions", feature: "Grok Build", caret: "yes", t3: "yes", cursor: "no" },
  { group: "Agents and subscriptions", feature: "Gemini CLI, OpenCode, Pi", caret: "yes", t3: "partial", cursor: "no" },
  { group: "Agents and subscriptions", feature: "Any Agent Client Protocol agent", caret: "yes", t3: "yes", cursor: "no" },
  { group: "Agents and subscriptions", feature: "Local models (Ollama, LM Studio, vLLM)", caret: "yes", t3: "no", cursor: "partial" },
  { group: "Agents and subscriptions", feature: "Auto routing across your own models", caret: "yes", t3: "no", cursor: "partial" },
  { group: "Agents and subscriptions", feature: "Second account per provider (separate config home)", caret: "yes", t3: "yes", cursor: "no" },
  // Threads and workflow
  { group: "Threads and workflow", feature: "Pin, snooze, settle, archive with undo", caret: "yes", t3: "yes", cursor: "no" },
  { group: "Threads and workflow", feature: "Queue or steer while a turn runs", caret: "yes", t3: "yes", cursor: "partial" },
  { group: "Threads and workflow", feature: "Permission modes per thread and per project", caret: "yes", t3: "yes", cursor: "yes" },
  { group: "Threads and workflow", feature: "Provider handoff mid-thread and forks", caret: "yes", t3: "yes", cursor: "no" },
  { group: "Threads and workflow", feature: "Multi-model fan-out, one worktree each", caret: "yes", t3: "yes", cursor: "yes" },
  { group: "Threads and workflow", feature: "Worktree per thread with cleanup policies", caret: "yes", t3: "yes", cursor: "partial" },
  { group: "Threads and workflow", feature: "Usage page across providers", caret: "yes", t3: "yes", cursor: "partial" },
  { group: "Threads and workflow", feature: "Desktop notifications", caret: "yes", t3: "yes", cursor: "yes" },
  { group: "Threads and workflow", feature: "Orchestrator MCP endpoint", caret: "yes", t3: "yes", cursor: "no" },
  // Safety
  { group: "Safety", feature: "Shell command classifier (AST) before every run", caret: "yes", t3: "no", cursor: "no" },
  { group: "Safety", feature: "OS sandbox (Seatbelt, bubblewrap) around agent commands, including external agents", caret: "yes", t3: "no", cursor: "partial" },
  { group: "Safety", feature: "Verified diffs after each turn", caret: "yes", t3: "no", cursor: "no" },
  // Remote
  { group: "Remote", feature: "Headless server with web client (LAN or Tailscale)", caret: "yes", t3: "yes", cursor: "no" },
  { group: "Remote", feature: "Mobile app", caret: "no", t3: "yes", cursor: "partial" },
  { group: "Remote", feature: "Hosted relay or cloud agents", caret: "no", t3: "yes", cursor: "yes" },
  // Footprint
  { group: "Footprint", feature: "No bundled Chromium, no Node at run time", caret: "yes", t3: "no", cursor: "no" },
  { group: "Footprint", feature: "Open source", caret: "yes", t3: "yes", cursor: "no" },
];

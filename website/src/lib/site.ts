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
  brewCommand: "brew install --cask fast-editor/lynkr/caret",
  quarantineCommand: "xattr -dr com.apple.quarantine /Applications/Caret.app",
};

export const nav = [
  { href: "/docs", label: "Docs" },
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
  { icon: "users", title: "External agents", body: "Run Claude Code, Codex, Gemini CLI, OpenCode, Pi or any Agent Client Protocol agent per chat, with its model, effort and permission mode in the composer." },
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
    a: "Yes. Pick Claude Code, Codex, Gemini CLI, OpenCode or any ACP agent from the composer. The agent runs locally with its own login; its text, tool calls, permission prompts, diffs and terminals flow through Caret's UI.",
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

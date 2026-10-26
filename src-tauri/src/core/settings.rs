//! Settings schema (mirrors src/types.ts `Settings`), persistence, scoped merge and secrets.
use crate::error::{CaretError, Result};
use crate::events;
use crate::paths;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, State};

// ───────────────────────── schema ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind { Anthropic, Openai, OpenaiCompatible, AzureOpenai, Ollama, Openrouter, Gemini, AnthropicCompatible }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub api_key_ref: Option<String>,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub headers: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")] pub api_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub auth_style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub enable_fallbacks: Option<bool>,
}
impl Default for ProviderConfig {
    fn default() -> Self { Self { id: String::new(), kind: ProviderKind::OpenaiCompatible, name: String::new(), base_url: None, api_key_ref: None, enabled: true, headers: None, api_version: None, auth_style: None, enable_fallbacks: None } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ModelConfig {
    pub id: String,
    pub provider_id: String,
    pub model: String,
    pub display_name: String,
    pub context_window: u32,
    pub max_output: u32,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_thinking: bool,
    pub supports_effort: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub input_price_per_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub output_price_per_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub cache_read_price_per_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub cache_write_price_per_m: Option<f64>,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub is_default: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")] pub is_fast: Option<bool>,
}
impl Default for ModelConfig {
    fn default() -> Self { Self { id: String::new(), provider_id: String::new(), model: String::new(), display_name: String::new(), context_window: 128_000, max_output: 16_000, supports_tools: true, supports_vision: false, supports_thinking: false, supports_effort: false, input_price_per_m: None, output_price_per_m: None, cache_read_price_per_m: None, cache_write_price_per_m: None, enabled: true, is_default: None, tags: None, is_fast: None } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TabSettings {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub model_id: Option<String>,
    pub debounce_ms: u64,
    pub max_lines: u32,
    pub partial_accepts: bool,
    pub next_edit_prediction: bool,
    pub suggest_in_comments: bool,
    pub disabled_languages: Vec<String>,
    pub context_lines: u32,
    pub cache_size: u32,
}
impl Default for TabSettings {
    fn default() -> Self { Self { enabled: true, model_id: None, debounce_ms: 250, max_lines: 12, partial_accepts: true, next_edit_prediction: true, suggest_in_comments: true, disabled_languages: vec!["plaintext".into(), "markdown".into(), "log".into()], context_lines: 80, cache_size: 200 } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentSettings {
    pub default_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub default_model_id: Option<String>,
    pub auto_run: String,
    pub command_allowlist: Vec<String>,
    pub command_denylist: Vec<String>,
    pub protect_dotfiles: bool,
    pub protect_delete: bool,
    pub protect_outside_workspace: bool,
    pub auto_fix_lints: bool,
    pub auto_apply_edits: bool,
    pub max_iterations: u32,
    pub max_tool_output_chars: u32,
    pub web_search_enabled: bool,
    pub web_search_provider: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub web_search_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub web_search_api_key_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub subagent_model_id: Option<String>,
    pub summarize_threshold_ratio: f32,
    pub chime_on_finish: bool,
    pub thinking: String,
    pub effort: String,
    pub command_timeout_secs: u64,
    pub mcp_auto_run: bool,
    pub auto_run_mcp_tools: Vec<String>,
    pub large_context_mode: bool,
    pub parallel_tool_calls: bool,
}
impl Default for AgentSettings {
    fn default() -> Self { Self {
        default_mode: "agent".into(), default_model_id: None, auto_run: "allowlist".into(),
        command_allowlist: vec!["ls".into(), "cat".into(), "pwd".into(), "git status".into(), "git diff".into(), "git log".into(), "rg".into(), "grep".into(), "find".into(), "echo".into(), "wc".into(), "head".into(), "tail".into(), "npm test".into(), "npm run".into(), "cargo check".into(), "cargo test".into(), "cargo build".into(), "pytest".into(), "go test".into(), "node".into(), "python3".into(), "tsc".into()],
        command_denylist: vec!["rm -rf /".into(), "sudo".into(), "shutdown".into(), "reboot".into(), "mkfs".into(), "dd if=".into(), "git push --force".into(), ":(){".into()],
        protect_dotfiles: true, protect_delete: true, protect_outside_workspace: true, auto_fix_lints: true, auto_apply_edits: true,
        max_iterations: 60, max_tool_output_chars: 30_000, web_search_enabled: true, web_search_provider: "duckduckgo".into(), web_search_endpoint: None, web_search_api_key_ref: None,
        subagent_model_id: None, summarize_threshold_ratio: 0.8, chime_on_finish: false, thinking: "auto".into(), effort: "high".into(), command_timeout_secs: 120,
        mcp_auto_run: false, auto_run_mcp_tools: vec![], large_context_mode: false, parallel_tool_calls: true,
    } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexingSettings {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub embeddings_provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub embeddings_model: Option<String>,
    pub max_file_size_kb: u32,
    pub exclude_globs: Vec<String>,
    pub chunk_lines: u32,
    pub chunk_overlap: u32,
    pub auto_index_new_folders: bool,
    pub hybrid_alpha: f32,
    pub git_graph_indexing: bool,
}
impl Default for IndexingSettings {
    fn default() -> Self { Self { enabled: true, embeddings_provider_id: None, embeddings_model: None, max_file_size_kb: 512, exclude_globs: vec!["**/node_modules/**".into(), "**/target/**".into(), "**/dist/**".into(), "**/build/**".into(), "**/.git/**".into(), "**/*.min.js".into(), "**/*.lock".into(), "**/package-lock.json".into(), "**/*.map".into()], chunk_lines: 60, chunk_overlap: 10, auto_index_new_folders: true, hybrid_alpha: 0.5, git_graph_indexing: false } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UiSettings {
    pub theme: String,
    pub font_family: String,
    pub font_size: u32,
    pub line_height: f32,
    pub tab_size: u32,
    pub insert_spaces: bool,
    pub word_wrap: String,
    pub minimap: bool,
    pub render_whitespace: String,
    pub cursor_blinking: String,
    pub cursor_style: String,
    pub bracket_pair_colorization: bool,
    pub format_on_save: bool,
    pub auto_save: String,
    pub auto_save_delay_ms: u64,
    pub chat_font_size: u32,
    pub sidebar_position: String,
    pub chat_position: String,
    pub terminal_font_family: String,
    pub terminal_font_size: u32,
    pub terminal_scrollback: u32,
    #[serde(default = "default_terminal_cursor_style")] pub terminal_cursor_style: String,
    #[serde(default = "default_true")] pub terminal_cursor_blink: bool,
    pub show_status_bar_memory: bool,
    pub layout: String,
    pub reduce_motion: bool,
    pub sticky_scroll: bool,
    pub line_numbers: String,
    pub smooth_scrolling: bool,
    pub font_ligatures: bool,
    pub editor_tabs_mode: String,
    pub preview_tabs: bool,
    pub breadcrumbs: bool,
    pub zoom_level: f32,
    pub confirm_delete: bool,
    pub compact_folders: bool,
}
fn default_terminal_cursor_style() -> String { "block".into() }
fn default_true() -> bool { true }

impl Default for UiSettings {
    fn default() -> Self { Self { theme: "caret-dark".into(), font_family: "JetBrains Mono, Menlo, Monaco, 'Courier New', monospace".into(), font_size: 13, line_height: 1.6, tab_size: 2, insert_spaces: true, word_wrap: "off".into(), minimap: true, render_whitespace: "selection".into(), cursor_blinking: "smooth".into(), cursor_style: "line".into(), bracket_pair_colorization: true, format_on_save: false, auto_save: "afterDelay".into(), auto_save_delay_ms: 1000, chat_font_size: 13, sidebar_position: "left".into(), chat_position: "right".into(), terminal_font_family: "JetBrains Mono, Menlo, monospace".into(), terminal_font_size: 12, terminal_scrollback: 5000, terminal_cursor_style: "block".into(), terminal_cursor_blink: true, show_status_bar_memory: true, layout: "editor".into(), reduce_motion: false, sticky_scroll: true, line_numbers: "on".into(), smooth_scrolling: true, font_ligatures: true, editor_tabs_mode: "multiple".into(), preview_tabs: true, breadcrumbs: true, zoom_level: 0.0, confirm_delete: true, compact_folders: true } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrivacySettings { pub privacy_mode: bool, pub allow_web_tools: bool, pub allow_remote_embeddings: bool }
impl Default for PrivacySettings { fn default() -> Self { Self { privacy_mode: false, allow_web_tools: true, allow_remote_embeddings: true } } }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneralSettings {
    #[serde(skip_serializing_if = "Option::is_none")] pub default_shell: Option<String>,
    pub open_last_workspace: bool,
    pub confirm_quit_with_running_agents: bool,
    pub git_auto_fetch: bool,
    pub git_auto_fetch_interval_secs: u64,
    pub global_ignore: Vec<String>,
    pub trusted_workspaces: Vec<String>,
    pub show_welcome: bool,
    pub use_cursor_compat_dirs: bool,
}
impl Default for GeneralSettings {
    fn default() -> Self { Self { default_shell: None, open_last_workspace: true, confirm_quit_with_running_agents: true, git_auto_fetch: false, git_auto_fetch_interval_secs: 180, global_ignore: vec![".DS_Store".into(), "Thumbs.db".into()], trusted_workspaces: vec![], show_welcome: true, use_cursor_compat_dirs: true } }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomMode {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub description: Option<String>,
    pub instructions: String,
    /// `"all"`, `"readonly"`, or a list of tool names.
    pub tools: Value,
    #[serde(skip_serializing_if = "Option::is_none")] pub auto_run: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")] pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub keybinding: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BetaSettings { pub browser_tools: bool, pub voice: bool, pub next_edit_prediction: bool, pub background_agents: bool, pub design_mode: bool }
impl Default for BetaSettings { fn default() -> Self { Self { browser_tools: true, voice: false, next_edit_prediction: true, background_agents: true, design_mode: false } } }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LspServerConfig {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub languages: Vec<String>,
    pub extensions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub initialization_options: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")] pub settings: Option<Value>,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub root_patterns: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")] pub env: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")] pub install_hint: Option<String>,
}
impl Default for LspServerConfig {
    fn default() -> Self { Self { id: String::new(), name: String::new(), command: String::new(), args: vec![], languages: vec![], extensions: vec![], initialization_options: None, settings: None, enabled: true, root_patterns: None, env: None, install_hint: None } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LspSettings { pub enabled: bool, pub servers: Vec<LspServerConfig> }
impl Default for LspSettings { fn default() -> Self { Self { enabled: true, servers: default_lsp_servers() } } }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct McpSettings { pub enabled: bool }
impl Default for McpSettings { fn default() -> Self { Self { enabled: true } } }


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VerifySettings {
    pub auto_run_after_turn: bool,
    pub run_tests: bool,
    pub mutation_testing: bool,
    pub mutation_max_mutants: u32,
    pub dependency_grounding: bool,
    pub security_rules: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub project_check_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub test_command: Option<String>,
    /// Run before the test command (e.g. a build) — auto-detected for compiled-output test suites when unset.
    #[serde(skip_serializing_if = "Option::is_none")] pub pre_test_command: Option<String>,
    pub timeout_secs: u64,
    pub block_merge_on_fail: bool,
    pub use_semgrep_if_available: bool,
}
impl Default for VerifySettings {
    fn default() -> Self { Self { auto_run_after_turn: true, run_tests: true, mutation_testing: true, mutation_max_mutants: 12, dependency_grounding: true, security_rules: true, project_check_command: None, test_command: None, pre_test_command: None, timeout_secs: 300, block_merge_on_fail: false, use_semgrep_if_available: true } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SandboxSettings {
    /// off | auto (sandbox when available) | strict (refuse to run without sandbox)
    pub mode: String,
    pub allow_network: bool,
    pub extra_writable_paths: Vec<String>,
    pub allow_read_home: bool,
    pub pin_mcp_tool_descriptions: bool,
}
impl Default for SandboxSettings {
    fn default() -> Self { Self { mode: "auto".into(), allow_network: true, extra_writable_paths: vec![], allow_read_home: true, pin_mcp_tool_descriptions: true } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AcpSettings {
    pub enabled: bool,
    /// 'caret' or an ACP agent id used for new sessions.
    pub default_agent_id: String,
    pub agents: Vec<crate::acp::AcpAgentConfig>,
    pub pass_mcp_servers: bool,
    /// agent id → { config option id → value } remembered from the composer's pickers (model, effort, …).
    pub session_defaults: BTreeMap<String, BTreeMap<String, Value>>,
}
impl Default for AcpSettings {
    fn default() -> Self { Self { enabled: true, default_agent_id: "caret".into(), agents: vec![], pass_mcp_servers: true, session_defaults: BTreeMap::new() } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TerminalSettings {
    pub blocks: bool,
    pub shell_integration: bool,
    pub ai_enabled: bool,
    pub program_status: bool,
    pub max_blocks: u32,
}
impl Default for TerminalSettings {
    fn default() -> Self { Self { blocks: true, shell_integration: true, ai_enabled: true, program_status: true, max_blocks: 200 } }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub providers: Vec<ProviderConfig>,
    pub models: Vec<ModelConfig>,
    pub tab: TabSettings,
    pub agent: AgentSettings,
    pub indexing: IndexingSettings,
    pub ui: UiSettings,
    pub privacy: PrivacySettings,
    pub general: GeneralSettings,
    pub user_rules: String,
    pub lsp: LspSettings,
    pub mcp: McpSettings,
    pub custom_modes: Vec<CustomMode>,
    pub beta: BetaSettings,
    pub keybindings: BTreeMap<String, Option<String>>,
    pub verify: VerifySettings,
    pub sandbox: SandboxSettings,
    pub acp: AcpSettings,
    pub terminal: TerminalSettings,
}
impl Default for Settings {
    fn default() -> Self {
        Self { providers: default_providers(), models: default_models(), tab: Default::default(), agent: Default::default(), indexing: Default::default(), ui: Default::default(), privacy: Default::default(), general: Default::default(), user_rules: String::new(), lsp: Default::default(), mcp: Default::default(), custom_modes: vec![], beta: Default::default(), keybindings: BTreeMap::new(), verify: Default::default(), sandbox: Default::default(), acp: Default::default(), terminal: Default::default() }
    }
}

impl Settings {
    pub fn model(&self, id: &str) -> Option<&ModelConfig> { self.models.iter().find(|m| m.id == id) }
    pub fn provider(&self, id: &str) -> Option<&ProviderConfig> { self.providers.iter().find(|p| p.id == id) }
    /// Default model: explicit agent default → model flagged default → first enabled.
    pub fn default_model(&self) -> Option<&ModelConfig> {
        self.agent.default_model_id.as_deref().and_then(|id| self.model(id)).filter(|m| m.enabled)
            .or_else(|| self.models.iter().find(|m| m.enabled && m.is_default == Some(true)))
            .or_else(|| self.models.iter().find(|m| m.enabled))
    }
    pub fn resolve_model(&self, id: Option<&str>) -> Option<&ModelConfig> {
        match id { Some(i) if i != "auto" && !i.is_empty() => self.model(i).or_else(|| self.default_model()), _ => self.default_model() }
    }
}

fn p(id: &str, kind: ProviderKind, name: &str, base: Option<&str>, key: &str) -> ProviderConfig {
    ProviderConfig { id: id.into(), kind, name: name.into(), base_url: base.map(String::from), api_key_ref: Some(key.into()), enabled: true, ..Default::default() }
}
pub fn default_providers() -> Vec<ProviderConfig> {
    vec![
        p("anthropic", ProviderKind::Anthropic, "Anthropic", None, "anthropic"),
        p("openai", ProviderKind::Openai, "OpenAI", None, "openai"),
        p("openrouter", ProviderKind::Openrouter, "OpenRouter", Some("https://openrouter.ai/api/v1"), "openrouter"),
        p("gemini", ProviderKind::Gemini, "Google Gemini", Some("https://generativelanguage.googleapis.com/v1beta/openai"), "gemini"),
        ProviderConfig { id: "ollama".into(), kind: ProviderKind::Ollama, name: "Ollama (local)".into(), base_url: Some("http://localhost:11434/v1".into()), api_key_ref: None, enabled: true, ..Default::default() },
        ProviderConfig { id: "local".into(), kind: ProviderKind::OpenaiCompatible, name: "OpenAI-compatible (custom)".into(), base_url: Some("http://localhost:8081/v1".into()), api_key_ref: Some("local".into()), enabled: true, ..Default::default() },
    ]
}
fn m(id: &str, provider: &str, model: &str, name: &str, ctx: u32, out: u32, vision: bool, thinking: bool, effort: bool, prices: (f64, f64, f64)) -> ModelConfig {
    ModelConfig { id: id.into(), provider_id: provider.into(), model: model.into(), display_name: name.into(), context_window: ctx, max_output: out, supports_tools: true, supports_vision: vision, supports_thinking: thinking, supports_effort: effort, input_price_per_m: Some(prices.0), output_price_per_m: Some(prices.1), cache_read_price_per_m: Some(prices.2), cache_write_price_per_m: None, enabled: true, is_default: None, tags: None, is_fast: None }
}
pub fn default_models() -> Vec<ModelConfig> {
    let mut v = vec![
        m("claude-opus-5-5", "anthropic", "claude-opus-5-5", "Claude Opus 5.5", 1_000_000, 128_000, true, true, true, (4.0, 20.0, 0.20)),
        m("claude-sonnet-5-5", "anthropic", "claude-sonnet-5-5", "Claude Sonnet 5.5", 1_000_000, 128_000, true, true, true, (2.0, 10.0, 0.20)),
        m("claude-fable-5-1", "anthropic", "claude-fable-5-1", "Claude Fable 5.1", 1_000_000, 128_000, true, true, true, (10.0, 50.0, 0.25)),
        m("claude-haiku-4-5", "anthropic", "claude-haiku-4-5", "Claude Haiku 4.5", 200_000, 64_000, true, true, false, (1.0, 5.0, 0.10)),
        m("gpt-5", "openai", "gpt-5", "GPT-5", 400_000, 128_000, true, true, false, (1.25, 10.0, 0.125)),
        m("gpt-5-mini", "openai", "gpt-5-mini", "GPT-5 mini", 400_000, 128_000, true, true, false, (0.25, 2.0, 0.025)),
        m("openrouter-auto", "openrouter", "openrouter/auto", "OpenRouter Auto", 200_000, 32_000, true, true, false, (0.0, 0.0, 0.0)),
        m("gemini-2.5-pro", "gemini", "gemini-2.5-pro", "Gemini 2.5 Pro", 1_000_000, 65_000, true, true, false, (1.25, 10.0, 0.31)),
    ];
    v[0].is_default = Some(true);
    v[3].is_fast = Some(true);
    v[3].tags = Some(vec!["fast".into(), "tab".into()]);
    v
}
fn lsp(id: &str, name: &str, cmd: &str, args: &[&str], langs: &[&str], exts: &[&str], hint: &str) -> LspServerConfig {
    LspServerConfig { id: id.into(), name: name.into(), command: cmd.into(), args: args.iter().map(|s| s.to_string()).collect(), languages: langs.iter().map(|s| s.to_string()).collect(), extensions: exts.iter().map(|s| s.to_string()).collect(), enabled: true, install_hint: Some(hint.into()), ..Default::default() }
}
pub fn default_lsp_servers() -> Vec<LspServerConfig> {
    vec![
        lsp("typescript", "TypeScript / JavaScript", "typescript-language-server", &["--stdio"], &["typescript", "typescriptreact", "javascript", "javascriptreact"], &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"], "npm i -g typescript typescript-language-server"),
        lsp("rust", "rust-analyzer", "rust-analyzer", &[], &["rust"], &["rs"], "rustup component add rust-analyzer"),
        lsp("python", "Pyright", "pyright-langserver", &["--stdio"], &["python"], &["py", "pyi"], "npm i -g pyright"),
        lsp("go", "gopls", "gopls", &[], &["go"], &["go"], "go install golang.org/x/tools/gopls@latest"),
        lsp("clangd", "clangd", "clangd", &[], &["c", "cpp", "objective-c"], &["c", "h", "cc", "cpp", "hpp", "cxx", "m", "mm"], "brew install llvm"),
        lsp("json", "JSON", "vscode-json-language-server", &["--stdio"], &["json", "jsonc"], &["json", "jsonc"], "npm i -g vscode-langservers-extracted"),
        lsp("css", "CSS", "vscode-css-language-server", &["--stdio"], &["css", "scss", "less"], &["css", "scss", "less"], "npm i -g vscode-langservers-extracted"),
        lsp("html", "HTML", "vscode-html-language-server", &["--stdio"], &["html"], &["html", "htm"], "npm i -g vscode-langservers-extracted"),
        lsp("yaml", "YAML", "yaml-language-server", &["--stdio"], &["yaml"], &["yml", "yaml"], "npm i -g yaml-language-server"),
        lsp("bash", "Bash", "bash-language-server", &["start"], &["shellscript"], &["sh", "bash", "zsh"], "npm i -g bash-language-server"),
        lsp("lua", "Lua", "lua-language-server", &[], &["lua"], &["lua"], "brew install lua-language-server"),
        lsp("java", "Java (jdtls)", "jdtls", &[], &["java"], &["java"], "brew install jdtls"),
        lsp("ruby", "Ruby (solargraph)", "solargraph", &["stdio"], &["ruby"], &["rb"], "gem install solargraph"),
        lsp("zig", "zls", "zls", &[], &["zig"], &["zig"], "brew install zls"),
        lsp("toml", "Taplo", "taplo", &["lsp", "stdio"], &["toml"], &["toml"], "cargo install taplo-cli"),
    ]
}

// ───────────────────────── persistence / state ─────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsScopes {
    pub user: Settings,
    pub workspace: Option<Value>,
    pub effective: Settings,
    pub user_path: String,
    pub workspace_path: Option<String>,
}

pub struct SettingsState {
    user: RwLock<Value>,
    workspace: RwLock<Option<Value>>,
    workspace_root: RwLock<Option<PathBuf>>,
    effective: RwLock<Settings>,
    secrets: RwLock<BTreeMap<String, String>>,
}

fn user_path() -> PathBuf { paths::config_dir().join("settings.json") }
fn secrets_path() -> PathBuf { paths::config_dir().join("secrets.json") }
fn workspace_settings_path(root: &Path) -> PathBuf { root.join(".caret").join("settings.json") }

fn read_json(p: &Path) -> Value {
    std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or(Value::Object(Default::default()))
}
fn write_json_atomic(p: &Path, v: &Value, mode_600: bool) -> Result<()> {
    if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(v)?)?;
    #[cfg(unix)]
    if mode_600 { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)); }
    std::fs::rename(&tmp, p)?;
    Ok(())
}

/// Deep-merge `patch` into `base` (objects recurse, everything else replaces; `null` deletes).
pub fn deep_merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                if v.is_null() { b.remove(k); continue; }
                match b.get_mut(k) {
                    Some(existing) if existing.is_object() && v.is_object() => deep_merge(existing, v),
                    _ => { b.insert(k.clone(), v.clone()); }
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

fn to_settings(v: &Value) -> Settings {
    // Defaults fill any missing field; unknown fields are ignored.
    serde_json::from_value(v.clone()).unwrap_or_else(|e| { tracing::warn!(error=%e, "settings parse failed; using defaults"); Settings::default() })
}

impl SettingsState {
    pub fn load() -> Self {
        let user = read_json(&user_path());
        let secrets: BTreeMap<String, String> = std::fs::read_to_string(secrets_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let st = Self { user: RwLock::new(user), workspace: RwLock::new(None), workspace_root: RwLock::new(None), effective: RwLock::new(Settings::default()), secrets: RwLock::new(secrets) };
        st.recompute();
        st
    }
    fn recompute(&self) {
        let mut merged = serde_json::to_value(Settings::default()).unwrap_or_default();
        deep_merge(&mut merged, &self.user.read());
        if let Some(ws) = self.workspace.read().as_ref() { deep_merge(&mut merged, ws); }
        *self.effective.write() = to_settings(&merged);
    }
    pub fn effective(&self) -> Settings { self.effective.read().clone() }
    pub fn scopes(&self) -> SettingsScopes {
        SettingsScopes { user: to_settings(&self.user.read()), workspace: self.workspace.read().clone(), effective: self.effective(), user_path: user_path().to_string_lossy().to_string(), workspace_path: self.workspace_root.read().as_ref().map(|r| workspace_settings_path(r).to_string_lossy().to_string()) }
    }
    pub fn set_workspace(&self, root: Option<&Path>) {
        *self.workspace_root.write() = root.map(Path::to_path_buf);
        *self.workspace.write() = root.map(|r| { let p = workspace_settings_path(r); if p.exists() { read_json(&p) } else { Value::Object(Default::default()) } });
        self.recompute();
    }
    pub fn patch(&self, patch: &Value, scope: &str) -> Result<Settings> {
        match scope {
            "workspace" => {
                let root = self.workspace_root.read().clone().ok_or_else(CaretError::no_workspace)?;
                let mut ws = self.workspace.write();
                let mut cur = ws.clone().unwrap_or(Value::Object(Default::default()));
                deep_merge(&mut cur, patch);
                write_json_atomic(&workspace_settings_path(&root), &cur, false)?;
                *ws = Some(cur);
            }
            _ => {
                let mut u = self.user.write();
                deep_merge(&mut u, patch);
                write_json_atomic(&user_path(), &u, false)?;
            }
        }
        self.recompute();
        Ok(self.effective())
    }
    pub fn reset(&self, scope: &str) -> Result<Settings> {
        match scope {
            "workspace" => { if let Some(root) = self.workspace_root.read().clone() { let _ = std::fs::remove_file(workspace_settings_path(&root)); } *self.workspace.write() = Some(Value::Object(Default::default())); }
            _ => { *self.user.write() = Value::Object(Default::default()); write_json_atomic(&user_path(), &Value::Object(Default::default()), false)?; }
        }
        self.recompute();
        Ok(self.effective())
    }
    // secrets
    pub fn secret(&self, id: &str) -> Option<String> {
        if let Some(v) = self.secrets.read().get(id) { if !v.is_empty() { return Some(v.clone()); } }
        // env fallbacks
        let env_key = match id { "anthropic" => "ANTHROPIC_API_KEY", "openai" => "OPENAI_API_KEY", "openrouter" => "OPENROUTER_API_KEY", "gemini" => "GEMINI_API_KEY", "local" => "LOCAL_LLM_API_KEY", "brave" => "BRAVE_API_KEY", "tavily" => "TAVILY_API_KEY", _ => "" };
        if !env_key.is_empty() { if let Ok(v) = std::env::var(env_key) { if !v.is_empty() { return Some(v); } } }
        None
    }
    pub fn set_secret(&self, id: &str, value: &str) -> Result<()> {
        let mut s = self.secrets.write();
        if value.is_empty() { s.remove(id); } else { s.insert(id.to_string(), value.to_string()); }
        write_json_atomic(&secrets_path(), &serde_json::to_value(&*s)?, true)
    }
    pub fn list_secrets(&self) -> Vec<String> { self.secrets.read().keys().cloned().collect() }
    pub fn import_env_keys(&self) -> Result<Vec<String>> {
        let pairs = [("anthropic", "ANTHROPIC_API_KEY"), ("openai", "OPENAI_API_KEY"), ("openrouter", "OPENROUTER_API_KEY"), ("gemini", "GEMINI_API_KEY"), ("local", "LYNKR_API_KEY"), ("brave", "BRAVE_API_KEY"), ("tavily", "TAVILY_API_KEY")];
        let mut imported = vec![];
        for (id, env) in pairs { if let Ok(v) = std::env::var(env) { if !v.is_empty() { self.set_secret(id, &v)?; imported.push(id.to_string()); } } }
        Ok(imported)
    }
}

fn broadcast(app: &AppHandle, st: &SettingsState) { events::emit(app, events::SETTINGS_CHANGED, st.effective()); }

// ───────────────────────── commands ─────────────────────────

#[tauri::command]
pub fn settings_get(st: State<'_, SettingsState>) -> Result<SettingsScopes> { Ok(st.scopes()) }

#[tauri::command]
pub fn settings_update(app: AppHandle, st: State<'_, SettingsState>, patch: Value, scope: Option<String>) -> Result<Settings> {
    let s = st.patch(&patch, scope.as_deref().unwrap_or("user"))?;
    broadcast(&app, &st);
    Ok(s)
}

#[tauri::command]
pub fn settings_reset(app: AppHandle, st: State<'_, SettingsState>, scope: String) -> Result<Settings> {
    let s = st.reset(&scope)?;
    broadcast(&app, &st);
    Ok(s)
}

#[tauri::command]
pub fn settings_set_secret(st: State<'_, SettingsState>, id: String, value: String) -> Result<()> { st.set_secret(&id, &value) }
#[tauri::command]
pub fn settings_delete_secret(st: State<'_, SettingsState>, id: String) -> Result<()> { st.set_secret(&id, "") }
#[tauri::command]
pub fn settings_list_secrets(st: State<'_, SettingsState>) -> Result<Vec<String>> { Ok(st.list_secrets()) }
#[tauri::command]
pub fn settings_import_env_keys(st: State<'_, SettingsState>) -> Result<Vec<String>> { st.import_env_keys() }

#[tauri::command]
pub fn settings_path(st: State<'_, SettingsState>, scope: String) -> Result<String> {
    Ok(match scope.as_str() {
        "workspace" => st.workspace_root.read().clone().map(|r| workspace_settings_path(&r)).ok_or_else(CaretError::no_workspace)?,
        "keybindings" => paths::config_dir().join("keybindings.json"),
        _ => user_path(),
    }.to_string_lossy().to_string())
}

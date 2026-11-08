//! Usage ledger: an append-only monthly JSONL of priced requests (`~/.caret/usage/YYYY-MM.jsonl`), the aggregations
//! behind Settings → Usage, and read-only parsers for the local histories of external agents (Claude Code, Codex,
//! OpenCode). Recording never blocks the caller: entries go through a channel to one writer thread.
use crate::ai::llm::UsageDelta;
use crate::core::settings::{ModelConfig, ModelPrice, Settings, SettingsState};
use crate::error::{CaretError, Result};
use crate::paths;
use chrono::{DateTime, Days, Local, NaiveDate, TimeZone};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::State;

// ───────────────────────── entries ─────────────────────────

/// One priced request. `source` is `agent` (built-in chat), `acp` (external agent in Caret chat), `inline`, `tab`,
/// `review`, `commit` or `other`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageEntry {
    pub ts: u64,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub workspace: Option<String>,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub thinking: u64,
    #[serde(skip_serializing_if = "Option::is_none")] pub cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub duration_ms: Option<u64>,
}

impl UsageEntry {
    pub fn new(source: &str, d: &UsageDelta, cost_usd: Option<f64>) -> Self {
        Self { ts: now_ms(), source: source.to_string(), input: d.input_tokens, output: d.output_tokens, cache_read: d.cache_read_tokens, cache_write: d.cache_write_tokens, thinking: d.thinking_tokens, cost_usd, ..Default::default() }
    }
    pub fn model(mut self, m: &ModelConfig) -> Self { self.model_id = Some(m.id.clone()); self.provider_id = Some(m.provider_id.clone()); self }
    pub fn agent(mut self, id: &str) -> Self { self.agent_id = Some(id.to_string()); self }
    pub fn session(mut self, id: &str) -> Self { self.session_id = Some(id.to_string()); self }
    pub fn workspace(mut self, root: Option<&Path>) -> Self { self.workspace = root.map(|p| p.to_string_lossy().to_string()); self }
    pub fn duration(mut self, ms: u64) -> Self { self.duration_ms = Some(ms); self }
    /// Nothing worth writing: no tokens and no (positive) cost.
    pub fn is_empty(&self) -> bool {
        self.input + self.output + self.cache_read + self.cache_write + self.thinking == 0 && self.cost_usd.map(|c| c <= 0.0).unwrap_or(true)
    }
    pub fn total_tokens(&self) -> u64 { self.input + self.output + self.cache_read + self.cache_write }
}

// ───────────────────────── time helpers (local calendar) ─────────────────────────

pub fn now_ms() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) }

fn local_dt(ms: u64) -> DateTime<Local> {
    Local.timestamp_millis_opt(ms.min(i64::MAX as u64) as i64).earliest().unwrap_or_else(|| Local.timestamp_millis_opt(0).earliest().expect("epoch"))
}
fn date_start_ms(d: NaiveDate) -> u64 {
    let dt = d.and_hms_opt(0, 0, 0).expect("midnight");
    Local.from_local_datetime(&dt).earliest().map(|x| x.timestamp_millis().max(0) as u64).unwrap_or(0)
}
pub fn day_start_ms(ms: u64) -> u64 { date_start_ms(local_dt(ms).date_naive()) }
pub fn day_key(ms: u64) -> String { local_dt(ms).format("%Y-%m-%d").to_string() }
pub fn month_key(ms: u64) -> String { local_dt(ms).format("%Y-%m").to_string() }
/// `[start of month, start of next month)` in local time for a `YYYY-MM` key.
fn month_bounds(key: &str) -> Option<(u64, u64)> {
    let (y, m) = key.split_once('-')?;
    let (y, m): (i32, u32) = (y.parse().ok()?, m.parse().ok()?);
    let start = NaiveDate::from_ymd_opt(y, m, 1)?;
    let next = if m == 12 { NaiveDate::from_ymd_opt(y + 1, 1, 1)? } else { NaiveDate::from_ymd_opt(y, m + 1, 1)? };
    Some((date_start_ms(start), date_start_ms(next)))
}
pub const RANGES: [&str; 4] = ["today", "7d", "30d", "all"];
pub const GROUPS: [&str; 6] = ["model", "provider", "agent", "source", "day", "workspace"];
/// `[from, to]` in ms for a named range, anchored at local midnight.
pub fn range_bounds(range: &str, now: u64) -> (u64, u64) {
    let today = local_dt(now).date_naive();
    match range {
        "today" => (date_start_ms(today), now),
        "7d" => (date_start_ms(today.checked_sub_days(Days::new(6)).unwrap_or(today)), now),
        "30d" => (date_start_ms(today.checked_sub_days(Days::new(29)).unwrap_or(today)), now),
        _ => (0, now),
    }
}
pub fn parse_iso_ms(s: &str) -> Option<u64> { DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis().max(0) as u64) }

// ───────────────────────── ledger ─────────────────────────

pub struct Ledger { dir: PathBuf }
impl Ledger {
    pub fn new(dir: impl Into<PathBuf>) -> Self { Self { dir: dir.into() } }
    pub fn default_dir() -> PathBuf { paths::sub_dir("usage") }
    pub fn open_default() -> Self { Self::new(Self::default_dir()) }
    pub fn dir(&self) -> &Path { &self.dir }
    pub fn file_for(&self, ts: u64) -> PathBuf { self.dir.join(format!("{}.jsonl", month_key(ts))) }

    pub fn append(&self, e: &UsageEntry) -> std::io::Result<()> {
        use std::io::Write;
        std::fs::create_dir_all(&self.dir)?;
        let mut line = serde_json::to_string(e).map_err(std::io::Error::other)?;
        line.push('\n');
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(self.file_for(e.ts))?;
        f.write_all(line.as_bytes())
    }

    /// `(YYYY-MM, path)` for every month file, oldest first.
    fn month_files(&self) -> Vec<(String, PathBuf)> {
        let Ok(rd) = std::fs::read_dir(&self.dir) else { return vec![] };
        let mut v: Vec<(String, PathBuf)> = rd.flatten().filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let key = name.strip_suffix(".jsonl")?;
            if key.len() != 7 || month_bounds(key).is_none() { return None; }
            Some((key.to_string(), e.path()))
        }).collect();
        v.sort();
        v
    }

    fn parse_file(path: &Path) -> Vec<UsageEntry> {
        let Ok(text) = std::fs::read_to_string(path) else { return vec![] };
        text.lines().filter_map(|l| serde_json::from_str::<UsageEntry>(l).ok()).collect()
    }

    /// Entries with `from <= ts <= to`, oldest first. Only month files that can overlap the range are opened.
    pub fn read_range(&self, from: u64, to: u64) -> Vec<UsageEntry> {
        let pad = 2 * 86_400_000u64;
        let mut out = vec![];
        for (key, path) in self.month_files() {
            if let Some((ms, me)) = month_bounds(&key) { if me.saturating_add(pad) < from || ms > to.saturating_add(pad) { continue; } }
            out.extend(Self::parse_file(&path).into_iter().filter(|e| e.ts >= from && e.ts <= to));
        }
        out.sort_by_key(|e| e.ts);
        out
    }

    /// The newest `limit` entries, newest first.
    pub fn read_recent(&self, limit: usize) -> Vec<UsageEntry> {
        let mut out: Vec<UsageEntry> = vec![];
        for (_, path) in self.month_files().into_iter().rev() {
            let mut v = Self::parse_file(&path);
            v.reverse();
            out.extend(v);
            if out.len() >= limit { break; }
        }
        out.sort_by(|a, b| b.ts.cmp(&a.ts));
        out.truncate(limit);
        out
    }
}

// ───────────────────────── recording (non-blocking) ─────────────────────────

static WRITER: OnceLock<mpsc::Sender<UsageEntry>> = OnceLock::new();
fn writer() -> &'static mpsc::Sender<UsageEntry> {
    WRITER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<UsageEntry>();
        let spawned = std::thread::Builder::new().name("caret-usage-ledger".into()).spawn(move || {
            let ledger = Ledger::open_default();
            for e in rx { if let Err(err) = ledger.append(&e) { tracing::warn!(error = %err, "usage ledger append failed"); } }
        });
        if let Err(err) = spawned { tracing::warn!(error = %err, "usage ledger writer thread failed to start"); }
        tx
    })
}

/// Queue an entry for the ledger. Cheap, never blocks; errors are logged, not returned.
pub fn record(settings: &Settings, entry: UsageEntry) {
    if !settings.usage.track || entry.is_empty() { return; }
    if let Err(err) = writer().send(entry) { tracing::warn!(error = %err, "usage ledger writer is gone"); }
}

/// ACP `usage_update`: agents report an absolute running cost (`cost.amount`), so the ledger gets the delta against
/// the cost already stored on the session. Token fields are only present with some agents and are taken as-is.
pub fn record_acp(settings: &Settings, h: &crate::ai::agent::manager::SessionHandle, agent_id: &str, update: &Value) {
    if !settings.usage.track { return; }
    let (prev_cost, model_id, workspace, session_id) = { let s = h.session.lock(); (s.usage.cost_usd, s.model_id.clone(), s.workspace_root.clone(), s.id.clone()) };
    let cost = update.pointer("/cost/amount").and_then(|c| c.as_f64()).map(|c| (c - prev_cost.unwrap_or(0.0)).max(0.0));
    let g = |keys: &[&str]| keys.iter().find_map(|k| update.get(*k).and_then(|v| v.as_u64())).unwrap_or(0);
    let entry = UsageEntry {
        ts: now_ms(), source: "acp".into(), provider_id: None, model_id: model_id.filter(|m| !m.is_empty() && m != "auto"), agent_id: Some(agent_id.to_string()),
        session_id: Some(session_id), workspace: if workspace.is_empty() { None } else { Some(workspace) },
        input: g(&["inputTokens"]), output: g(&["outputTokens"]), cache_read: g(&["cacheReadTokens", "cachedTokens"]), cache_write: g(&["cacheWriteTokens"]), thinking: g(&["thinkingTokens", "reasoningTokens"]),
        cost_usd: cost.filter(|c| *c > 0.0), duration_ms: None,
    };
    record(settings, entry);
}

// ───────────────────────── prices ─────────────────────────

/// Prices per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prices { pub input: f64, pub output: f64, pub cache_read: f64, pub cache_write: f64 }
impl Prices {
    /// Same defaults as `UsageDelta::cost_usd`: cache read 10% of input, cache write 125%.
    pub fn per_m(input: f64, output: f64, cache_read: Option<f64>, cache_write: Option<f64>) -> Self {
        Self { input, output, cache_read: cache_read.unwrap_or(input * 0.1), cache_write: cache_write.unwrap_or(input * 1.25) }
    }
    pub fn cost(&self, input: u64, output: u64, cache_read: u64, cache_write: u64) -> f64 {
        (input as f64 * self.input + output as f64 * self.output + cache_read as f64 * self.cache_read + cache_write as f64 * self.cache_write) / 1e6
    }
    fn from_model(m: &ModelConfig) -> Option<Self> { Some(Self::per_m(m.input_price_per_m?, m.output_price_per_m.unwrap_or(0.0), m.cache_read_price_per_m, m.cache_write_price_per_m)) }
    fn from_custom(p: &ModelPrice) -> Self { Self::per_m(p.input, p.output, p.cache_read, p.cache_write) }
}

/// Public list prices (USD per million tokens: input, output, cache read) for well-known model id prefixes. Longest
/// prefix wins, so `claude-sonnet-4-5-20250929` resolves to `claude-sonnet-4-5`. Cache write defaults to 125% of input.
const BUILTIN_PRICES: &[(&str, f64, f64, f64)] = &[
    // Anthropic (first-party ids as they appear in Claude Code transcripts; the first three match Caret's catalog)
    ("claude-opus-5-5", 4.0, 20.0, 0.20), ("claude-sonnet-5-5", 2.0, 10.0, 0.20), ("claude-fable-5-1", 10.0, 50.0, 0.25),
    ("claude-opus-4-5", 5.0, 25.0, 0.50), ("claude-opus-4-1", 15.0, 75.0, 1.50), ("claude-opus-4", 15.0, 75.0, 1.50),
    ("claude-sonnet-4-5", 3.0, 15.0, 0.30), ("claude-sonnet-4", 3.0, 15.0, 0.30), ("claude-3-7-sonnet", 3.0, 15.0, 0.30), ("claude-3-5-sonnet", 3.0, 15.0, 0.30),
    ("claude-haiku-4-5", 1.0, 5.0, 0.10), ("claude-3-5-haiku", 0.80, 4.0, 0.08), ("claude-3-haiku", 0.25, 1.25, 0.03),
    // OpenAI (GPT-5 family incl. Codex variants, 4.1, 4o, o-series)
    ("gpt-5.1-codex-max", 1.25, 10.0, 0.125), ("gpt-5.1-codex-mini", 0.25, 2.0, 0.025), ("gpt-5.1-codex", 1.25, 10.0, 0.125), ("gpt-5.1-mini", 0.25, 2.0, 0.025), ("gpt-5.1", 1.25, 10.0, 0.125),
    ("gpt-5-codex", 1.25, 10.0, 0.125), ("gpt-5-pro", 15.0, 120.0, 15.0), ("gpt-5-mini", 0.25, 2.0, 0.025), ("gpt-5-nano", 0.05, 0.40, 0.005), ("gpt-5", 1.25, 10.0, 0.125),
    ("codex-mini", 1.50, 6.0, 0.375), ("gpt-4.1-mini", 0.40, 1.60, 0.10), ("gpt-4.1-nano", 0.10, 0.40, 0.025), ("gpt-4.1", 2.0, 8.0, 0.50), ("gpt-4o-mini", 0.15, 0.60, 0.075), ("gpt-4o", 2.50, 10.0, 1.25),
    ("o4-mini", 1.10, 4.40, 0.275), ("o3-pro", 20.0, 80.0, 20.0), ("o3-mini", 1.10, 4.40, 0.55), ("o3", 2.0, 8.0, 0.50), ("o1-pro", 150.0, 600.0, 150.0), ("o1-mini", 1.10, 4.40, 0.55), ("o1", 15.0, 60.0, 7.50),
    // Google
    ("gemini-3-pro", 2.0, 12.0, 0.20), ("gemini-3-flash", 0.50, 3.0, 0.05), ("gemini-2.5-pro", 1.25, 10.0, 0.31), ("gemini-2.5-flash-lite", 0.10, 0.40, 0.01), ("gemini-2.5-flash", 0.30, 2.50, 0.03),
    // xAI
    ("grok-4-1-fast", 0.20, 0.50, 0.05), ("grok-4-fast", 0.20, 0.50, 0.05), ("grok-4", 3.0, 15.0, 0.75), ("grok-code-fast-1", 0.20, 1.50, 0.02), ("grok-3-mini", 0.30, 0.50, 0.075), ("grok-3", 3.0, 15.0, 0.75),
];

/// Lower-case, drop a provider prefix (`anthropic/…`), `[1m]`, `@version` / `:tag` suffixes and `-latest`.
pub fn normalize_model_id(id: &str) -> String {
    let mut s = id.trim().to_ascii_lowercase();
    if let Some(i) = s.rfind('/') { s = s[i + 1..].to_string(); }
    if let Some(x) = s.strip_suffix("[1m]") { s = x.to_string(); }
    if let Some(i) = s.find(['@', ':']) { s.truncate(i); }
    s.trim_end_matches("-latest").to_string()
}
fn is_boundary(c: Option<char>) -> bool { matches!(c, None | Some('-') | Some('.') | Some('_') | Some('@') | Some(':')) }
/// Longest key that equals `id` or is a prefix of it ending on a separator.
fn longest_prefix<'a, T, I: IntoIterator<Item = (&'a str, T)>>(keys: I, id: &str) -> Option<T> {
    let mut best: Option<(usize, T)> = None;
    for (k, v) in keys {
        if id.starts_with(k) && is_boundary(id[k.len()..].chars().next()) && best.as_ref().map(|(l, _)| k.len() > *l).unwrap_or(true) { best = Some((k.len(), v)); }
    }
    best.map(|(_, v)| v)
}
pub fn builtin_prices(model_id: &str) -> Option<Prices> {
    let id = normalize_model_id(model_id);
    longest_prefix(BUILTIN_PRICES.iter().map(|(k, i, o, c)| (*k, (*i, *o, *c))), &id).map(|(i, o, c)| Prices::per_m(i, o, Some(c), None))
}
/// `usage.customPrices` first (exact id, then prefix), then the model catalog (by id or provider model name), then the built-in table.
pub fn resolve_prices(settings: &Settings, model_id: &str) -> Option<Prices> {
    let norm = normalize_model_id(model_id);
    if let Some(p) = settings.usage.custom_prices.iter().find(|(k, _)| k.eq_ignore_ascii_case(model_id) || normalize_model_id(k) == norm) { return Some(Prices::from_custom(p.1)); }
    let custom: Vec<(String, &ModelPrice)> = settings.usage.custom_prices.iter().map(|(k, v)| (normalize_model_id(k), v)).collect();
    if let Some(p) = longest_prefix(custom.iter().map(|(k, v)| (k.as_str(), *v)), &norm) { return Some(Prices::from_custom(p)); }
    if let Some(p) = settings.models.iter().find(|m| m.id == model_id || normalize_model_id(&m.model) == norm).and_then(Prices::from_model) { return Some(p); }
    builtin_prices(model_id)
}
/// The entry's own cost when the emit site priced it, otherwise a lookup by model id.
pub fn entry_cost(settings: &Settings, e: &UsageEntry) -> Option<f64> {
    e.cost_usd.or_else(|| e.model_id.as_deref().and_then(|m| resolve_prices(settings, m)).map(|p| p.cost(e.input, e.output, e.cache_read, e.cache_write)))
}

// ───────────────────────── aggregation ─────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Totals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub thinking: u64,
    /// `None` when no request in the group could be priced (unknown model and no reported cost).
    pub cost_usd: Option<f64>,
    pub requests: u64,
    /// Requests that had tokens but no price.
    pub unpriced: u64,
    /// cache_read / (input + cache_read + cache_write).
    pub cache_hit_rate: f64,
}
impl Totals {
    fn add(&mut self, input: u64, output: u64, cache_read: u64, cache_write: u64, thinking: u64, cost: Option<f64>) {
        self.input += input; self.output += output; self.cache_read += cache_read; self.cache_write += cache_write; self.thinking += thinking; self.requests += 1;
        match cost { Some(c) => self.cost_usd = Some(self.cost_usd.unwrap_or(0.0) + c), None => self.unpriced += 1 }
    }
    fn finish(&mut self) { let denom = self.input + self.cache_read + self.cache_write; self.cache_hit_rate = if denom > 0 { self.cache_read as f64 / denom as f64 } else { 0.0 }; }
    pub fn tokens(&self) -> u64 { self.input + self.output + self.cache_read + self.cache_write }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Row { pub key: String, pub label: String, #[serde(flatten)] pub totals: Totals }

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Summary { pub totals: Totals, pub rows: Vec<Row>, pub from: u64, pub to: u64 }

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DailyPoint { pub day: String, pub cost_usd: f64, pub tokens: u64 }

pub fn source_label(source: &str) -> &'static str {
    match source { "agent" => "Agent chat", "acp" => "External agent", "inline" => "Inline edit", "tab" => "Tab", "review" => "Review", "commit" => "Commit message", _ => "Other" }
}
fn basename(p: &str) -> String { Path::new(p).file_name().map(|s| s.to_string_lossy().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| p.to_string()) }

fn group_key(settings: &Settings, e: &UsageEntry, group_by: &str) -> (String, String) {
    match group_by {
        "provider" => { let id = e.provider_id.clone().unwrap_or_else(|| "unknown".into()); let label = settings.provider(&id).map(|p| p.name.clone()).unwrap_or_else(|| id.clone()); (id, label) }
        "agent" => {
            let id = e.agent_id.clone().unwrap_or_else(|| "caret".into());
            let label = if id == "caret" { "Caret".to_string() } else { settings.acp.agents.iter().find(|a| a.id == id).map(|a| a.name.clone()).unwrap_or_else(|| id.clone()) };
            (id, label)
        }
        "source" => (e.source.clone(), source_label(&e.source).to_string()),
        "day" => { let k = day_key(e.ts); (k.clone(), k) }
        "workspace" => match &e.workspace { Some(w) if !w.is_empty() => (w.clone(), basename(w)), _ => ("(none)".into(), "No workspace".into()) },
        _ => { let id = e.model_id.clone().unwrap_or_else(|| "unknown".into()); let label = settings.model(&id).map(|m| m.display_name.clone()).unwrap_or_else(|| id.clone()); (id, label) }
    }
}

fn finish_rows(groups: BTreeMap<String, (String, Totals)>, by_key: bool) -> Vec<Row> {
    let mut rows: Vec<Row> = groups.into_iter().map(|(key, (label, mut t))| { t.finish(); Row { key, label, totals: t } }).collect();
    if !by_key { rows.sort_by(|a, b| b.totals.cost_usd.unwrap_or(0.0).partial_cmp(&a.totals.cost_usd.unwrap_or(0.0)).unwrap_or(std::cmp::Ordering::Equal).then(b.totals.requests.cmp(&a.totals.requests)).then(a.key.cmp(&b.key))); }
    rows
}

pub fn summarize(settings: &Settings, entries: &[UsageEntry], group_by: &str, from: u64, to: u64) -> Summary {
    let mut totals = Totals::default();
    let mut groups: BTreeMap<String, (String, Totals)> = BTreeMap::new();
    for e in entries {
        let cost = entry_cost(settings, e);
        totals.add(e.input, e.output, e.cache_read, e.cache_write, e.thinking, cost);
        let (key, label) = group_key(settings, e, group_by);
        groups.entry(key).or_insert_with(|| (label, Totals::default())).1.add(e.input, e.output, e.cache_read, e.cache_write, e.thinking, cost);
    }
    totals.finish();
    Summary { totals, rows: finish_rows(groups, group_by == "day"), from, to }
}

/// One point per local day from `from` (or the oldest entry when `from == 0`) to `to`, zero-filled.
pub fn daily(settings: &Settings, entries: &[UsageEntry], from: u64, to: u64) -> Vec<DailyPoint> {
    let start = if from == 0 { match entries.iter().map(|e| e.ts).min() { Some(t) => t, None => return vec![] } } else { from };
    let mut by: HashMap<String, (f64, u64)> = HashMap::new();
    for e in entries { let d = by.entry(day_key(e.ts)).or_default(); d.0 += entry_cost(settings, e).unwrap_or(0.0); d.1 += e.total_tokens(); }
    let mut out = vec![];
    let mut d = local_dt(start).date_naive();
    let end = local_dt(to.max(start)).date_naive();
    while d <= end && out.len() < 5000 {
        let key = d.format("%Y-%m-%d").to_string();
        let (cost_usd, tokens) = by.get(&key).copied().unwrap_or((0.0, 0));
        out.push(DailyPoint { day: key, cost_usd, tokens });
        d = match d.succ_opt() { Some(n) => n, None => break };
    }
    out
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}
pub fn to_csv(entries: &[UsageEntry]) -> String {
    let mut out = String::from("ts,time,source,provider,model,agent,session,workspace,input,output,cache_read,cache_write,thinking,cost_usd,duration_ms\n");
    for e in entries {
        let iso = local_dt(e.ts).to_rfc3339();
        let opt = |o: &Option<String>| csv_field(o.as_deref().unwrap_or(""));
        out.push_str(&format!("{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n", e.ts, iso, csv_field(&e.source), opt(&e.provider_id), opt(&e.model_id), opt(&e.agent_id), opt(&e.session_id), opt(&e.workspace),
            e.input, e.output, e.cache_read, e.cache_write, e.thinking, e.cost_usd.map(|c| format!("{c:.6}")).unwrap_or_default(), e.duration_ms.map(|d| d.to_string()).unwrap_or_default()));
    }
    out
}

// ───────────────────────── external agents ─────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Window { pub label: String, pub used_percent: f64, pub window_minutes: u64, #[serde(skip_serializing_if = "Option::is_none")] pub resets_at: Option<u64> }

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExternalAgent {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub source_path_pattern: String,
    pub totals: Totals,
    pub by_model: Vec<Row>,
    pub windows: Vec<Window>,
    #[serde(skip_serializing_if = "Option::is_none")] pub note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExternalReport { pub agents: Vec<ExternalAgent>, pub from: u64, pub to: u64 }

/// One assistant response parsed from an external history.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtRecord { pub ts: u64, pub model: String, pub input: u64, pub output: u64, pub cache_read: u64, pub cache_write: u64, pub thinking: u64, pub cost: Option<f64> }

/// Hard cap on history bytes read per agent per scan.
pub const MAX_SCAN_BYTES: u64 = 1024 * 1024 * 1024;
const EXTERNAL_CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct ScanBudget { pub bytes: u64, pub files: usize, pub capped: bool }
impl ScanBudget {
    /// Read a file if it was modified at or after `from` and the byte budget allows it.
    fn take(&mut self, path: &Path, from: u64) -> Option<String> {
        let meta = std::fs::metadata(path).ok()?;
        if from > 0 { let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(u64::MAX); if mtime < from { return None; } }
        if self.bytes.saturating_add(meta.len()) > MAX_SCAN_BYTES { self.capped = true; return None; }
        let text = std::fs::read_to_string(path).ok()?;
        self.bytes += meta.len(); self.files += 1;
        Some(text)
    }
}

/// Files with `ext` under `root`, newest first (by mtime), so a byte cap drops the oldest history rather than an
/// alphabetical slice of it.
fn files_under(root: &Path, max_depth: usize, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<(u64, PathBuf)> = walkdir::WalkDir::new(root).max_depth(max_depth).follow_links(false).into_iter().filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file() && e.path().extension().and_then(|x| x.to_str()) == Some(ext))
        .map(|e| {
            let mtime = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
            (mtime, e.into_path())
        }).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    v.into_iter().map(|(_, p)| p).collect()
}

// Claude Code: ~/.claude/projects/<project>/<session>.jsonl
fn claude_root() -> PathBuf { std::env::var("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|_| paths::home_dir().join(".claude")) }

/// `type: "assistant"` lines carry `message.model`, `message.usage` and an ISO `timestamp`. Claude Code writes one
/// line per content block with the same `message.id` and identical usage, so responses are deduplicated by id.
pub fn parse_claude_jsonl(text: &str, from: u64, to: u64, seen: &mut HashSet<String>) -> Vec<ExtRecord> {
    let mut out = vec![];
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("type").and_then(|t| t.as_str()) != Some("assistant") { continue; }
        let Some(msg) = v.get("message") else { continue };
        let Some(usage) = msg.get("usage").filter(|u| u.is_object()) else { continue };
        let model = msg.get("model").and_then(|m| m.as_str()).unwrap_or("unknown");
        if model.contains("synthetic") { continue; }
        let id = msg.get("id").and_then(|i| i.as_str()).or_else(|| v.get("requestId").and_then(|i| i.as_str()));
        if let Some(id) = id { if !seen.insert(id.to_string()) { continue; } }
        let ts = v.get("timestamp").and_then(|t| t.as_str()).and_then(parse_iso_ms).unwrap_or(0);
        if ts < from || ts > to { continue; }
        let g = |k: &str| usage.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
        out.push(ExtRecord { ts, model: model.to_string(), input: g("input_tokens"), output: g("output_tokens"), cache_read: g("cache_read_input_tokens"), cache_write: g("cache_creation_input_tokens"), thinking: 0, cost: None });
    }
    out
}
fn scan_claude(root: &Path, from: u64, to: u64, budget: &mut ScanBudget) -> Option<Vec<ExtRecord>> {
    let projects = root.join("projects");
    if !projects.is_dir() { return None; }
    let mut out = vec![];
    for f in files_under(&projects, 4, "jsonl") {
        let Some(text) = budget.take(&f, from) else { if budget.capped { break; } else { continue; } };
        let mut seen = HashSet::new();
        out.extend(parse_claude_jsonl(&text, from, to, &mut seen));
    }
    Some(out)
}

// Codex: ~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl
fn codex_root() -> PathBuf { std::env::var("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|_| paths::home_dir().join(".codex")) }

pub fn window_label(minutes: u64) -> String {
    if minutes == 0 { return "Window".into(); }
    if minutes % 10_080 == 0 { let n = minutes / 10_080; return if n == 1 { "Weekly".into() } else { format!("{n} weeks") }; }
    if minutes % 1_440 == 0 { let n = minutes / 1_440; return if n == 1 { "Daily".into() } else { format!("{n} days") }; }
    if minutes % 60 == 0 { let n = minutes / 60; return if n == 1 { "1 hour".into() } else { format!("{n} hours") }; }
    format!("{minutes} min")
}
/// `payload.rate_limits.{primary,secondary}`: `{ used_percent, window_minutes, resets_at | resets_in_seconds }`.
pub fn parse_codex_rate_limits(rl: &Value, ts: u64) -> Vec<Window> {
    let mut out = vec![];
    for key in ["primary", "secondary"] {
        let Some(w) = rl.get(key).filter(|w| w.is_object()) else { continue };
        let used_percent = w.get("used_percent").and_then(|x| x.as_f64()).unwrap_or(0.0);
        let window_minutes = w.get("window_minutes").and_then(|x| x.as_u64()).unwrap_or(0);
        let resets_at = w.get("resets_at").and_then(|r| match r { Value::Number(n) => n.as_u64().map(|s| if s < 100_000_000_000 { s * 1000 } else { s }), Value::String(s) => parse_iso_ms(s), _ => None })
            .or_else(|| w.get("resets_in_seconds").and_then(|s| s.as_u64()).map(|s| ts + s * 1000));
        out.push(Window { label: window_label(window_minutes), used_percent, window_minutes, resets_at });
    }
    out
}
/// Sums `payload.info.last_token_usage` of every `token_count` event in range (Codex `input_tokens` includes the
/// cached part, which is split out as cache reads) and keeps the newest `rate_limits` seen.
pub fn parse_codex_jsonl(text: &str, from: u64, to: u64) -> (Vec<ExtRecord>, Option<(u64, Vec<Window>)>) {
    let mut model = String::from("unknown");
    let mut out = vec![];
    let mut latest: Option<(u64, Vec<Window>)> = None;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let ts = v.get("timestamp").and_then(|t| t.as_str()).and_then(parse_iso_ms).unwrap_or(0);
        let Some(payload) = v.get("payload") else { continue };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("session_meta") | Some("turn_context") => { if let Some(m) = payload.get("model").and_then(|m| m.as_str()) { model = m.to_string(); } }
            Some("event_msg") if payload.get("type").and_then(|t| t.as_str()) == Some("token_count") => {
                if let Some(last) = payload.pointer("/info/last_token_usage").filter(|l| l.is_object()) {
                    if ts >= from && ts <= to {
                        let g = |k: &str| last.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
                        let (input, cached) = (g("input_tokens"), g("cached_input_tokens"));
                        let rec = ExtRecord { ts, model: model.clone(), input: input.saturating_sub(cached), output: g("output_tokens"), cache_read: cached, cache_write: 0, thinking: g("reasoning_output_tokens"), cost: None };
                        if rec.input + rec.output + rec.cache_read > 0 { out.push(rec); }
                    }
                }
                if let Some(rl) = payload.get("rate_limits").filter(|r| r.is_object()) {
                    let w = parse_codex_rate_limits(rl, ts);
                    if !w.is_empty() && latest.as_ref().map(|(t, _)| ts >= *t).unwrap_or(true) { latest = Some((ts, w)); }
                }
            }
            _ => {}
        }
    }
    (out, latest)
}
fn scan_codex(root: &Path, from: u64, to: u64, budget: &mut ScanBudget) -> Option<(Vec<ExtRecord>, Vec<Window>)> {
    let sessions = root.join("sessions");
    if !sessions.is_dir() { return None; }
    let mut out = vec![];
    let mut latest: Option<(u64, Vec<Window>)> = None;
    for f in files_under(&sessions, 5, "jsonl") {
        if !f.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("rollout-")).unwrap_or(false) { continue; }
        // Rate limits are useful even when the file is older than the range, so look at every file the budget allows.
        let Some(text) = budget.take(&f, 0) else { if budget.capped { break; } else { continue; } };
        let (recs, w) = parse_codex_jsonl(&text, from, to);
        out.extend(recs);
        if let Some((t, w)) = w { if latest.as_ref().map(|(lt, _)| t >= *lt).unwrap_or(true) { latest = Some((t, w)); } }
    }
    Some((out, latest.map(|(_, w)| w).unwrap_or_default()))
}

// OpenCode: ~/.local/share/opencode/storage/message/<session>/*.json, or opencode.db (SQLite) in newer versions
fn opencode_dir() -> PathBuf {
    std::env::var("XDG_DATA_HOME").map(|x| PathBuf::from(x).join("opencode")).unwrap_or_else(|_| paths::home_dir().join(".local/share/opencode"))
}
/// Assistant message: `tokens { input, output, reasoning, cache { read, write } }`, `cost`, `modelID`, `providerID`, `time.created` (ms).
pub fn parse_opencode_message(v: &Value, from: u64, to: u64) -> Option<ExtRecord> {
    if v.get("role").and_then(|r| r.as_str()) != Some("assistant") { return None; }
    let ts = v.pointer("/time/created").and_then(|t| t.as_u64())?;
    if ts < from || ts > to { return None; }
    let t = v.get("tokens").filter(|t| t.is_object())?;
    let g = |p: &str| t.pointer(p).and_then(|x| x.as_u64()).unwrap_or(0);
    let model = v.get("modelID").and_then(|m| m.as_str()).unwrap_or("unknown").to_string();
    let cost = v.get("cost").and_then(|c| c.as_f64()).filter(|c| *c > 0.0);
    Some(ExtRecord { ts, model, input: g("/input"), output: g("/output"), cache_read: g("/cache/read"), cache_write: g("/cache/write"), thinking: g("/reasoning"), cost })
}
fn scan_opencode_sqlite(db: &Path, from: u64, to: u64) -> std::result::Result<Vec<ExtRecord>, String> {
    let sqlite = which::which("sqlite3").map_err(|_| "the sqlite3 command is not installed, so opencode.db could not be read".to_string())?;
    let sql = format!("select data from message where time_created >= {from} and time_created <= {to} and data like '%\"assistant\"%';");
    let out = std::process::Command::new(sqlite).arg("-readonly").arg("-batch").arg("-noheader").arg(db).arg(sql).output().map_err(|e| e.to_string())?;
    if !out.status.success() { return Err(String::from_utf8_lossy(&out.stderr).trim().to_string()); }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut recs = vec![];
    let mut buf = String::new();
    for line in text.lines() {
        buf.push_str(line);
        if let Ok(v) = serde_json::from_str::<Value>(&buf) { recs.extend(parse_opencode_message(&v, from, to)); buf.clear(); }
        else if buf.len() > 4 * 1024 * 1024 { buf.clear(); }
    }
    Ok(recs)
}
fn scan_opencode(dir: &Path, from: u64, to: u64, budget: &mut ScanBudget) -> Option<(Vec<ExtRecord>, Option<String>)> {
    let storage = dir.join("storage").join("message");
    if storage.is_dir() {
        let mut out = vec![];
        for f in files_under(&storage, 3, "json") {
            let Some(text) = budget.take(&f, from) else { if budget.capped { break; } else { continue; } };
            if let Ok(v) = serde_json::from_str::<Value>(&text) { out.extend(parse_opencode_message(&v, from, to)); }
        }
        return Some((out, None));
    }
    let db = dir.join("opencode.db");
    if db.is_file() {
        return Some(match scan_opencode_sqlite(&db, from, to) { Ok(recs) => (recs, Some("Read from opencode.db (SQLite) via the sqlite3 command.".into())), Err(e) => (vec![], Some(format!("OpenCode history found but not readable: {e}"))) });
    }
    None
}

fn build_agent(settings: &Settings, id: &str, name: &str, pattern: &str, available: bool, records: Vec<ExtRecord>, windows: Vec<Window>, note: Option<String>) -> ExternalAgent {
    let mut totals = Totals::default();
    let mut by: BTreeMap<String, (String, Totals)> = BTreeMap::new();
    for r in &records {
        let cost = r.cost.or_else(|| resolve_prices(settings, &r.model).map(|p| p.cost(r.input, r.output, r.cache_read, r.cache_write)));
        totals.add(r.input, r.output, r.cache_read, r.cache_write, r.thinking, cost);
        by.entry(r.model.clone()).or_insert_with(|| (r.model.clone(), Totals::default())).1.add(r.input, r.output, r.cache_read, r.cache_write, r.thinking, cost);
    }
    totals.finish();
    ExternalAgent { id: id.into(), name: name.into(), available, source_path_pattern: pattern.into(), totals, by_model: finish_rows(by, false), windows, note }
}
fn with_cap(note: Option<String>, budget: &ScanBudget) -> Option<String> {
    if !budget.capped { return note; }
    let cap = format!("Scan stopped after {} MB of history (newest first); the oldest sessions are not counted.", MAX_SCAN_BYTES / (1024 * 1024));
    Some(match note { Some(n) => format!("{n} {cap}"), None => cap })
}

pub fn build_external(settings: &Settings, from: u64, to: u64) -> ExternalReport {
    let mut agents = vec![];
    {
        let mut b = ScanBudget::default();
        let recs = scan_claude(&claude_root(), from, to, &mut b);
        let note = with_cap(Some("Claude Code keeps subscription windows on the server, not on disk; run /usage inside Claude Code to see them.".into()), &b);
        agents.push(build_agent(settings, "claude-code", "Claude Code", "~/.claude/projects/<project>/<session>.jsonl", recs.is_some(), recs.unwrap_or_default(), vec![], note));
    }
    {
        let mut b = ScanBudget::default();
        let res = scan_codex(&codex_root(), from, to, &mut b);
        let available = res.is_some();
        let (recs, windows) = res.unwrap_or_default();
        agents.push(build_agent(settings, "codex", "Codex", "~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl", available, recs, windows, with_cap(None, &b)));
    }
    {
        let mut b = ScanBudget::default();
        let res = scan_opencode(&opencode_dir(), from, to, &mut b);
        let available = res.is_some();
        let (recs, note) = res.unwrap_or_default();
        agents.push(build_agent(settings, "opencode", "OpenCode", "~/.local/share/opencode/storage/message/<session>/*.json or opencode.db", available, recs, vec![], with_cap(note, &b)));
    }
    ExternalReport { agents, from, to }
}

static EXTERNAL_CACHE: Lazy<Mutex<HashMap<String, (Instant, ExternalReport)>>> = Lazy::new(|| Mutex::new(HashMap::new()));
/// `build_external` memoised for 60 s per range (and per custom-price table, since prices change the totals).
pub fn external_report(settings: &Settings, range: &str) -> ExternalReport {
    let key = format!("{range}|{}", serde_json::to_string(&settings.usage.custom_prices).unwrap_or_default());
    if let Some((t, r)) = EXTERNAL_CACHE.lock().get(&key) { if t.elapsed() < EXTERNAL_CACHE_TTL { return r.clone(); } }
    let (from, to) = range_bounds(range, now_ms());
    let report = build_external(settings, from, to);
    let mut cache = EXTERNAL_CACHE.lock();
    cache.retain(|_, (t, _)| t.elapsed() < EXTERNAL_CACHE_TTL);
    cache.insert(key, (Instant::now(), report.clone()));
    report
}

// ───────────────────────── backfill from saved sessions ─────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackfillReport { pub sessions: usize, pub imported: usize, pub skipped: usize }

const BACKFILL_MARKER: &str = ".sessions-backfilled-v1";

/// Import the usage recorded on assistant messages in every saved session (`<sessions_dir>/<workspace>/<id>.json`)
/// into the ledger, so history from before the ledger existed shows up. Idempotent: an entry whose
/// `(session_id, ts)` is already in the ledger is skipped. Messages without a cost are priced from the current
/// model prices when known.
pub fn backfill_from_sessions(settings: &Settings, ledger: &Ledger, sessions_dir: &Path) -> BackfillReport {
    use crate::ai::agent::session::{MessageRole, Session};
    let existing: HashSet<(String, u64)> = ledger.read_range(0, u64::MAX).into_iter().filter_map(|e| e.session_id.map(|s| (s, e.ts))).collect();
    let mut rep = BackfillReport::default();
    let Ok(workspaces) = std::fs::read_dir(sessions_dir) else { return rep };
    for ws in workspaces.flatten().filter(|e| e.path().is_dir()) {
        let Ok(files) = std::fs::read_dir(ws.path()) else { continue };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") || path.file_name().and_then(|n| n.to_str()) == Some("index.json") { continue; }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let Ok(session) = serde_json::from_str::<Session>(&text) else { continue };
            rep.sessions += 1;
            let agent = session.agent_id.clone().filter(|a| a != "caret" && !a.is_empty());
            for m in &session.messages {
                if !matches!(m.role, MessageRole::Assistant) { continue; }
                let Some(u) = &m.usage else { continue };
                if u.input_tokens + u.output_tokens + u.cache_read_tokens + u.cache_write_tokens == 0 && u.cost_usd.map(|c| c <= 0.0).unwrap_or(true) { continue; }
                if existing.contains(&(session.id.clone(), m.created_at)) { rep.skipped += 1; continue; }
                let model_id = m.model_id.clone().or_else(|| session.model_id.clone());
                let mut e = UsageEntry {
                    ts: m.created_at,
                    source: if agent.is_some() { "acp" } else { "agent" }.to_string(),
                    provider_id: model_id.as_deref().and_then(|id| settings.model(id)).map(|mc| mc.provider_id.clone()),
                    model_id,
                    agent_id: Some(agent.clone().unwrap_or_else(|| "caret".to_string())),
                    session_id: Some(session.id.clone()),
                    workspace: Some(session.workspace_root.clone()).filter(|w| !w.is_empty()),
                    input: u.input_tokens, output: u.output_tokens, cache_read: u.cache_read_tokens, cache_write: u.cache_write_tokens,
                    thinking: u.thinking_tokens.unwrap_or(0),
                    cost_usd: u.cost_usd,
                    duration_ms: m.duration_ms,
                };
                if e.cost_usd.is_none() { e.cost_usd = entry_cost(settings, &e); }
                match ledger.append(&e) { Ok(()) => rep.imported += 1, Err(err) => tracing::warn!(error = %err, "usage backfill: append failed") }
            }
        }
    }
    rep
}

/// Run the backfill once per ledger (marker file in the usage folder); later runs are no-ops unless forced.
pub fn backfill_once(settings: &Settings, force: bool) -> Option<BackfillReport> {
    let ledger = Ledger::open_default();
    let marker = ledger.dir().join(BACKFILL_MARKER);
    if marker.exists() && !force { return None; }
    let rep = backfill_from_sessions(settings, &ledger, &paths::sub_dir("sessions"));
    let _ = std::fs::create_dir_all(ledger.dir());
    let _ = std::fs::write(&marker, format!("{}\n", serde_json::to_string(&rep).unwrap_or_default()));
    tracing::info!(sessions = rep.sessions, imported = rep.imported, skipped = rep.skipped, "usage ledger backfilled from saved sessions");
    Some(rep)
}

// ───────────────────────── commands ─────────────────────────

fn check_range(range: &str) -> Result<()> { if RANGES.contains(&range) { Ok(()) } else { Err(CaretError::invalid(format!("unknown range `{range}`"))) } }

#[tauri::command(async)]
pub fn usage_summary(st: State<'_, SettingsState>, range: String, group_by: String, workspace: Option<String>) -> Result<Summary> {
    check_range(&range)?;
    if !GROUPS.contains(&group_by.as_str()) { return Err(CaretError::invalid(format!("unknown group_by `{group_by}`"))); }
    let settings = st.effective();
    let (from, to) = range_bounds(&range, now_ms());
    let entries = filter_workspace(Ledger::open_default().read_range(from, to), workspace.as_deref());
    Ok(summarize(&settings, &entries, &group_by, from, to))
}

/// Keep only entries recorded in `workspace` (the Settings page's Workspace scope); `None` keeps everything.
pub fn filter_workspace(entries: Vec<UsageEntry>, workspace: Option<&str>) -> Vec<UsageEntry> {
    match workspace.map(|w| w.trim_end_matches('/')).filter(|w| !w.is_empty()) {
        None => entries,
        Some(w) => entries.into_iter().filter(|e| e.workspace.as_deref().map(|x| x.trim_end_matches('/')) == Some(w)).collect(),
    }
}

#[tauri::command(async)]
pub fn usage_daily(st: State<'_, SettingsState>, range: String, workspace: Option<String>) -> Result<Vec<DailyPoint>> {
    check_range(&range)?;
    let settings = st.effective();
    let (from, to) = range_bounds(&range, now_ms());
    let entries = filter_workspace(Ledger::open_default().read_range(from, to), workspace.as_deref());
    Ok(daily(&settings, &entries, from, to))
}

#[tauri::command(async)]
pub fn usage_recent(limit: Option<usize>) -> Result<Vec<UsageEntry>> { Ok(Ledger::open_default().read_recent(limit.unwrap_or(50).clamp(1, 1000))) }

#[tauri::command(async)]
pub fn usage_export_csv(range: String, workspace: Option<String>) -> Result<String> {
    check_range(&range)?;
    let (from, to) = range_bounds(&range, now_ms());
    Ok(to_csv(&filter_workspace(Ledger::open_default().read_range(from, to), workspace.as_deref())))
}

#[tauri::command(async)]
pub fn usage_external(st: State<'_, SettingsState>, range: Option<String>) -> Result<ExternalReport> {
    let range = range.unwrap_or_else(|| "30d".into());
    check_range(&range)?;
    let settings = st.effective();
    let (from, to) = range_bounds(&range, now_ms());
    if !settings.usage.include_external { return Ok(ExternalReport { agents: vec![], from, to }); }
    Ok(external_report(&settings, &range))
}

/// Import usage from saved sessions into the ledger (idempotent). Runs automatically once per machine on startup;
/// the button on the Usage page calls it again after restoring sessions or when older history is missing.
#[tauri::command(async)]
pub fn usage_backfill(st: State<'_, SettingsState>) -> Result<BackfillReport> {
    Ok(backfill_once(&st.effective(), true).unwrap_or_default())
}

#[tauri::command(async)]
pub fn usage_dir() -> Result<String> { Ok(Ledger::default_dir().to_string_lossy().to_string()) }

// ───────────────────────── tests ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn delta(i: u64, o: u64, cr: u64, cw: u64) -> UsageDelta { UsageDelta { input_tokens: i, output_tokens: o, cache_read_tokens: cr, cache_write_tokens: cw, thinking_tokens: 0 } }
    fn entry(ts: u64, source: &str, model: &str, cost: Option<f64>) -> UsageEntry {
        let mut e = UsageEntry::new(source, &delta(1000, 200, 300, 100), cost);
        e.ts = ts; e.model_id = Some(model.into()); e.provider_id = Some(if model.starts_with("gpt") { "openai".into() } else { "anthropic".into() });
        e
    }
    fn ts_of(y: i32, m: u32, d: u32, h: u32) -> u64 { date_start_ms(NaiveDate::from_ymd_opt(y, m, d).unwrap()) + h as u64 * 3_600_000 }

    #[test]
    fn entry_roundtrips_and_detects_empty() {
        let e = entry(1_700_000_000_000, "agent", "claude-opus-5-5", Some(0.5));
        let back: UsageEntry = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(back, e);
        assert!(!e.is_empty());
        assert!(UsageEntry::new("tab", &UsageDelta::default(), None).is_empty());
        assert!(UsageEntry::new("acp", &UsageDelta::default(), Some(0.0)).is_empty());
        assert!(!UsageEntry::new("acp", &UsageDelta::default(), Some(0.01)).is_empty());
        let json = serde_json::to_value(&e).unwrap();
        assert!(json.get("cacheRead").is_some() && json.get("durationMs").is_none(), "camelCase, optional fields skipped");
    }

    #[test]
    fn ledger_appends_across_month_boundary_and_reads_ranges() {
        let dir = tempfile::tempdir().unwrap();
        let l = Ledger::new(dir.path());
        let jan = ts_of(2026, 1, 31, 23);
        let feb = ts_of(2026, 2, 1, 1);
        let mar = ts_of(2026, 3, 15, 12);
        for (i, t) in [jan, feb, mar].iter().enumerate() { l.append(&entry(*t, "agent", "claude-opus-5-5", Some(i as f64))).unwrap(); }
        assert!(l.file_for(jan).ends_with("2026-01.jsonl") && l.file_for(feb).ends_with("2026-02.jsonl"));
        assert_eq!(l.month_files().iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), vec!["2026-01", "2026-02", "2026-03"]);
        let all = l.read_range(0, u64::MAX);
        assert_eq!(all.iter().map(|e| e.ts).collect::<Vec<_>>(), vec![jan, feb, mar], "sorted oldest first");
        let span = l.read_range(jan, feb);
        assert_eq!(span.len(), 2, "range spanning the month boundary reads both files");
        let only_feb = l.read_range(feb - 1, feb + 1);
        assert_eq!(only_feb.len(), 1);
        assert_eq!(only_feb[0].ts, feb);
        let recent = l.read_recent(2);
        assert_eq!(recent.iter().map(|e| e.ts).collect::<Vec<_>>(), vec![mar, feb], "newest first");
        // Junk files and lines are ignored.
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
        std::fs::write(dir.path().join("2026-03.jsonl"), format!("{}\nnot json\n", serde_json::to_string(&entry(mar, "tab", "gpt-5", Some(0.1))).unwrap())).unwrap();
        assert_eq!(l.read_range(0, u64::MAX).len(), 3);
    }

    #[test]
    fn range_bounds_follow_local_calendar() {
        let now = ts_of(2026, 3, 10, 15);
        let (f, t) = range_bounds("today", now);
        assert_eq!((f, t), (ts_of(2026, 3, 10, 0), now));
        assert_eq!(range_bounds("7d", now).0, ts_of(2026, 3, 4, 0), "7 days including today");
        assert_eq!(range_bounds("30d", now).0, ts_of(2026, 2, 9, 0));
        assert_eq!(range_bounds("all", now), (0, now));
        assert_eq!(day_key(now), "2026-03-10");
        assert_eq!(month_key(now), "2026-03");
        let (ms, me) = month_bounds("2026-12").unwrap();
        assert_eq!(ms, ts_of(2026, 12, 1, 0));
        assert_eq!(me, ts_of(2027, 1, 1, 0));
        assert!(month_bounds("abcd-ef").is_none());
        assert_eq!(parse_iso_ms("2026-03-10T12:00:00.000Z"), Some(1_773_144_000_000));
    }

    #[test]
    fn summarize_groups_by_every_dimension() {
        let settings = Settings::default();
        let t = ts_of(2026, 3, 10, 9);
        let mut a = entry(t, "agent", "claude-opus-5-5", Some(0.5)); a.agent_id = Some("caret".into()); a.workspace = Some("/w/alpha".into());
        let mut b = entry(t + 3_600_000, "inline", "gpt-5", Some(0.25)); b.workspace = Some("/w/beta".into());
        let mut c = entry(t + 86_400_000, "acp", "claude-opus-5-5", Some(0.75)); c.agent_id = Some("claude-code".into()); c.workspace = Some("/w/alpha".into());
        let entries = vec![a, b, c];
        let s = summarize(&settings, &entries, "model", 0, t + 2 * 86_400_000);
        assert_eq!(s.totals.requests, 3);
        assert_eq!(s.totals.input, 3000);
        assert!((s.totals.cost_usd.unwrap() - 1.5).abs() < 1e-9);
        assert!((s.totals.cache_hit_rate - 900.0 / 4200.0).abs() < 1e-9);
        assert_eq!(s.rows.len(), 2);
        assert_eq!(s.rows[0].key, "claude-opus-5-5", "sorted by cost desc");
        assert_eq!(s.rows[0].label, "Claude Opus 5.5", "catalog display name");
        assert_eq!(s.rows[0].totals.requests, 2);
        let by = |g: &str| summarize(&settings, &entries, g, 0, u64::MAX).rows.into_iter().map(|r| (r.key, r.label, r.totals.requests)).collect::<Vec<_>>();
        assert_eq!(by("provider"), vec![("anthropic".into(), "Anthropic".into(), 2), ("openai".into(), "OpenAI".into(), 1)]);
        // `b` has no agent id and counts as Caret: both agents cost 0.75, so the tie breaks on request count.
        let agents = by("agent");
        assert_eq!(agents.iter().map(|r| (r.0.as_str(), r.2)).collect::<Vec<_>>(), vec![("caret", 2), ("claude-code", 1)]);
        assert_eq!(agents[0].1, "Caret");
        let sources = by("source");
        assert_eq!(sources.iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), vec!["External agent", "Agent chat", "Inline edit"]);
        let days = by("day");
        assert_eq!(days.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), vec!["2026-03-10", "2026-03-11"], "day rows ascending");
        assert_eq!(days[0].2, 2);
        let ws = by("workspace");
        assert_eq!(ws[0], ("/w/alpha".into(), "alpha".into(), 2));
        // flattened row serialisation
        let v = serde_json::to_value(&s.rows[0]).unwrap();
        assert!(v.get("costUsd").is_some() && v.get("cacheHitRate").is_some() && v.get("totals").is_none());
    }

    #[test]
    fn summarize_filters_by_range_and_prices_unpriced_entries() {
        let mut settings = Settings::default();
        let now = ts_of(2026, 3, 10, 15);
        let (from, to) = range_bounds("today", now);
        let dir = tempfile::tempdir().unwrap();
        let l = Ledger::new(dir.path());
        l.append(&entry(now - 2 * 86_400_000, "agent", "claude-opus-5-5", Some(9.0))).unwrap();
        l.append(&entry(now - 60_000, "agent", "claude-opus-5-5", Some(1.0))).unwrap();
        let mut unpriced = entry(now - 30_000, "acp", "mystery-model", None); unpriced.cost_usd = None;
        l.append(&unpriced).unwrap();
        let mut external_model = entry(now - 10_000, "acp", "gpt-5-codex", None); external_model.cost_usd = None;
        l.append(&external_model).unwrap();
        let entries = l.read_range(from, to);
        assert_eq!(entries.len(), 3, "the two-day-old entry is outside today");
        let s = summarize(&settings, &entries, "model", from, to);
        assert_eq!(s.totals.unpriced, 1);
        // 1.0 + gpt-5-codex via built-in table: 1000*1.25 + 200*10 + 300*0.125 + 100*1.5625 (1.25x input) per million
        let expected = 1.0 + (1000.0 * 1.25 + 200.0 * 10.0 + 300.0 * 0.125 + 100.0 * 1.25 * 1.25) / 1e6;
        assert!((s.totals.cost_usd.unwrap() - expected).abs() < 1e-9, "{}", s.totals.cost_usd.unwrap());
        let mystery = s.rows.iter().find(|r| r.key == "mystery-model").unwrap();
        assert_eq!(mystery.totals.cost_usd, None, "unknown model has no cost");
        assert_eq!(mystery.totals.unpriced, 1);
        // A custom price makes it priced.
        settings.usage.custom_prices.insert("mystery-model".into(), ModelPrice { input: 1.0, output: 2.0, cache_read: Some(0.5), cache_write: None });
        let s2 = summarize(&settings, &entries, "model", from, to);
        let mystery = s2.rows.iter().find(|r| r.key == "mystery-model").unwrap();
        let want = (1000.0 * 1.0 + 200.0 * 2.0 + 300.0 * 0.5 + 100.0 * 1.25) / 1e6;
        assert!((mystery.totals.cost_usd.unwrap() - want).abs() < 1e-12);
        assert_eq!(s2.totals.unpriced, 0);
        // daily is zero-filled over the range
        let d = daily(&settings, &entries, range_bounds("7d", now).0, now);
        assert_eq!(d.len(), 7);
        assert_eq!(d[6].day, "2026-03-10");
        assert_eq!(d[6].tokens, 3 * 1600);
        assert!(d[0].cost_usd == 0.0 && d[0].tokens == 0);
        assert!(daily(&settings, &[], 0, now).is_empty(), "all-time with no entries");
        assert_eq!(daily(&settings, &entries, 0, now).len(), 1, "all-time starts at the oldest entry");
    }

    #[test]
    fn price_resolution_order_and_normalisation() {
        assert_eq!(normalize_model_id("anthropic/Claude-Sonnet-4-5-20250929"), "claude-sonnet-4-5-20250929");
        assert_eq!(normalize_model_id("claude-opus-4-1[1m]"), "claude-opus-4-1");
        assert_eq!(normalize_model_id("gemini-2.5-pro@001"), "gemini-2.5-pro");
        assert_eq!(normalize_model_id("claude-3-5-sonnet-latest"), "claude-3-5-sonnet");
        let p = builtin_prices("claude-sonnet-4-5-20250929").unwrap();
        assert_eq!((p.input, p.output, p.cache_read, p.cache_write), (3.0, 15.0, 0.30, 3.75));
        assert_eq!(builtin_prices("gpt-5-codex").unwrap().input, 1.25);
        assert_eq!(builtin_prices("gpt-5.1-codex-max").unwrap().output, 10.0);
        assert_eq!(builtin_prices("gpt-5-mini").unwrap().input, 0.25, "longest prefix wins over gpt-5");
        assert_eq!(builtin_prices("o3-2025-04-16").unwrap().input, 2.0);
        assert_eq!(builtin_prices("gemini-3-pro-preview").unwrap().input, 2.0);
        assert_eq!(builtin_prices("grok-4-fast-reasoning").unwrap().output, 0.50);
        assert!(builtin_prices("gpt-50").is_none(), "prefix must end on a separator");
        assert!(builtin_prices("totally-unknown").is_none());
        let mut settings = Settings::default();
        // catalog entry wins over the built-in table
        let cat = resolve_prices(&settings, "gemini-2.5-pro").unwrap();
        assert_eq!(cat.cache_read, 0.31);
        // custom beats catalog, exact or prefix, case-insensitive
        settings.usage.custom_prices.insert("Gemini-2.5-Pro".into(), ModelPrice { input: 7.0, output: 8.0, cache_read: None, cache_write: None });
        let c = resolve_prices(&settings, "gemini-2.5-pro").unwrap();
        assert_eq!((c.input, c.output), (7.0, 8.0));
        assert!((c.cache_read - 0.7).abs() < 1e-12 && (c.cache_write - 8.75).abs() < 1e-12, "{c:?}");
        settings.usage.custom_prices.insert("my-local".into(), ModelPrice { input: 0.0, output: 0.0, cache_read: None, cache_write: None });
        assert_eq!(resolve_prices(&settings, "my-local-7b").unwrap().cost(1_000_000, 1, 1, 1), 0.0);
        assert!(resolve_prices(&settings, "nope").is_none());
        let cost = Prices::per_m(1.0, 2.0, None, None).cost(1_000_000, 500_000, 1_000_000, 1_000_000);
        assert!((cost - (1.0 + 1.0 + 0.1 + 1.25)).abs() < 1e-12);
    }

    #[test]
    fn csv_export_escapes_fields() {
        let mut e = entry(1_700_000_000_000, "agent", "claude-opus-5-5", Some(0.123456789));
        e.workspace = Some("/w/has,comma".into()); e.session_id = Some("say \"hi\"".into()); e.duration_ms = Some(42);
        let csv = to_csv(&[e, entry(1_700_000_001_000, "tab", "gpt-5", None)]);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("ts,time,source,provider,model,agent,session,workspace,input,output,cache_read,cache_write,thinking,cost_usd,duration_ms"));
        assert!(lines[1].contains("\"/w/has,comma\"") && lines[1].contains("\"say \"\"hi\"\"\""), "{}", lines[1]);
        assert!(lines[1].ends_with(",0.123457,42"));
        assert!(lines[2].ends_with(",1000,200,300,100,0,,"), "{}", lines[2]);
    }

    #[test]
    fn parses_claude_code_transcripts_with_dedupe() {
        let line = |id: &str, ts: &str, model: &str, i: u64, o: u64| json!({ "type": "assistant", "timestamp": ts, "requestId": format!("req_{id}"), "message": { "id": id, "model": model, "usage": { "input_tokens": i, "output_tokens": o, "cache_read_input_tokens": 5000, "cache_creation_input_tokens": 700, "cache_creation": { "ephemeral_5m_input_tokens": 700, "ephemeral_1h_input_tokens": 0 } } } }).to_string();
        let text = [
            json!({ "type": "user", "timestamp": "2026-03-10T10:00:00.000Z", "message": { "role": "user", "content": "hi" } }).to_string(),
            line("msg_1", "2026-03-10T10:00:01.000Z", "claude-sonnet-4-5-20250929", 100, 20),
            line("msg_1", "2026-03-10T10:00:01.500Z", "claude-sonnet-4-5-20250929", 100, 20),
            line("msg_1", "2026-03-10T10:00:02.000Z", "claude-sonnet-4-5-20250929", 100, 20),
            line("msg_2", "2026-03-10T10:05:00.000Z", "claude-opus-4-1-20250805", 10, 1),
            line("msg_3", "2026-03-10T10:06:00.000Z", "<synthetic>", 1, 1),
            line("msg_4", "2026-03-01T10:06:00.000Z", "claude-sonnet-4-5-20250929", 1, 1),
            "garbage line".to_string(),
            json!({ "type": "cost-state", "totalCostUSD": 1.0 }).to_string(),
        ].join("\n");
        let from = parse_iso_ms("2026-03-09T00:00:00Z").unwrap();
        let to = parse_iso_ms("2026-03-11T00:00:00Z").unwrap();
        let mut seen = HashSet::new();
        let recs = parse_claude_jsonl(&text, from, to, &mut seen);
        assert_eq!(recs.len(), 2, "three block lines of msg_1 collapse to one; synthetic and out-of-range dropped");
        assert_eq!(recs[0].model, "claude-sonnet-4-5-20250929");
        assert_eq!((recs[0].input, recs[0].output, recs[0].cache_read, recs[0].cache_write), (100, 20, 5000, 700));
        let agent = build_agent(&Settings::default(), "claude-code", "Claude Code", "p", true, recs, vec![], None);
        assert_eq!(agent.totals.requests, 2);
        assert_eq!(agent.by_model.len(), 2);
        let sonnet = agent.by_model.iter().find(|r| r.key.starts_with("claude-sonnet")).unwrap();
        let want = (100.0 * 3.0 + 20.0 * 15.0 + 5000.0 * 0.30 + 700.0 * 3.75) / 1e6;
        assert!((sonnet.totals.cost_usd.unwrap() - want).abs() < 1e-12);
        assert_eq!(agent.by_model[0].key, "claude-opus-4-1-20250805", "most expensive first (opus cache writes dominate)");
        assert_eq!(agent.totals.unpriced, 0);
    }

    #[test]
    fn parses_codex_rollouts_tokens_and_rate_limits() {
        let text = [
            json!({ "timestamp": "2026-03-10T09:00:00.000Z", "type": "session_meta", "payload": { "id": "s1", "cwd": "/w", "model_provider": "openai" } }).to_string(),
            json!({ "timestamp": "2026-03-10T09:00:01.000Z", "type": "turn_context", "payload": { "cwd": "/w", "model": "gpt-5-codex" } }).to_string(),
            json!({ "timestamp": "2026-03-10T09:00:05.000Z", "type": "event_msg", "payload": { "type": "token_count", "info": { "total_token_usage": { "input_tokens": 1000, "cached_input_tokens": 800, "output_tokens": 50, "reasoning_output_tokens": 10, "total_tokens": 1050 }, "last_token_usage": { "input_tokens": 1000, "cached_input_tokens": 800, "output_tokens": 50, "reasoning_output_tokens": 10, "total_tokens": 1050 }, "model_context_window": 272000 }, "rate_limits": { "primary": { "used_percent": 12.5, "window_minutes": 300, "resets_in_seconds": 600 }, "secondary": { "used_percent": 40.0, "window_minutes": 10080, "resets_at": 1773400000 } } } }).to_string(),
            json!({ "timestamp": "2026-03-10T09:01:00.000Z", "type": "event_msg", "payload": { "type": "token_count", "info": null, "rate_limits": { "primary": { "used_percent": 13.0, "window_minutes": 300, "resets_in_seconds": 540 } } } }).to_string(),
            json!({ "timestamp": "2026-03-10T09:02:00.000Z", "type": "event_msg", "payload": { "type": "token_count", "info": { "total_token_usage": { "input_tokens": 1500, "cached_input_tokens": 1200, "output_tokens": 80 }, "last_token_usage": { "input_tokens": 500, "cached_input_tokens": 400, "output_tokens": 30, "reasoning_output_tokens": 0 } } } }).to_string(),
            json!({ "timestamp": "2026-03-10T09:03:00.000Z", "type": "event_msg", "payload": { "type": "agent_message", "message": "done" } }).to_string(),
            json!({ "timestamp": "2026-03-12T09:03:00.000Z", "type": "event_msg", "payload": { "type": "token_count", "info": { "last_token_usage": { "input_tokens": 999, "cached_input_tokens": 0, "output_tokens": 1 } } } }).to_string(),
        ].join("\n");
        let from = parse_iso_ms("2026-03-10T00:00:00Z").unwrap();
        let to = parse_iso_ms("2026-03-11T00:00:00Z").unwrap();
        let (recs, latest) = parse_codex_jsonl(&text, from, to);
        assert_eq!(recs.len(), 2, "two token_count events with usage in range");
        assert_eq!(recs[0].model, "gpt-5-codex");
        assert_eq!((recs[0].input, recs[0].cache_read, recs[0].output, recs[0].thinking), (200, 800, 50, 10), "cached input split out of input");
        assert_eq!((recs[1].input, recs[1].cache_read, recs[1].output), (100, 400, 30));
        let (ts, windows) = latest.unwrap();
        assert_eq!(ts, parse_iso_ms("2026-03-10T09:01:00.000Z").unwrap(), "newest rate_limits win even without usage info");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].label, "5 hours");
        assert_eq!(windows[0].used_percent, 13.0);
        assert_eq!(windows[0].resets_at, Some(ts + 540_000));
        let first = parse_codex_rate_limits(&json!({ "primary": { "used_percent": 12.5, "window_minutes": 300, "resets_in_seconds": 600 }, "secondary": { "used_percent": 40.0, "window_minutes": 10080, "resets_at": 1773400000 } }), 1_000);
        assert_eq!(first.len(), 2);
        assert_eq!(first[1].label, "Weekly");
        assert_eq!(first[1].resets_at, Some(1_773_400_000_000), "epoch seconds become ms");
        assert_eq!(window_label(60), "1 hour");
        assert_eq!(window_label(1440), "Daily");
        assert_eq!(window_label(45), "45 min");
        let agent = build_agent(&Settings::default(), "codex", "Codex", "p", true, recs, windows, None);
        let want = (300.0 * 1.25 + 80.0 * 10.0 + 1200.0 * 0.125) / 1e6;
        assert!((agent.totals.cost_usd.unwrap() - want).abs() < 1e-12);
        assert!((agent.totals.cache_hit_rate - 1200.0 / 1500.0).abs() < 1e-12);
    }

    #[test]
    fn parses_opencode_messages_from_json_tree() {
        let dir = tempfile::tempdir().unwrap();
        let msgs = dir.path().join("storage").join("message").join("ses_1");
        std::fs::create_dir_all(&msgs).unwrap();
        let t = parse_iso_ms("2026-03-10T09:00:00Z").unwrap();
        std::fs::write(msgs.join("msg_user.json"), json!({ "id": "msg_user", "role": "user", "time": { "created": t } }).to_string()).unwrap();
        std::fs::write(msgs.join("msg_a.json"), json!({ "id": "msg_a", "role": "assistant", "modelID": "claude-sonnet-4-5", "providerID": "anthropic", "cost": 0.0123, "tokens": { "input": 100, "output": 50, "reasoning": 7, "cache": { "read": 2000, "write": 300 } }, "time": { "created": t + 1000, "completed": t + 5000 } }).to_string()).unwrap();
        std::fs::write(msgs.join("msg_b.json"), json!({ "id": "msg_b", "role": "assistant", "modelID": "some-local-model", "providerID": "ollama", "cost": 0, "tokens": { "input": 10, "output": 5, "reasoning": 0, "cache": { "read": 0, "write": 0 } }, "time": { "created": t + 2000 } }).to_string()).unwrap();
        std::fs::write(msgs.join("msg_old.json"), json!({ "id": "msg_old", "role": "assistant", "modelID": "claude-sonnet-4-5", "providerID": "anthropic", "cost": 5.0, "tokens": { "input": 1, "output": 1, "reasoning": 0, "cache": { "read": 0, "write": 0 } }, "time": { "created": t - 86_400_000 * 5 } }).to_string()).unwrap();
        let mut budget = ScanBudget::default();
        let (recs, note) = scan_opencode(dir.path(), t - 3_600_000, t + 3_600_000, &mut budget).expect("storage dir present");
        assert!(note.is_none());
        assert_eq!(recs.len(), 2);
        let a = recs.iter().find(|r| r.model == "claude-sonnet-4-5").unwrap();
        assert_eq!((a.input, a.output, a.cache_read, a.cache_write, a.thinking), (100, 50, 2000, 300, 7));
        assert_eq!(a.cost, Some(0.0123), "reported cost is kept");
        let b = recs.iter().find(|r| r.model == "some-local-model").unwrap();
        assert_eq!(b.cost, None, "zero cost falls back to the price table");
        let agent = build_agent(&Settings::default(), "opencode", "OpenCode", "p", true, recs, vec![], None);
        assert_eq!(agent.totals.unpriced, 1);
        assert!((agent.totals.cost_usd.unwrap() - 0.0123).abs() < 1e-12);
        assert!(scan_opencode(&dir.path().join("missing"), 0, u64::MAX, &mut ScanBudget::default()).is_none());
        assert!(parse_opencode_message(&json!({ "role": "assistant" }), 0, u64::MAX).is_none(), "no time.created");
    }

    #[test]
    fn scan_budget_skips_old_files_and_caps_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.jsonl");
        std::fs::write(&f, "x").unwrap();
        let mut b = ScanBudget::default();
        assert!(b.take(&f, 0).is_some());
        assert_eq!((b.bytes, b.files), (1, 1));
        assert!(b.take(&f, now_ms() + 86_400_000).is_none(), "modified before `from`");
        assert!(!b.capped);
        let mut full = ScanBudget { bytes: MAX_SCAN_BYTES, ..Default::default() };
        assert!(full.take(&f, 0).is_none());
        assert!(full.capped);
        assert!(with_cap(None, &full).unwrap().contains("1024 MB"));
        assert_eq!(with_cap(Some("n".into()), &b), Some("n".into()));
        let claude = scan_claude(dir.path(), 0, u64::MAX, &mut ScanBudget::default());
        assert!(claude.is_none(), "no projects dir means not installed");
        assert!(scan_codex(dir.path(), 0, u64::MAX, &mut ScanBudget::default()).is_none());
    }

    #[test]
    fn external_report_marks_missing_agents() {
        let dir = tempfile::tempdir().unwrap();
        let projects = dir.path().join("projects").join("-w-alpha");
        std::fs::create_dir_all(&projects).unwrap();
        let t = now_ms() - 60_000;
        let iso = chrono::DateTime::<chrono::Utc>::from(UNIX_EPOCH + Duration::from_millis(t)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        std::fs::write(projects.join("s.jsonl"), json!({ "type": "assistant", "timestamp": iso, "message": { "id": "m1", "model": "claude-haiku-4-5-20251001", "usage": { "input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0 } } }).to_string()).unwrap();
        let (from, to) = range_bounds("today", now_ms());
        let mut b = ScanBudget::default();
        let recs = scan_claude(dir.path(), from, to, &mut b).unwrap();
        assert_eq!(recs.len(), 1);
        let agent = build_agent(&Settings::default(), "claude-code", "Claude Code", "p", true, recs, vec![], None);
        assert_eq!(agent.totals.requests, 1);
        assert!(agent.totals.cost_usd.unwrap() > 0.0);
        let missing = build_agent(&Settings::default(), "codex", "Codex", "p", false, vec![], vec![], None);
        assert!(!missing.available && missing.totals.requests == 0 && missing.by_model.is_empty());
        let v = serde_json::to_value(&ExternalReport { agents: vec![missing], from, to }).unwrap();
        assert_eq!(v["agents"][0]["sourcePathPattern"], "p");
        assert_eq!(v["agents"][0]["totals"]["costUsd"], Value::Null);
    }

    #[test]
    fn backfill_imports_saved_sessions_once() {
        use crate::ai::agent::session::{ChatMessage, MessageRole, Session, Usage};
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions"); let ledger = Ledger::new(dir.path().join("usage"));
        let mut s = Session::default(); s.workspace_root = "/w/proj".into(); s.model_id = Some("gpt-5-codex".into());
        let mut a = ChatMessage::new(MessageRole::Assistant); a.created_at = 1_700_000_000_000; a.model_id = Some("gpt-5-codex".into());
        a.usage = Some(Usage { input_tokens: 1000, output_tokens: 200, cache_read_tokens: 300, cache_write_tokens: 0, thinking_tokens: None, cost_usd: None, requests: Some(1) });
        let mut b = ChatMessage::new(MessageRole::Assistant); b.created_at = 1_700_000_060_000;
        b.usage = Some(Usage { input_tokens: 10, output_tokens: 5, cache_read_tokens: 0, cache_write_tokens: 0, thinking_tokens: Some(3), cost_usd: Some(0.0), requests: Some(1) });
        let mut user = ChatMessage::new(MessageRole::User); user.usage = Some(Usage { input_tokens: 99, ..Default::default() }); // never counted
        s.messages = vec![user, a, b];
        let ws = sessions.join("abcd"); std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join(format!("{}.json", s.id)), serde_json::to_string(&s).unwrap()).unwrap();
        std::fs::write(ws.join("index.json"), "[]").unwrap();
        let settings = Settings::default();
        let rep = backfill_from_sessions(&settings, &ledger, &sessions);
        assert_eq!((rep.sessions, rep.imported, rep.skipped), (1, 2, 0));
        let entries = ledger.read_range(0, u64::MAX);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].session_id.as_deref(), Some(s.id.as_str()));
        assert_eq!(entries[0].source, "agent");
        assert_eq!(entries[0].workspace.as_deref(), Some("/w/proj"));
        assert_eq!((entries[0].input, entries[0].output, entries[0].cache_read), (1000, 200, 300));
        // priced from the built-in table because the message carried no cost
        let want = (1000.0 * 1.25 + 200.0 * 10.0 + 300.0 * 0.125) / 1e6;
        assert!((entries[0].cost_usd.unwrap() - want).abs() < 1e-9, "{:?}", entries[0].cost_usd);
        assert_eq!(entries[1].cost_usd, Some(0.0));
        assert_eq!(entries[1].thinking, 3);
        // second run is a no-op
        let rep2 = backfill_from_sessions(&settings, &ledger, &sessions);
        assert_eq!((rep2.imported, rep2.skipped), (0, 2));
        assert_eq!(ledger.read_range(0, u64::MAX).len(), 2);
    }

    #[test]
    fn files_under_lists_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.jsonl"); let new = dir.path().join("new.jsonl");
        std::fs::write(&old, "a").unwrap();
        let t = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::open(&old).unwrap().set_modified(t).unwrap();
        std::fs::write(&new, "b").unwrap();
        let v = files_under(dir.path(), 2, "jsonl");
        assert_eq!(v, vec![new, old]);
    }

    #[test]
    fn workspace_filter_matches_root_ignoring_trailing_slash() {
        let mk = |w: Option<&str>| UsageEntry { ts: 1, source: "agent".into(), workspace: w.map(String::from), input: 1, ..Default::default() };
        let all = vec![mk(Some("/w/a")), mk(Some("/w/b/")), mk(None)];
        assert_eq!(filter_workspace(all.clone(), None).len(), 3);
        assert_eq!(filter_workspace(all.clone(), Some("")).len(), 3);
        assert_eq!(filter_workspace(all.clone(), Some("/w/a/")).len(), 1);
        assert_eq!(filter_workspace(all.clone(), Some("/w/b")).len(), 1);
        assert_eq!(filter_workspace(all, Some("/w/c")).len(), 0);
    }
}

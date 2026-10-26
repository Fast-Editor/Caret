//! Verified Diff — verification engine. Produces a `VerificationReport` for a scope (session changes,
//! staged, working tree, files, branch): diagnostics + project check, dependency grounding, security rules,
//! tests, diff-scoped mutation score, lint; then a risk tier and machine "facts" vs agent "claims".
pub mod checks;
pub mod risk;
pub mod scope;
pub mod shell;
pub mod store;

use crate::ai::agent::manager::AgentManager;
use crate::ai::agent::session::AgentEvent;
use crate::error::{CaretError, Result};
use crate::events;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tokio_util::sync::CancellationToken;

// ───────────────────────── contract types (mirror src/types.ts) ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyScope {
    pub kind: String, // session | change-set | staged | working-tree | files | branch
    #[serde(default, skip_serializing_if = "Option::is_none")] pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub change_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub files: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub base: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyFinding {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub end_line: Option<u32>,
    pub severity: String, // critical | high | medium | low | info
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub suggestion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub fixable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub evidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub url: Option<String>,
}
impl VerifyFinding {
    pub fn new(severity: &str, message: impl Into<String>) -> Self {
        Self { id: crate::ai::agent::session::short_id(), path: None, line: None, end_line: None, severity: severity.into(), message: message.into(), rule: None, suggestion: None, fixable: None, evidence: None, url: None }
    }
    pub fn at(mut self, path: &str, line: Option<u32>) -> Self { self.path = Some(path.to_string()); self.line = line; self }
    pub fn rule(mut self, r: &str) -> Self { self.rule = Some(r.to_string()); self }
    pub fn suggest(mut self, s: impl Into<String>) -> Self { self.suggestion = Some(s.into()); self }
    pub fn evidence(mut self, e: impl Into<String>) -> Self { self.evidence = Some(e.into()); self }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyCheck {
    pub id: String,
    pub kind: String, // diagnostics | project-check | dependencies | security | tests | mutation | equivalence | properties | lint
    pub title: String,
    pub status: String, // pending | running | pass | warn | fail | skipped | error
    pub summary: String,
    pub findings: Vec<VerifyFinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub skipped_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub metrics: Option<std::collections::BTreeMap<String, f64>>,
}
impl VerifyCheck {
    pub fn pending(kind: &str, title: &str) -> Self { Self { id: kind.to_string(), kind: kind.to_string(), title: title.to_string(), status: "pending".into(), summary: String::new(), findings: vec![], duration_ms: None, command: None, output: None, skipped_reason: None, metrics: None } }
    pub fn skip(&mut self, reason: impl Into<String>) { self.status = "skipped".into(); let r = reason.into(); self.summary = r.clone(); self.skipped_reason = Some(r); }
    pub fn metric(&mut self, k: &str, v: f64) { self.metrics.get_or_insert_with(Default::default).insert(k.to_string(), v); }
    pub fn worst_severity(&self) -> Option<&str> {
        let order = ["critical", "high", "medium", "low", "info"];
        order.iter().copied().find(|s| self.findings.iter().any(|f| f.severity == *s))
    }
    /// Status from findings: fail on critical/high (or `fail_on_medium`), warn on medium/low, pass otherwise.
    pub fn settle(&mut self, fail_on_medium: bool) {
        self.status = match self.worst_severity() {
            Some("critical") | Some("high") => "fail".into(),
            Some("medium") => if fail_on_medium { "fail".into() } else { "warn".into() },
            Some("low") | Some("info") => "warn".into(),
            _ => "pass".into(),
        };
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReport {
    pub id: String,
    pub scope: VerifyScope,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub session_id: Option<String>,
    pub workspace_root: String,
    pub started_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub finished_at: Option<u64>,
    pub status: String,
    pub checks: Vec<VerifyCheck>,
    pub risk_tier: u8,
    pub risk_reasons: Vec<String>,
    pub facts: Vec<String>,
    pub claims: Vec<String>,
    pub files_touched: Vec<String>,
    pub additions: u32,
    pub deletions: u32,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum VerifyEvent {
    Report { report: VerificationReport },
    Check { report_id: String, check: VerifyCheck },
    Done { report: VerificationReport },
    Error { #[serde(default, skip_serializing_if = "Option::is_none")] report_id: Option<String>, message: String },
}

pub const VERIFY_CHANGED: &str = "caret://verify/changed";
const MEMORY_REPORTS: usize = 50;

// ───────────────────────── state ─────────────────────────

pub struct VerifyInner {
    /// Most recent reports (bounded).
    reports: RwLock<HashMap<String, VerificationReport>>,
    order: RwLock<Vec<String>>,
    running: dashmap::DashMap<String, CancellationToken>,
    /// One verification at a time per process (they share the working tree and the test runner).
    lock: tokio::sync::Mutex<()>,
}
#[derive(Clone)]
pub struct VerifyState(pub Arc<VerifyInner>);
impl Default for VerifyState {
    fn default() -> Self { Self(Arc::new(VerifyInner { reports: RwLock::new(HashMap::new()), order: RwLock::new(vec![]), running: dashmap::DashMap::new(), lock: tokio::sync::Mutex::new(()) })) }
}
impl VerifyInner {
    pub fn put(&self, r: VerificationReport) {
        let id = r.id.clone();
        { let mut o = self.order.write(); if !o.contains(&id) { o.push(id.clone()); } while o.len() > MEMORY_REPORTS { let old = o.remove(0); self.reports.write().remove(&old); } }
        self.reports.write().insert(id, r);
    }
    pub fn get(&self, id: &str) -> Option<VerificationReport> { self.reports.read().get(id).cloned() }
    pub fn list(&self, session_id: Option<&str>, limit: usize) -> Vec<VerificationReport> {
        let o = self.order.read(); let r = self.reports.read();
        o.iter().rev().filter_map(|id| r.get(id)).filter(|rep| session_id.map(|s| rep.session_id.as_deref() == Some(s)).unwrap_or(true)).take(limit).cloned().collect()
    }
}

/// Fan-out for progress: optional Channel + the agent event stream (when the scope has a session).
#[derive(Clone)]
pub struct Emitter { app: AppHandle, channel: Option<Arc<Channel<VerifyEvent>>>, session_id: Option<String> }
impl Emitter {
    pub fn send(&self, ev: VerifyEvent) { if let Some(ch) = &self.channel { let _ = ch.send(ev.clone()); } }
    pub fn check(&self, report_id: &str, check: &VerifyCheck) { self.send(VerifyEvent::Check { report_id: report_id.to_string(), check: check.clone() }); }
    /// Progress snapshot for the chat UI (proof panel shows "Verifying…" with per-check status while running).
    pub fn progress(&self, report: &VerificationReport) {
        if let Some(sid) = &self.session_id { let mgr = self.app.state::<Arc<AgentManager>>(); mgr.emit(AgentEvent::Verification { session_id: sid.clone(), report: report.clone() }); }
    }
    fn done(&self, report: &VerificationReport) {
        self.send(VerifyEvent::Done { report: report.clone() });
        events::emit(&self.app, VERIFY_CHANGED, serde_json::json!({ "reportId": report.id, "sessionId": report.session_id, "status": report.status, "riskTier": report.risk_tier }));
        if let Some(sid) = &self.session_id {
            let mgr = self.app.state::<Arc<AgentManager>>();
            mgr.set_last_verification(sid, &report.id);
            mgr.emit(AgentEvent::Verification { session_id: sid.clone(), report: report.clone() });
        }
    }
}

fn now_ms() -> u64 { crate::ai::agent::session::now_ms() }

/// Start a verification; returns the report id immediately and streams progress.
pub async fn start(app: AppHandle, st: Arc<VerifyInner>, scope: VerifyScope, channel: Option<Channel<VerifyEvent>>) -> Result<String> {
    let root = app.state::<crate::core::workspace::WorkspaceState>().root().ok_or_else(CaretError::no_workspace)?;
    let id = crate::ai::agent::session::new_id();
    let settings = app.state::<crate::core::settings::SettingsState>().effective();
    let session_id = scope.session_id.clone();
    let report = VerificationReport { id: id.clone(), scope: scope.clone(), session_id: session_id.clone(), workspace_root: root.to_string_lossy().to_string(), started_at: now_ms(), finished_at: None, status: "running".into(), checks: checks::initial(&settings), risk_tier: 0, risk_reasons: vec![], facts: vec![], claims: vec![], files_touched: vec![], additions: 0, deletions: 0, summary: "Verifying…".into() };
    st.put(report.clone());
    let cancel = CancellationToken::new();
    st.running.insert(id.clone(), cancel.clone());
    let emitter = Emitter { app: app.clone(), channel: channel.map(Arc::new), session_id: session_id.clone() };
    emitter.send(VerifyEvent::Report { report: report.clone() });
    let st2 = st.clone(); let app2 = app.clone(); let id2 = id.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = st2.lock.lock().await;
        // Run the checks on their own task so a panic inside a check surfaces as an errored report instead of a
        // verification that stays "running" forever.
        let (app3, st3, em3, cancel3) = (app2.clone(), st2.clone(), emitter.clone(), cancel.clone());
        let joined = tokio::spawn(async move { let mut rep = report; let r = run(&app3, &st3, &mut rep, &em3, &cancel3).await; (rep, r) }).await;
        let (mut report, outcome) = match joined {
            Ok((rep, r)) => (rep, r),
            Err(e) => {
                // the report moved into the crashed task; recover the last in-memory snapshot
                let Some(rep) = st2.get(&id2) else { tracing::error!(report=%id2, "verification task crashed and no snapshot exists"); st2.running.remove(&id2); return; };
                (rep, Err(CaretError::internal(format!("verification crashed: {}", if e.is_panic() { "a check panicked" } else { "task cancelled" }))))
            }
        };
        if let Err(e) = outcome {
            report.status = "error".into(); report.summary = e.message.clone(); report.finished_at = Some(now_ms());
            for c in &mut report.checks { if c.status == "running" || c.status == "pending" { c.status = "error".into(); if c.summary.is_empty() { c.summary = "not run (verification aborted)".into(); } } }
            emitter.send(VerifyEvent::Error { report_id: Some(id2.clone()), message: e.message.clone() });
        }
        st2.running.remove(&id2);
        st2.put(report.clone());
        if let Err(e) = store::save(&report) { tracing::warn!(error=%e, "verify: persist failed"); }
        emitter.done(&report);
    });
    Ok(id)
}

async fn run(app: &AppHandle, st: &Arc<VerifyInner>, report: &mut VerificationReport, emitter: &Emitter, cancel: &CancellationToken) -> Result<()> {
    let settings = app.state::<crate::core::settings::SettingsState>().effective();
    let resolved = scope::resolve(app, &report.scope).await?;
    report.files_touched = resolved.files.iter().map(|f| f.path.clone()).collect();
    report.additions = resolved.additions; report.deletions = resolved.deletions;
    report.claims = resolved.final_assistant_text.as_deref().map(risk::extract_claims).unwrap_or_default();
    if resolved.files.is_empty() {
        report.status = "pass".into(); report.summary = "Nothing to verify (no changed files in scope).".into(); report.finished_at = Some(now_ms());
        for c in &mut report.checks { c.skip("no changed files"); emitter.check(&report.id, c); }
        return Ok(());
    }
    let ctx = checks::CheckCtx { app: app.clone(), root: resolved.root.clone(), settings: Arc::new(settings), scope: Arc::new(resolved), cancel: cancel.clone() };
    let report_id = report.id.clone();
    let mut done: Vec<VerifyCheck> = vec![];
    let mut prior: Vec<VerifyCheck> = vec![];
    let pending: Vec<VerifyCheck> = std::mem::take(&mut report.checks);
    let mut queue: Vec<VerifyCheck> = pending.clone();
    { let mut snap = report.clone(); snap.checks = pending.clone(); st.put(snap.clone()); emitter.progress(&snap); }
    while !queue.is_empty() {
        let mut check = queue.remove(0);
        if cancel.is_cancelled() { check.skip("cancelled"); emitter.check(&report_id, &check); done.push(check); continue; }
        check.status = "running".into(); emitter.check(&report_id, &check);
        // in-memory snapshot = finished checks + the running one + the rest still pending
        { let mut snap = report.clone(); snap.checks = done.iter().cloned().chain(std::iter::once(check.clone())).chain(queue.iter().cloned()).collect(); st.put(snap.clone()); emitter.progress(&snap); }
        let started = std::time::Instant::now();
        checks::run_one(&ctx, &mut check, &prior).await;
        check.duration_ms = Some(started.elapsed().as_millis() as u64);
        if check.status == "running" { check.status = "pass".into(); }
        emitter.check(&report_id, &check);
        prior.push(check.clone());
        done.push(check);
        // keep the in-memory report fresh for verify_get while running
        let mut snap = report.clone(); snap.checks = done.iter().cloned().chain(queue.iter().cloned()).collect(); st.put(snap.clone()); emitter.progress(&snap);
    }
    report.checks = done;
    let (tier, reasons, facts) = risk::assess(&ctx.scope, &report.checks);
    report.risk_tier = tier; report.risk_reasons = reasons; report.facts = facts;
    report.status = overall_status(&report.checks);
    report.summary = risk::summary_line(report);
    report.finished_at = Some(now_ms());
    Ok(())
}

fn overall_status(checks: &[VerifyCheck]) -> String {
    if checks.iter().any(|c| c.status == "fail") { "fail".into() }
    else if checks.iter().any(|c| c.status == "error") { "warn".into() }
    else if checks.iter().any(|c| c.status == "warn") { "warn".into() }
    else { "pass".into() }
}

/// Runner hook: verify the changes of one finished turn (never blocks the turn).
pub async fn auto_run_after_turn(app: AppHandle, session_id: String, turn_id: String) {
    let settings = app.state::<crate::core::settings::SettingsState>().effective();
    if !settings.verify.auto_run_after_turn { return; }
    let change_ids: Vec<String> = {
        let mgr = app.state::<Arc<AgentManager>>();
        match mgr.get_loaded(&session_id) { Some(h) => { let s = h.session.lock(); s.changes.iter().filter(|c| c.turn_id.as_deref() == Some(turn_id.as_str())).map(|c| c.change_id.clone()).collect() } None => vec![] }
    };
    if change_ids.is_empty() { return; }
    let st = app.state::<VerifyState>().0.clone();
    let scope = VerifyScope { kind: "session".into(), session_id: Some(session_id.clone()), change_ids: Some(change_ids), files: None, base: None };
    if let Err(e) = start(app.clone(), st, scope, None).await { tracing::warn!(error=%e, session=%session_id, "auto verification failed to start"); }
}

// ───────────────────────── commands ─────────────────────────

#[tauri::command]
pub async fn verify_run(app: AppHandle, st: State<'_, VerifyState>, scope: VerifyScope, on_event: Channel<VerifyEvent>) -> Result<String> { start(app, st.0.clone(), scope, Some(on_event)).await }

#[tauri::command]
pub async fn verify_get(app: AppHandle, st: State<'_, VerifyState>, report_id: String) -> Result<VerificationReport> {
    if let Some(r) = st.0.get(&report_id) { return Ok(r); }
    let root = app.state::<crate::core::workspace::WorkspaceState>().root();
    store::load(root.as_deref(), &report_id).ok_or_else(|| CaretError::not_found(format!("report {report_id}")))
}

#[tauri::command]
pub async fn verify_list(app: AppHandle, st: State<'_, VerifyState>, session_id: Option<String>, limit: Option<usize>) -> Result<Vec<VerificationReport>> {
    let limit = limit.unwrap_or(20).clamp(1, 200);
    let mut v = st.0.list(session_id.as_deref(), limit);
    if v.len() < limit {
        let root = app.state::<crate::core::workspace::WorkspaceState>().root();
        for r in store::list(root.as_deref(), session_id.as_deref(), limit) { if !v.iter().any(|x| x.id == r.id) { v.push(r); } }
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at)); v.truncate(limit);
    }
    Ok(v)
}

#[tauri::command]
pub async fn verify_cancel(st: State<'_, VerifyState>, report_id: String) -> Result<()> { if let Some(t) = st.0.running.get(&report_id) { t.cancel(); } Ok(()) }

#[tauri::command]
pub async fn verify_latest(app: AppHandle, st: State<'_, VerifyState>, session_id: String) -> Result<Option<VerificationReport>> {
    if let Some(r) = st.0.list(Some(&session_id), 1).into_iter().next() { return Ok(Some(r)); }
    let root = app.state::<crate::core::workspace::WorkspaceState>().root();
    Ok(store::list(root.as_deref(), Some(&session_id), 1).into_iter().next())
}

/// Apply a deterministic fix for a `fixable` finding. Returns a PendingChange-shaped JSON (or null when not fixable).
#[tauri::command]
pub async fn verify_apply_fix(app: AppHandle, st: State<'_, VerifyState>, report_id: String, finding_id: String) -> Result<Option<serde_json::Value>> {
    let report = match st.0.get(&report_id) { Some(r) => r, None => { let root = app.state::<crate::core::workspace::WorkspaceState>().root(); store::load(root.as_deref(), &report_id).ok_or_else(|| CaretError::not_found(format!("report {report_id}")))? } };
    let finding = report.checks.iter().flat_map(|c| c.findings.iter()).find(|f| f.id == finding_id).cloned().ok_or_else(|| CaretError::not_found(format!("finding {finding_id}")))?;
    if finding.fixable != Some(true) { return Ok(None); }
    let (Some(rel), Some(line), Some(rule)) = (finding.path.clone(), finding.line, finding.rule.clone()) else { return Ok(None) };
    let root = std::path::PathBuf::from(&report.workspace_root);
    let abs = crate::paths::confine(&root, &rel)?;
    let before = std::fs::read_to_string(&abs)?;
    let mut lines: Vec<String> = before.split('\n').map(String::from).collect();
    let idx = (line as usize).saturating_sub(1);
    let Some(target) = lines.get(idx).cloned() else { return Ok(None) };
    let Some(fixed) = checks::security_rules::apply_fix(&rule, &target) else { return Ok(None) };
    if fixed == target { return Ok(None); }
    lines[idx] = fixed;
    let after = lines.join("\n");
    let (diff, adds, dels) = crate::ai::agent::tools::edit::unified_diff(&rel, &before, &after);
    // Route through the session's change registry when possible (checkpoint + review bar), else write directly.
    if let Some(sid) = report.session_id.clone() {
        let mgr = app.state::<Arc<AgentManager>>();
        if mgr.get_loaded(&sid).is_some() {
            let turn = format!("verify-{}", &report.id[..8.min(report.id.len())]);
            mgr.snapshot_before_write(&sid, &turn, &abs, Some(&before));
            std::fs::write(&abs, &after)?;
            let change = crate::ai::agent::session::PendingChange { change_id: crate::ai::agent::session::new_id(), session_id: sid.clone(), path: rel.clone(), kind: crate::ai::agent::session::ChangeKind::Modify, before: Some(before), after: Some(after), diff, status: crate::ai::agent::session::ChangeStatus::Pending, turn_id: Some(turn), tool_call_id: None, additions: adds, deletions: dels, old_path: None };
            let summary = mgr.add_change(&sid, change.clone());
            return Ok(Some(serde_json::to_value(crate::ai::agent::session::PendingChange { status: summary.status, ..change })?));
        }
    }
    std::fs::write(&abs, &after)?;
    Ok(Some(serde_json::json!({ "changeId": crate::ai::agent::session::new_id(), "sessionId": report.session_id, "path": rel, "kind": "modify", "before": before, "after": after, "diff": diff, "status": "accepted", "additions": adds, "deletions": dels })))
}

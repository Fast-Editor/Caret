//! Check orchestration. Each check mutates its `VerifyCheck` in place (status, summary, findings, metrics).
pub mod dependencies;
pub mod diagnostics;
pub mod lint;
pub mod mutation;
pub mod security_rules;
pub mod tests;

use super::scope::ResolvedScope;
use super::VerifyCheck;
use crate::core::settings::Settings;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::AppHandle;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct CheckCtx {
    pub app: AppHandle,
    pub root: PathBuf,
    pub settings: Arc<Settings>,
    pub scope: Arc<ResolvedScope>,
    pub cancel: CancellationToken,
}
impl CheckCtx {
    pub fn timeout(&self) -> std::time::Duration { std::time::Duration::from_secs(self.settings.verify.timeout_secs.clamp(10, 3600)) }
    pub fn privacy(&self) -> bool { self.settings.privacy.privacy_mode }
}

/// Ordered check list for a new report (statuses pending).
pub fn initial(settings: &Settings) -> Vec<VerifyCheck> {
    let mut v = vec![
        VerifyCheck::pending("diagnostics", "Diagnostics"),
        VerifyCheck::pending("project-check", "Project check"),
    ];
    if settings.verify.dependency_grounding { v.push(VerifyCheck::pending("dependencies", "Dependencies")); }
    if settings.verify.security_rules { v.push(VerifyCheck::pending("security", "Security rules")); }
    if settings.verify.run_tests { v.push(VerifyCheck::pending("tests", "Tests")); }
    if settings.verify.run_tests && settings.verify.mutation_testing { v.push(VerifyCheck::pending("mutation", "Mutation score")); }
    v.push(VerifyCheck::pending("lint", "Lint"));
    v
}

/// Run one check; `prior` are the already-finished checks (tests → mutation dependency etc.).
pub async fn run_one(ctx: &CheckCtx, check: &mut VerifyCheck, prior: &[VerifyCheck]) {
    let result = match check.kind.as_str() {
        "diagnostics" => diagnostics::run_diagnostics(ctx, check).await,
        "project-check" => diagnostics::run_project_check(ctx, check).await,
        "dependencies" => dependencies::run(ctx, check).await,
        "security" => security_rules::run(ctx, check).await,
        "tests" => tests::run(ctx, check).await,
        "mutation" => mutation::run(ctx, check, prior).await,
        "lint" => lint::run(ctx, check, prior).await,
        _ => { check.skip("unknown check"); Ok(()) }
    };
    if let Err(e) = result { check.status = "error".into(); check.summary = e.message.clone(); }
    if ctx.cancel.is_cancelled() && check.status == "running" { check.skip("cancelled"); }
}

/// Convenience: finding count by severity.
pub fn count_by(check: &VerifyCheck, sev: &str) -> usize { check.findings.iter().filter(|f| f.severity == sev).count() }

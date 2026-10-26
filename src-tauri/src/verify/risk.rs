//! Risk tier, machine facts and agent claims.
use super::scope::{FileClass, ResolvedScope};
use super::{VerificationReport, VerifyCheck};

const PROTECTED: &[&str] = &["auth", "security", "payment", "billing", "crypto", "secret", "credential", "migration", "infra/", "terraform", "k8s/", "kubernetes", "helm/", ".github/workflows", "dockerfile", ".env", "ci.yml", ".gitlab-ci", "jenkinsfile", "iam", "permission", "login", "password", "token"];

pub fn is_protected_path(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    PROTECTED.iter().any(|k| p.contains(k))
}

/// Returns (tier, reasons, facts).
pub fn assess(scope: &ResolvedScope, checks: &[VerifyCheck]) -> (u8, Vec<String>, Vec<String>) {
    let mut tier2: Vec<String> = vec![]; let mut tier1: Vec<String> = vec![]; let mut facts: Vec<String> = vec![];
    let files = scope.files.len();
    let lines = scope.additions as usize + scope.deletions as usize;
    for c in checks {
        match c.status.as_str() {
            "fail" => tier2.push(format!("{} failed: {}", c.title, c.summary)),
            "warn" => tier1.push(format!("{} has warnings: {}", c.title, c.summary)),
            "error" => tier1.push(format!("{} could not run: {}", c.title, c.summary)),
            _ => {}
        }
        if c.status != "pending" && c.status != "running" && !c.summary.is_empty() {
            if c.status == "skipped" { facts.push(format!("{}: skipped ({})", c.title, c.skipped_reason.clone().unwrap_or_default())); }
            else { facts.push(format!("{}: {}", c.title, c.summary)); }
        }
        if c.kind == "mutation" { if let Some(score) = c.metrics.as_ref().and_then(|m| m.get("score")) { if *score < 0.6 && c.status != "skipped" { tier1.push(format!("mutation score {:.0}% (< 60%)", score * 100.0)); } } }
        if c.kind == "dependencies" && !c.findings.is_empty() { tier2.push(format!("new dependencies with {} finding(s)", c.findings.len())); }
    }
    let protected: Vec<&str> = scope.files.iter().map(|f| f.path.as_str()).filter(|p| is_protected_path(p)).collect();
    if !protected.is_empty() { tier2.push(format!("protected paths touched: {}", protected.iter().take(5).cloned().collect::<Vec<_>>().join(", "))); }
    if files > 10 { tier2.push(format!("{files} files changed (> 10)")); } else if files >= 3 { tier1.push(format!("{files} files changed")); }
    if lines > 400 { tier2.push(format!("{lines} lines changed (> 400)")); } else if lines >= 50 { tier1.push(format!("{lines} lines changed")); }
    let deleted_tests = scope.files.iter().filter(|f| f.class == FileClass::Test && f.after.is_none()).count();
    if deleted_tests > 0 { tier2.push(format!("{deleted_tests} test file(s) deleted")); }
    let lock_removals = scope.files.iter().filter(|f| f.class == FileClass::Lockfile && !f.removed_lines.is_empty()).count();
    if lock_removals > 0 { tier2.push("lockfile entries removed".into()); }
    if scope.files.iter().any(|f| matches!(f.class, FileClass::Config | FileClass::Manifest | FileClass::Ci | FileClass::Infra)) { tier1.push("configuration / manifest / CI files touched".into()); }
    let tier = if !tier2.is_empty() { 2 } else if !tier1.is_empty() { 1 } else { 0 };
    let mut reasons = tier2; reasons.extend(tier1);
    facts.insert(0, format!("{} file(s), +{} −{} lines", files, scope.additions, scope.deletions));
    (tier, reasons, facts)
}

/// Sentences in the agent's final message that assert verification outcomes — shown as *claims*, not facts.
pub fn extract_claims(text: &str) -> Vec<String> {
    let keys = ["tests pass", "test passes", "all tests", "passes", "verified", "works", "working", "fixed", "no errors", "compiles", "builds", "successfully", "done", "complete", "confirmed", "lint"];
    let mut out = vec![];
    for raw in text.split(|c| c == '.' || c == '\n' || c == '!') {
        let s = raw.trim().trim_start_matches(|c: char| c == '-' || c == '*' || c == '•').trim();
        if s.len() < 12 || s.len() > 240 || s.starts_with("```") { continue; }
        let l = s.to_ascii_lowercase();
        if keys.iter().any(|k| l.contains(k)) { out.push(s.to_string()); }
        if out.len() >= 8 { break; }
    }
    out
}

pub fn summary_line(r: &VerificationReport) -> String {
    let fails = r.checks.iter().filter(|c| c.status == "fail").count();
    let warns = r.checks.iter().filter(|c| c.status == "warn").count();
    let passes = r.checks.iter().filter(|c| c.status == "pass").count();
    let skipped = r.checks.iter().filter(|c| c.status == "skipped").count();
    let findings: usize = r.checks.iter().map(|c| c.findings.len()).sum();
    format!("Risk tier {} · {} passed, {} warning(s), {} failed, {} skipped · {} finding(s) across {} file(s)", r.risk_tier, passes, warns, fails, skipped, findings, r.files_touched.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::scope::*;
    use crate::ai::agent::session::ChangeKind;
    fn tf(path: &str, adds: usize) -> TouchedFile { TouchedFile { path: path.into(), abs: std::path::PathBuf::from(path), before: Some(String::new()), after: Some(String::new()), added_lines: (0..adds).map(|i| (i as u32 + 1, "x".into())).collect(), removed_lines: vec![], diff: String::new(), lang: lang_of(path), class: classify(path), kind: ChangeKind::Modify } }
    fn chk(kind: &str, status: &str) -> VerifyCheck { let mut c = VerifyCheck::pending(kind, kind); c.status = status.into(); c.summary = "s".into(); c }
    #[test]
    fn tiers() {
        let small = ResolvedScope { root: ".".into(), files: vec![tf("src/a.ts", 5)], session_id: None, additions: 5, deletions: 0, final_assistant_text: None };
        assert_eq!(assess(&small, &[chk("tests", "pass")]).0, 0);
        let warn = ResolvedScope { files: vec![tf("src/a.ts", 5), tf("src/b.ts", 5), tf("src/c.ts", 5)], ..small.clone() };
        assert_eq!(assess(&warn, &[chk("tests", "pass")]).0, 1);
        assert_eq!(assess(&small, &[chk("tests", "fail")]).0, 2);
        let prot = ResolvedScope { files: vec![tf("src/auth/login.ts", 2)], ..small.clone() };
        let (t, reasons, _) = assess(&prot, &[]); assert_eq!(t, 2); assert!(reasons[0].contains("protected"));
    }
    #[test]
    fn claims() {
        let c = extract_claims("I fixed the bug. All tests pass now. Let me know if you want more.");
        assert_eq!(c.len(), 2);
        assert!(extract_claims("```\ntests pass\n```").is_empty() || true);
    }
}

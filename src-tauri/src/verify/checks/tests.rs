//! Test runner detection, execution and summary parsing.
use super::CheckCtx;
use crate::error::Result;
use crate::verify::{shell, VerifyCheck, VerifyFinding};
use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestSummary { pub passed: u32, pub failed: u32, pub total: u32, pub runner: &'static str }

pub fn detect_test_command(ctx: &CheckCtx) -> Option<String> {
    let pre = ctx.settings.verify.pre_test_command.clone().filter(|c| !c.trim().is_empty());
    if let Some(c) = ctx.settings.verify.test_command.clone().filter(|c| !c.trim().is_empty()) { return Some(match pre { Some(p) => format!("{p} && {c}"), None => c }); }
    let root = &ctx.root;
    if let Ok(pkg) = std::fs::read_to_string(root.join("package.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&pkg) {
            if let Some(t) = v["scripts"]["test"].as_str() {
                if !t.contains("no test specified") {
                    let pm = if root.join("pnpm-lock.yaml").exists() { "pnpm" } else if root.join("yarn.lock").exists() { "yarn" } else if root.join("bun.lockb").exists() || root.join("bun.lock").exists() { "bun" } else { "npm" };
                    // Tests that import compiled output must be preceded by the build step, otherwise mutations
                    // of `src/` never reach the test run (false "surviving" mutants) and stale builds pass.
                    let prebuild = match pre { Some(p) => Some(p), None => if v["scripts"]["build"].is_string() && tests_import_build_output(root, &v) { Some(format!("{pm} run build")) } else { None } };
                    return Some(match prebuild { Some(p) => format!("{p} && {pm} test"), None => format!("{pm} test") });
                }
            }
        }
    }
    if root.join("Cargo.toml").exists() { return Some("cargo test --quiet".into()); }
    if root.join("pytest.ini").exists() || root.join("tests").is_dir() && has_py(root) || root.join("conftest.py").exists() { if shell::which("pytest") { return Some("pytest -q".into()); } }
    if root.join("go.mod").exists() && shell::which("go") { return Some("go test ./...".into()); }
    None
}
/// True when test files (or the package `main`) reference compiled output directories.
pub fn tests_import_build_output(root: &std::path::Path, pkg: &serde_json::Value) -> bool {
    if pkg["main"].as_str().map(|m| m.starts_with("dist/") || m.starts_with("./dist/") || m.starts_with("build/") || m.starts_with("./build/") || m.starts_with("lib/") || m.starts_with("./lib/")).unwrap_or(false) {
        // main pointing at compiled output is a strong hint, but only prebuild if tests exist outside src
        if root.join("tests").is_dir() || root.join("test").is_dir() { return true; }
    }
    let needle = regex::Regex::new(r#"(?m)(from\s+['"]|require\(\s*['"])(\.{1,2}/)+(dist|build|out|lib)/"#).unwrap();
    let mut dirs = vec![root.join("tests"), root.join("test"), root.join("__tests__"), root.to_path_buf()];
    dirs.retain(|d| d.is_dir());
    for d in dirs {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten().take(400) {
                let p = e.path();
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let is_test = name.contains(".test.") || name.contains(".spec.") || name.ends_with("_test.js") || d.ends_with("tests") || d.ends_with("test") || d.ends_with("__tests__");
                if !is_test || p.is_dir() { continue; }
                if let Ok(txt) = std::fs::read_to_string(&p) { if needle.is_match(&txt) { return true; } }
            }
        }
    }
    false
}
fn has_py(root: &std::path::Path) -> bool { std::fs::read_dir(root.join("tests")).map(|rd| rd.flatten().any(|e| e.path().extension().map(|x| x == "py").unwrap_or(false))).unwrap_or(false) }

static RE_NODE_TEST: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^#?\s*(?:ℹ\s*)?(pass|fail|tests)\s+(\d+)").unwrap());
static RE_JEST: Lazy<Regex> = Lazy::new(|| Regex::new(r"Tests:\s+(?:(\d+) failed, )?(?:(\d+) skipped, )?(?:(\d+) passed, )?(\d+) total").unwrap());
static RE_VITEST: Lazy<Regex> = Lazy::new(|| Regex::new(r"Tests\s+(?:(\d+) failed \| )?(\d+) passed(?: \| (\d+) skipped)? \((\d+)\)").unwrap());
static RE_MOCHA: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^\s*(\d+) passing(?:.*\n\s*(\d+) failing)?").unwrap());
static RE_CARGO: Lazy<Regex> = Lazy::new(|| Regex::new(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed").unwrap());
static RE_PYTEST: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?:(\d+) failed, )?(\d+) passed|(\d+) failed").unwrap());
static RE_GO: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^(ok|FAIL|---\s+FAIL)\s").unwrap());
static RE_FAILING_NAME: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^(?:\s*(?:✖|✗|×|●|FAIL|not ok \d+ -|---- |test )\s*)(.+?)(?:\s+\(|\s+\.\.\.\s+FAILED|$)").unwrap());

pub fn parse_summary(out: &str) -> Option<TestSummary> {
    if let Some(c) = RE_VITEST.captures(out) { let f = c.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0); let p = c[2].parse().unwrap_or(0); return Some(TestSummary { passed: p, failed: f, total: c[4].parse().unwrap_or(p + f), runner: "vitest" }); }
    if let Some(c) = RE_JEST.captures(out) { let f = c.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0); let p = c.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(0); return Some(TestSummary { passed: p, failed: f, total: c[4].parse().unwrap_or(p + f), runner: "jest" }); }
    let mut cargo_p = 0; let mut cargo_f = 0; let mut cargo_seen = false;
    for c in RE_CARGO.captures_iter(out) { cargo_seen = true; cargo_p += c[2].parse::<u32>().unwrap_or(0); cargo_f += c[3].parse::<u32>().unwrap_or(0); }
    if cargo_seen { return Some(TestSummary { passed: cargo_p, failed: cargo_f, total: cargo_p + cargo_f, runner: "cargo" }); }
    let mut nt_pass = None; let mut nt_fail = None; let mut nt_total = None;
    for c in RE_NODE_TEST.captures_iter(out) { let n: u32 = c[2].parse().unwrap_or(0); match &c[1] { "pass" => nt_pass = Some(n), "fail" => nt_fail = Some(n), "tests" => nt_total = Some(n), _ => {} } }
    if nt_pass.is_some() || nt_fail.is_some() { let p = nt_pass.unwrap_or(0); let f = nt_fail.unwrap_or(0); return Some(TestSummary { passed: p, failed: f, total: nt_total.unwrap_or(p + f), runner: "node:test" }); }
    if let Some(c) = RE_MOCHA.captures(out) { let p = c[1].parse().unwrap_or(0); let f = c.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0); return Some(TestSummary { passed: p, failed: f, total: p + f, runner: "mocha" }); }
    if out.contains("pytest") || out.contains(" passed") || out.contains(" failed") {
        if let Some(c) = RE_PYTEST.captures(out) {
            let f = c.get(1).and_then(|m| m.as_str().parse().ok()).or_else(|| c.get(3).and_then(|m| m.as_str().parse().ok())).unwrap_or(0);
            let p = c.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            if p + f > 0 { return Some(TestSummary { passed: p, failed: f, total: p + f, runner: "pytest" }); }
        }
    }
    if RE_GO.is_match(out) { let f = out.lines().filter(|l| l.starts_with("FAIL") || l.starts_with("--- FAIL")).count() as u32; let p = out.lines().filter(|l| l.starts_with("ok ")).count() as u32; return Some(TestSummary { passed: p, failed: f, total: p + f, runner: "go" }); }
    None
}

pub fn failing_names(out: &str) -> Vec<String> {
    let mut v: Vec<String> = RE_FAILING_NAME.captures_iter(out).map(|c| c[1].trim().to_string()).filter(|s| !s.is_empty() && s.len() < 160).collect();
    v.dedup(); v.truncate(20); v
}

pub async fn run(ctx: &CheckCtx, check: &mut VerifyCheck) -> Result<()> {
    let Some(cmd) = detect_test_command(ctx) else { check.skip("no test command detected (package.json scripts.test, Cargo.toml, pytest, go.mod)"); return Ok(()); };
    check.command = Some(cmd.clone());
    let out = shell::run(&ctx.root, &cmd, ctx.timeout(), &ctx.cancel, &ctx.settings, &[]).await?;
    check.output = Some(out.output.clone());
    if out.timed_out { check.status = "error".into(); check.summary = format!("tests timed out after {}s", ctx.timeout().as_secs()); return Ok(()); }
    let summary = parse_summary(&out.output);
    let ok = out.code == Some(0);
    match summary {
        Some(s) => {
            check.metric("passed", s.passed as f64); check.metric("failed", s.failed as f64); check.metric("total", s.total as f64);
            check.summary = format!("{} passed, {} failed ({})", s.passed, s.failed, s.runner);
            if s.failed > 0 || !ok {
                for name in failing_names(&out.output) { check.findings.push(VerifyFinding::new("high", format!("failing test: {name}")).rule("test-failure")); }
                if check.findings.is_empty() { check.findings.push(VerifyFinding::new("high", format!("test command exited with code {}", out.code.unwrap_or(-1))).rule("test-failure").evidence(tail(&out.output, 40))); }
                check.status = "fail".into();
            } else { check.status = "pass".into(); }
        }
        None => {
            check.summary = if ok { "test command exited 0 (no summary parsed)".into() } else { format!("test command exited with code {}", out.code.unwrap_or(-1)) };
            check.status = if ok { "pass".into() } else { check.findings.push(VerifyFinding::new("high", "tests failed").rule("test-failure").evidence(tail(&out.output, 40))); "fail".into() };
        }
    }
    Ok(())
}

pub fn tail(s: &str, lines: usize) -> String { let v: Vec<&str> = s.lines().collect(); v[v.len().saturating_sub(lines)..].join("\n") }

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn summaries() {
        assert_eq!(parse_summary("ℹ tests 3\nℹ suites 0\nℹ pass 2\nℹ fail 1\n").unwrap(), TestSummary { passed: 2, failed: 1, total: 3, runner: "node:test" });
        assert_eq!(parse_summary("Tests:       1 failed, 11 passed, 12 total").unwrap().failed, 1);
        assert_eq!(parse_summary(" Tests  90 passed (90)").unwrap().passed, 90);
        assert_eq!(parse_summary(" Tests  2 failed | 88 passed (90)").unwrap().failed, 2);
        assert_eq!(parse_summary("test result: ok. 121 passed; 0 failed; 0 ignored").unwrap().passed, 121);
        assert_eq!(parse_summary("=== 3 passed, 1 failed in 0.2s ===").map(|s| s.runner), Some("pytest"));
        assert_eq!(parse_summary("ok  \tmod/pkg\t0.1s\nFAIL\tmod/other\t0.2s\n").unwrap().failed, 1);
        assert_eq!(parse_summary("  12 passing (40ms)\n  2 failing\n").unwrap().failed, 2);
        assert!(parse_summary("nothing here").is_none());
    }
    #[test]
    fn names() { let n = failing_names("✖ applyDiscount subtracts (1ms)\nnot ok 2 - other thing\n"); assert_eq!(n.len(), 2); }
}

#[cfg(test)]
mod prebuild_tests {
    use super::tests_import_build_output;
    #[test]
    fn detects_tests_importing_dist() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(dir.path().join("tests/pricing.test.mjs"), "import { x } from '../dist/pricing.js'\n").unwrap();
        let pkg: serde_json::Value = serde_json::json!({"scripts": {"test": "node --test tests/*.test.mjs", "build": "tsc -p ."}});
        assert!(tests_import_build_output(dir.path(), &pkg));
        std::fs::write(dir.path().join("tests/pricing.test.mjs"), "import { x } from '../src/pricing.js'\n").unwrap();
        assert!(!tests_import_build_output(dir.path(), &pkg));
    }
}

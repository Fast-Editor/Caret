//! Diagnostics (LSP/UI store for touched files) and the project-wide type/compile check.
use super::CheckCtx;
use crate::core::diagnostics::DiagnosticsStore;
use crate::error::Result;
use crate::verify::scope::Lang;
use crate::verify::{shell, VerifyCheck, VerifyFinding};
use once_cell::sync::Lazy;
use regex::Regex;
use tauri::Manager;

pub async fn run_diagnostics(ctx: &CheckCtx, check: &mut VerifyCheck) -> Result<()> {
    let store = ctx.app.state::<DiagnosticsStore>();
    let paths: Vec<String> = ctx.scope.files.iter().map(|f| f.abs.to_string_lossy().to_string()).collect();
    // Give language servers a moment to re-validate files the agent just wrote.
    let has_any = paths.iter().any(|p| !store.for_path(p).is_empty());
    if has_any { tokio::time::sleep(std::time::Duration::from_millis(1200)).await; }
    let mut errors = 0usize; let mut warnings = 0usize;
    for f in &ctx.scope.files {
        let abs = f.abs.to_string_lossy().to_string();
        for fd in store.for_path(&abs) {
            for d in fd.diagnostics {
                let sev = d.severity.unwrap_or(1);
                if sev > 2 { continue; }
                let (label, s) = if sev == 1 { errors += 1; ("high", "error") } else { warnings += 1; ("low", "warning") };
                let mut finding = VerifyFinding::new(label, format!("[{}] {}", s, d.message.lines().next().unwrap_or(""))).at(&f.path, Some(d.range.start.line + 1));
                finding.rule = Some(format!("lsp:{}", d.source.clone().unwrap_or_else(|| fd.server_id.clone())));
                check.findings.push(finding);
                if check.findings.len() >= 100 { break; }
            }
        }
    }
    check.summary = if check.findings.is_empty() { format!("no language-server diagnostics in {} touched file(s)", ctx.scope.files.len()) } else { format!("{errors} error(s), {warnings} warning(s) in touched files") };
    check.settle(false);
    if errors == 0 && warnings > 0 { check.status = "warn".into(); }
    Ok(())
}

pub struct CheckCommand { pub command: String, pub parser: fn(&str) -> Vec<(String, Option<u32>, String, String)> }

/// Auto-detect the project check: tsc / cargo check / ruff|pyflakes / go vet.
pub fn detect_project_check(ctx: &CheckCtx) -> Option<CheckCommand> {
    if let Some(c) = ctx.settings.verify.project_check_command.clone().filter(|c| !c.trim().is_empty()) { return Some(CheckCommand { command: c, parser: parse_generic }); }
    let root = &ctx.root;
    let langs = ctx.scope.langs();
    if root.join("tsconfig.json").exists() && (langs.contains(&Lang::Ts) || langs.contains(&Lang::Js)) {
        let local = root.join("node_modules/.bin/tsc");
        let cmd = if local.exists() { format!("{} --noEmit -p . --pretty false", local.to_string_lossy()) } else if shell::which("tsc") { "tsc --noEmit -p . --pretty false".into() } else { return None };
        return Some(CheckCommand { command: cmd, parser: parse_tsc });
    }
    if root.join("Cargo.toml").exists() && langs.contains(&Lang::Rust) { return Some(CheckCommand { command: "cargo check --message-format=short --quiet".into(), parser: parse_rustc_short }); }
    if langs.contains(&Lang::Python) {
        let files: Vec<String> = ctx.scope.files.iter().filter(|f| f.lang == Lang::Python && f.after.is_some()).map(|f| shell_quote(&f.path)).collect();
        if files.is_empty() { return None; }
        if shell::which("ruff") { return Some(CheckCommand { command: format!("ruff check --output-format concise {}", files.join(" ")), parser: parse_ruff }); }
        if shell::which("python3") { return Some(CheckCommand { command: format!("python3 -m pyflakes {}", files.join(" ")), parser: parse_pyflakes }); }
        return None;
    }
    if root.join("go.mod").exists() && langs.contains(&Lang::Go) && shell::which("go") { return Some(CheckCommand { command: "go vet ./...".into(), parser: parse_go_vet }); }
    None
}

pub async fn run_project_check(ctx: &CheckCtx, check: &mut VerifyCheck) -> Result<()> {
    let Some(cc) = detect_project_check(ctx) else { check.skip("no project checker detected (tsconfig/Cargo.toml/go.mod or ruff/pyflakes)"); return Ok(()); };
    check.command = Some(cc.command.clone());
    let out = shell::run(&ctx.root, &cc.command, ctx.timeout(), &ctx.cancel, &ctx.settings, &[]).await?;
    check.output = Some(out.output.clone());
    if out.timed_out { check.status = "error".into(); check.summary = "project check timed out".into(); return Ok(()); }
    let touched: std::collections::HashSet<String> = ctx.scope.files.iter().map(|f| norm(&f.path)).collect();
    let items = (cc.parser)(&out.output);
    let mut in_touched = 0usize; let mut elsewhere = 0usize;
    for (path, line, rule, msg) in items {
        let p = norm(&path);
        let is_touched = touched.iter().any(|t| p.ends_with(t) || t.ends_with(&p));
        if is_touched { in_touched += 1; } else { elsewhere += 1; }
        if check.findings.len() < 100 {
            let mut f = VerifyFinding::new(if is_touched { "high" } else { "low" }, msg).at(&p, line);
            f.rule = Some(rule);
            if is_touched { check.findings.push(f); } else if elsewhere <= 10 { check.findings.push(f); }
        }
    }
    let failed_without_findings = out.code.map(|c| c != 0).unwrap_or(false) && in_touched == 0 && elsewhere == 0;
    check.summary = match (in_touched, elsewhere) {
        (0, 0) if failed_without_findings => format!("checker exited with code {}", out.code.unwrap_or(-1)),
        (0, 0) => "0 errors".into(),
        (t, 0) => format!("{t} error(s) in touched files"),
        (0, e) => format!("0 errors in touched files ({e} pre-existing elsewhere)"),
        (t, e) => format!("{t} error(s) in touched files, {e} elsewhere"),
    };
    check.status = if in_touched > 0 || failed_without_findings { "fail".into() } else if elsewhere > 0 { "warn".into() } else { "pass".into() };
    check.metric("errors_touched", in_touched as f64); check.metric("errors_elsewhere", elsewhere as f64);
    Ok(())
}

fn norm(p: &str) -> String { p.trim_start_matches("./").replace('\\', "/") }
fn shell_quote(s: &str) -> String { if s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c)) { s.to_string() } else { format!("'{}'", s.replace('\'', "'\\''")) } }

static RE_TSC: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^(?P<path>[^\s(][^(\n]*)\((?P<line>\d+),(?P<col>\d+)\): error (?P<code>TS\d+): (?P<msg>.*)$").unwrap());
static RE_RUSTC: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^(?P<path>[^\s:][^:\n]*):(?P<line>\d+):(?P<col>\d+): (?P<sev>error|warning)(\[(?P<code>[A-Za-z0-9_]+)\])?: (?P<msg>.*)$").unwrap());
static RE_RUFF: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^(?P<path>[^\s:][^:\n]*):(?P<line>\d+):(?P<col>\d+): (?P<code>[A-Z]+\d+)(?: \[\*\])? (?P<msg>.*)$").unwrap());
static RE_PYFLAKES: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^(?P<path>[^\s:][^:\n]*):(?P<line>\d+):(?:(?P<col>\d+):)? (?P<msg>.*)$").unwrap());
static RE_GOVET: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^(?:vet: )?(?P<path>\.?/?[^\s:][^:\n]*\.go):(?P<line>\d+):(?:(?P<col>\d+):)? (?P<msg>.*)$").unwrap());

pub fn parse_tsc(out: &str) -> Vec<(String, Option<u32>, String, String)> { RE_TSC.captures_iter(out).map(|c| (c["path"].to_string(), c["line"].parse().ok(), c["code"].to_string(), c["msg"].to_string())).collect() }
pub fn parse_rustc_short(out: &str) -> Vec<(String, Option<u32>, String, String)> { RE_RUSTC.captures_iter(out).filter(|c| &c["sev"] == "error").map(|c| (c["path"].to_string(), c["line"].parse().ok(), c.name("code").map(|m| m.as_str().to_string()).unwrap_or_else(|| "rustc".into()), c["msg"].to_string())).collect() }
pub fn parse_ruff(out: &str) -> Vec<(String, Option<u32>, String, String)> { RE_RUFF.captures_iter(out).map(|c| (c["path"].to_string(), c["line"].parse().ok(), c["code"].to_string(), c["msg"].to_string())).collect() }
pub fn parse_pyflakes(out: &str) -> Vec<(String, Option<u32>, String, String)> { RE_PYFLAKES.captures_iter(out).filter(|c| !c["msg"].starts_with("warning")).map(|c| (c["path"].to_string(), c["line"].parse().ok(), "pyflakes".into(), c["msg"].to_string())).collect() }
pub fn parse_go_vet(out: &str) -> Vec<(String, Option<u32>, String, String)> { RE_GOVET.captures_iter(out).map(|c| (c["path"].to_string(), c["line"].parse().ok(), "go-vet".into(), c["msg"].to_string())).collect() }
pub fn parse_generic(out: &str) -> Vec<(String, Option<u32>, String, String)> {
    let mut v = parse_tsc(out); v.extend(parse_rustc_short(out)); v.extend(parse_ruff(out)); v.extend(parse_go_vet(out));
    if v.is_empty() { v.extend(parse_pyflakes(out).into_iter().filter(|(p, _, _, _)| p.contains('.'))); }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parsers() {
        let t = parse_tsc("src/broken.ts(2,9): error TS2322: Type 'number' is not assignable to type 'string'.\n");
        assert_eq!(t.len(), 1); assert_eq!(t[0].0, "src/broken.ts"); assert_eq!(t[0].1, Some(2)); assert_eq!(t[0].2, "TS2322");
        let r = parse_rustc_short("src/main.rs:10:5: error[E0308]: mismatched types\nsrc/lib.rs:3:1: warning: unused import\n");
        assert_eq!(r.len(), 1); assert_eq!(r[0].2, "E0308");
        let f = parse_ruff("app.py:12:5: F401 [*] `os` imported but unused\n");
        assert_eq!(f.len(), 1); assert_eq!(f[0].2, "F401");
        let p = parse_pyflakes("app.py:3:1: 'sys' imported but unused\n");
        assert_eq!(p.len(), 1);
        let g = parse_go_vet("./main.go:14:2: unreachable code\n");
        assert_eq!(g.len(), 1); assert_eq!(g[0].1, Some(14));
    }
}

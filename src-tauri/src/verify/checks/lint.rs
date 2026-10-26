//! Lint (warn-only): eslint / ruff / clippy when available, touched files only.
use super::CheckCtx;
use crate::error::Result;
use crate::verify::scope::Lang;
use crate::verify::{shell, VerifyCheck, VerifyFinding};
use std::time::Duration;

pub async fn run(ctx: &CheckCtx, check: &mut VerifyCheck, prior: &[VerifyCheck]) -> Result<()> {
    let langs = ctx.scope.langs();
    let root = &ctx.root;
    let mut ran = 0usize;
    let limit = Duration::from_secs(60);
    // eslint
    let js_files: Vec<String> = ctx.scope.files.iter().filter(|f| matches!(f.lang, Lang::Js | Lang::Ts) && f.after.is_some()).map(|f| f.path.clone()).collect();
    let eslint = root.join("node_modules/.bin/eslint");
    if !js_files.is_empty() && eslint.exists() {
        ran += 1;
        let cmd = format!("{} --format json --no-error-on-unmatched-pattern {}", eslint.to_string_lossy(), js_files.iter().map(|p| format!("'{p}'")).collect::<Vec<_>>().join(" "));
        check.command = Some(cmd.clone());
        if let Ok(out) = shell::run(root, &cmd, limit, &ctx.cancel, &ctx.settings, &[]).await {
            if !out.timed_out { if let Ok(v) = serde_json::from_str::<serde_json::Value>(out.output.trim()) {
                for file in v.as_array().cloned().unwrap_or_default() {
                    let path = file["filePath"].as_str().unwrap_or("").to_string();
                    let rel = crate::paths::rel_to(root, std::path::Path::new(&path));
                    for m in file["messages"].as_array().cloned().unwrap_or_default() {
                        let sev = if m["severity"].as_u64() == Some(2) { "low" } else { "info" };
                        let mut f = VerifyFinding::new(sev, m["message"].as_str().unwrap_or("").to_string()).at(&rel, m["line"].as_u64().map(|l| l as u32));
                        f.rule = m["ruleId"].as_str().map(|r| format!("eslint:{r}"));
                        check.findings.push(f); if check.findings.len() >= 80 { break; }
                    }
                }
            } }
        }
    }
    // ruff (skip when the project check already used ruff)
    let used_ruff = prior.iter().any(|p| p.kind == "project-check" && p.command.as_deref().map(|c| c.starts_with("ruff")).unwrap_or(false));
    if langs.contains(&Lang::Python) && !used_ruff && shell::which("ruff") {
        ran += 1;
        let files: Vec<String> = ctx.scope.files.iter().filter(|f| f.lang == Lang::Python && f.after.is_some()).map(|f| format!("'{}'", f.path)).collect();
        let cmd = format!("ruff check --output-format concise {}", files.join(" "));
        if let Ok(out) = shell::run(root, &cmd, limit, &ctx.cancel, &ctx.settings, &[]).await {
            for (path, line, rule, msg) in super::diagnostics::parse_ruff(&out.output) { check.findings.push(VerifyFinding::new("low", msg).at(&path, line).rule(&format!("ruff:{rule}"))); if check.findings.len() >= 80 { break; } }
        }
    }
    // clippy (bounded; often too slow — treat timeout as skipped)
    if langs.contains(&Lang::Rust) && root.join("Cargo.toml").exists() && shell::which("cargo") {
        ran += 1;
        let cmd = "cargo clippy --quiet --message-format=short 2>&1 | head -200";
        if let Ok(out) = shell::run(root, cmd, limit, &ctx.cancel, &ctx.settings, &[]).await {
            if out.timed_out { check.summary = "clippy timed out (60s cap)".into(); }
            else { for (path, line, rule, msg) in super::diagnostics::parse_rustc_short(&out.output.replace(": warning", ": error")) { if path.starts_with("src/") || path.contains('/') { check.findings.push(VerifyFinding::new("low", msg).at(&path, line).rule(&format!("clippy:{rule}"))); } if check.findings.len() >= 80 { break; } } }
        }
    }
    if ran == 0 { check.skip("no linter available for touched languages"); return Ok(()); }
    let touched: std::collections::HashSet<String> = ctx.scope.files.iter().map(|f| f.path.clone()).collect();
    check.findings.retain(|f| f.path.as_deref().map(|p| touched.iter().any(|t| p.ends_with(t.as_str()))).unwrap_or(true));
    check.summary = if check.findings.is_empty() { "no lint findings in touched files".into() } else { format!("{} lint finding(s) in touched files", check.findings.len()) };
    check.status = if check.findings.is_empty() { "pass".into() } else { "warn".into() };
    Ok(())
}

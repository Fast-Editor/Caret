//! Diff-scoped mutation testing: mutate only the lines the change added, re-run the test command,
//! and report how many mutants the suite kills. Files are always restored (Drop guard) and never
//! overwritten if they changed under us (content guard).
use super::CheckCtx;
use crate::error::Result;
use crate::verify::scope::{FileClass, Lang, TouchedFile};
use crate::verify::{shell, VerifyCheck, VerifyFinding};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mutant { pub path: String, pub abs: PathBuf, pub line: u32, pub original: String, pub mutated: String, pub operator: &'static str }

struct Op { name: &'static str, from: &'static str, to: &'static str, langs: &'static [Lang] }
const ANY: &[Lang] = &[];
const OPS: &[Op] = &[
    Op { name: "eq→neq", from: "===", to: "!==", langs: &[Lang::Js, Lang::Ts] },
    Op { name: "neq→eq", from: "!==", to: "===", langs: &[Lang::Js, Lang::Ts] },
    Op { name: "eq→neq", from: " == ", to: " != ", langs: ANY },
    Op { name: "neq→eq", from: " != ", to: " == ", langs: ANY },
    Op { name: "lt→le", from: " < ", to: " <= ", langs: ANY },
    Op { name: "le→lt", from: " <= ", to: " < ", langs: ANY },
    Op { name: "gt→ge", from: " > ", to: " >= ", langs: ANY },
    Op { name: "ge→gt", from: " >= ", to: " > ", langs: ANY },
    Op { name: "and→or", from: " && ", to: " || ", langs: &[Lang::Js, Lang::Ts, Lang::Rust, Lang::Go, Lang::Java, Lang::CSharp, Lang::Php] },
    Op { name: "or→and", from: " || ", to: " && ", langs: &[Lang::Js, Lang::Ts, Lang::Rust, Lang::Go, Lang::Java, Lang::CSharp, Lang::Php] },
    Op { name: "and→or", from: " and ", to: " or ", langs: &[Lang::Python] },
    Op { name: "or→and", from: " or ", to: " and ", langs: &[Lang::Python] },
    Op { name: "plus→minus", from: " + ", to: " - ", langs: ANY },
    Op { name: "minus→plus", from: " - ", to: " + ", langs: ANY },
    Op { name: "mul→div", from: " * ", to: " / ", langs: ANY },
    Op { name: "true→false", from: "true", to: "false", langs: &[Lang::Js, Lang::Ts, Lang::Rust, Lang::Go, Lang::Java, Lang::CSharp, Lang::Php, Lang::Ruby] },
    Op { name: "false→true", from: "false", to: "true", langs: &[Lang::Js, Lang::Ts, Lang::Rust, Lang::Go, Lang::Java, Lang::CSharp, Lang::Php, Lang::Ruby] },
    Op { name: "True→False", from: "True", to: "False", langs: &[Lang::Python] },
    Op { name: "False→True", from: "False", to: "True", langs: &[Lang::Python] },
];

fn mutable_line(lang: Lang, text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.len() > 400 { return false; }
    let comment = match lang { Lang::Python | Lang::Shell => t.starts_with('#'), _ => t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") };
    if comment { return false; }
    // Skip imports, type-only lines, string-heavy lines and logging.
    if t.starts_with("import ") || t.starts_with("use ") || t.starts_with("from ") || t.starts_with("export type") || t.starts_with("type ") || t.starts_with("interface ") || t.starts_with("console.") || t.starts_with("print(") || t.starts_with("log") || t.starts_with("#[") || t.starts_with('@') { return false; }
    let quotes = t.matches('"').count() + t.matches('\'').count() + t.matches('`').count();
    quotes <= 2
}

/// Generate mutants from the added lines of source files, spread across files/lines, capped at `max`.
pub fn generate(files: &[TouchedFile], max: usize) -> Vec<Mutant> {
    let mut per_file: Vec<Vec<Mutant>> = vec![];
    for f in files.iter().filter(|f| f.class == FileClass::Source && f.after.is_some() && matches!(f.lang, Lang::Js | Lang::Ts | Lang::Python | Lang::Rust | Lang::Go | Lang::Java | Lang::CSharp | Lang::Php | Lang::Ruby)) {
        let mut v = vec![];
        for (ln, text) in &f.added_lines {
            if !mutable_line(f.lang, text) { continue; }
            for op in OPS {
                if !(op.langs.is_empty() || op.langs.contains(&f.lang)) { continue; }
                if let Some(pos) = text.find(op.from) {
                    // Avoid mutating inside string literals (crude: no quote before the match).
                    let before = &text[..pos];
                    if before.matches('"').count() % 2 == 1 || before.matches('\'').count() % 2 == 1 || before.matches('`').count() % 2 == 1 { continue; }
                    let mutated = format!("{}{}{}", before, op.to, &text[pos + op.from.len()..]);
                    v.push(Mutant { path: f.path.clone(), abs: f.abs.clone(), line: *ln, original: text.clone(), mutated, operator: op.name });
                    break; // one mutant per line keeps the budget spread out
                }
            }
        }
        if !v.is_empty() { per_file.push(v); }
    }
    // Round-robin across files so one big file doesn't consume the budget.
    let mut out = vec![]; let mut idx = 0usize;
    while out.len() < max && per_file.iter().any(|v| !v.is_empty()) {
        let n = per_file.len(); let v = &mut per_file[idx % n];
        if !v.is_empty() { let step = (v.len() / 3).max(1); let pick = (v.len() - 1) % step; out.push(v.remove(pick.min(v.len() - 1))); }
        idx += 1;
    }
    out
}

/// Restores a file's original content on drop (panic/cancel safe). Skips the restore when the file no
/// longer holds our mutant (someone else edited it meanwhile) so we never clobber user work.
struct Restore { abs: PathBuf, original: String, expected: String, armed: bool }
impl Drop for Restore {
    fn drop(&mut self) {
        if !self.armed { return; }
        match std::fs::read_to_string(&self.abs) {
            Ok(cur) if cur == self.expected => { let _ = std::fs::write(&self.abs, &self.original); }
            Ok(_) => eprintln!("verify/mutation: {} changed during run; leaving as-is", self.abs.display()),
            Err(_) => { let _ = std::fs::write(&self.abs, &self.original); }
        }
    }
}

fn apply(content: &str, m: &Mutant) -> Option<String> {
    let mut lines: Vec<&str> = content.split_inclusive('\n').collect();
    let i = (m.line as usize).checked_sub(1)?;
    let cur = lines.get(i)?;
    if cur.trim_end_matches(['\n', '\r']) != m.original { return None; }
    let nl = if cur.ends_with("\r\n") { "\r\n" } else if cur.ends_with('\n') { "\n" } else { "" };
    let replaced = format!("{}{}", m.mutated, nl);
    lines[i] = &replaced;
    Some(lines.concat())
}

pub async fn run(ctx: &CheckCtx, check: &mut VerifyCheck, prior: &[VerifyCheck]) -> Result<()> {
    let tests = prior.iter().find(|c| c.kind == "tests");
    match tests { Some(t) if t.status == "pass" => {}, Some(t) => { check.skip(format!("tests {} — mutation needs a green suite", t.status)); return Ok(()); }, None => { check.skip("tests check not run"); return Ok(()); } }
    let Some(cmd) = super::tests::detect_test_command(ctx) else { check.skip("no test command"); return Ok(()); };
    let max = (ctx.settings.verify.mutation_max_mutants as usize).clamp(1, 60);
    let mutants = generate(&ctx.scope.files, max);
    if mutants.is_empty() { check.skip("no mutable added lines in source files"); return Ok(()); }
    check.command = Some(cmd.clone());
    let base_ms = tests.and_then(|t| t.duration_ms).unwrap_or(10_000).max(1_000);
    let per_mutant = Duration::from_millis((base_ms * 3 + 10_000).min(ctx.timeout().as_millis() as u64));
    let budget = ctx.timeout();
    let started = Instant::now();
    let (mut killed, mut survived, mut timeouts, mut skipped) = (0usize, 0usize, 0usize, 0usize);
    let mut survivors: Vec<&Mutant> = vec![];
    for m in &mutants {
        if ctx.cancel.is_cancelled() { skipped += 1; continue; }
        if started.elapsed() + per_mutant > budget { skipped += 1; continue; }
        let Ok(current) = std::fs::read_to_string(&m.abs) else { skipped += 1; continue; };
        let Some(mutated) = apply(&current, m) else { skipped += 1; continue; };
        let guard = Restore { abs: m.abs.clone(), original: current.clone(), expected: mutated.clone(), armed: true };
        if std::fs::write(&m.abs, &mutated).is_err() { skipped += 1; continue; }
        let out = shell::run(&ctx.root, &cmd, per_mutant, &ctx.cancel, &ctx.settings, &[("CARET_MUTANT", "1")]).await;
        drop(guard);
        match out {
            Ok(o) if o.timed_out => { timeouts += 1; killed += 1; }
            Ok(o) => { let failed = o.code != Some(0) || super::tests::parse_summary(&o.output).map(|s| s.failed > 0).unwrap_or(false); if failed { killed += 1; } else { survived += 1; survivors.push(m); } }
            Err(_) => { skipped += 1; }
        }
    }
    let tested = killed + survived;
    for m in survivors.iter().take(20) {
        check.findings.push(VerifyFinding::new("medium", format!("tests still pass when `{}` becomes `{}` ({})", m.original.trim(), m.mutated.trim(), m.operator)).at(&m.path, Some(m.line)).rule("mutant-survived").suggest("add a test that exercises this line's behaviour"));
    }
    check.metric("mutants", mutants.len() as f64); check.metric("killed", killed as f64); check.metric("survived", survived as f64); check.metric("timeouts", timeouts as f64); check.metric("skipped", skipped as f64);
    if tested == 0 { check.skip(format!("no mutants could be tested ({skipped} skipped)")); return Ok(()); }
    let score = killed as f64 / tested as f64;
    check.metric("score", (score * 1000.0).round() / 1000.0);
    check.summary = format!("{killed}/{tested} mutants killed ({:.0}%){}", score * 100.0, if skipped > 0 { format!(", {skipped} skipped for budget") } else { String::new() });
    check.status = if score >= 0.6 { "pass".into() } else if score >= 0.3 { "warn".into() } else { "fail".into() };
    Ok(())
}

#[allow(dead_code)]
fn _unused(_: &Path) {}

#[cfg(test)]
mod t {
    use super::*;
    use crate::ai::agent::session::ChangeKind;
    fn tf(path: &str, after: &str) -> TouchedFile { let added = after.lines().enumerate().map(|(i, l)| (i as u32 + 1, l.to_string())).collect(); TouchedFile { path: path.into(), abs: PathBuf::from(path), before: None, after: Some(after.into()), added_lines: added, removed_lines: vec![], diff: String::new(), lang: crate::verify::scope::lang_of(path), class: crate::verify::scope::classify(path), kind: ChangeKind::Create } }
    #[test]
    fn generates_and_applies() {
        let f = tf("src/calc.ts", "export function discount(p: number, pct: number) {\n  if (pct > 100) return 0;\n  const s = 'a + b';\n  return p - p * pct / 100;\n}\n");
        let ms = generate(&[f.clone()], 12);
        assert!(ms.iter().any(|m| m.line == 2 && m.operator == "gt→ge"));
        assert!(!ms.iter().any(|m| m.line == 3), "string-only lines are skipped");
        let m = ms.iter().find(|m| m.line == 2).unwrap();
        let applied = apply(f.after.as_deref().unwrap(), m).unwrap();
        assert!(applied.contains("if (pct >= 100) return 0;"));
        assert!(apply("different\ncontent\n", m).is_none(), "content guard refuses mismatched lines");
    }
    #[test]
    fn python_ops() { let f = tf("app.py", "def ok(x):\n    return x > 1 and x < 9\n"); let ms = generate(&[f], 5); assert!(ms.iter().any(|m| matches!(m.operator, "gt→ge" | "lt→le" | "and→or"))); }
    #[test]
    fn cap_and_spread() { let a = tf("a.ts", &(0..30).map(|i| format!("if (x > {i}) y = {i};")).collect::<Vec<_>>().join("\n")); let b = tf("b.ts", &(0..30).map(|i| format!("if (x < {i}) y = {i};")).collect::<Vec<_>>().join("\n")); let ms = generate(&[a, b], 6); assert_eq!(ms.len(), 6); assert!(ms.iter().filter(|m| m.path == "a.ts").count() == 3); }
    #[test]
    fn restore_guard() {
        let dir = tempfile::tempdir().unwrap(); let p = dir.path().join("f.txt"); std::fs::write(&p, "orig").unwrap();
        { let g = Restore { abs: p.clone(), original: "orig".into(), expected: "mut".into(), armed: true }; std::fs::write(&p, "mut").unwrap(); drop(g); }
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "orig");
        { let g = Restore { abs: p.clone(), original: "orig".into(), expected: "mut".into(), armed: true }; std::fs::write(&p, "user-edit").unwrap(); drop(g); }
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "user-edit", "user edits are never clobbered");
    }
}

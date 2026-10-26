//! Regex security rules applied to added lines (plus optional semgrep when installed).
use super::CheckCtx;
use crate::error::Result;
use crate::verify::scope::{FileClass, Lang};
use crate::verify::{shell, VerifyCheck, VerifyFinding};
use once_cell::sync::Lazy;
use regex::Regex;

pub struct Rule { pub id: &'static str, pub severity: &'static str, pub langs: &'static [Lang], pub re: Regex, pub message: &'static str, pub suggestion: &'static str, pub fix: Option<fn(&str) -> Option<String>>, /// Skip when the line contains any of these substrings (stands in for look-around, which `regex` lacks).
    pub unless: &'static [&'static str] }
impl Rule { fn unless(mut self, u: &'static [&'static str]) -> Self { self.unless = u; self } }
const ALL: &[Lang] = &[];
const JS: &[Lang] = &[Lang::Js, Lang::Ts];
const PY: &[Lang] = &[Lang::Python];
const RS: &[Lang] = &[Lang::Rust];
const GO: &[Lang] = &[Lang::Go];
const SH: &[Lang] = &[Lang::Shell];
const WEB: &[Lang] = &[Lang::Js, Lang::Ts, Lang::Python, Lang::Go, Lang::Rust, Lang::Ruby, Lang::Php, Lang::Java, Lang::CSharp];

fn r(id: &'static str, severity: &'static str, langs: &'static [Lang], re: &str, message: &'static str, suggestion: &'static str) -> Rule { Rule { id, severity, langs, re: Regex::new(re).expect(id), message, suggestion, fix: None, unless: &[] } }
fn rf(id: &'static str, severity: &'static str, langs: &'static [Lang], re: &str, message: &'static str, suggestion: &'static str, fix: fn(&str) -> Option<String>) -> Rule { Rule { id, severity, langs, re: Regex::new(re).expect(id), message, suggestion, fix: Some(fix), unless: &[] } }

fn fix_tls_reject(l: &str) -> Option<String> { Some(l.replace("NODE_TLS_REJECT_UNAUTHORIZED'] = '0'", "NODE_TLS_REJECT_UNAUTHORIZED'] = '1'").replace("NODE_TLS_REJECT_UNAUTHORIZED = '0'", "NODE_TLS_REJECT_UNAUTHORIZED = '1'").replace("NODE_TLS_REJECT_UNAUTHORIZED=0", "NODE_TLS_REJECT_UNAUTHORIZED=1").replace("rejectUnauthorized: false", "rejectUnauthorized: true")).filter(|s| s != l) }
fn fix_py_verify(l: &str) -> Option<String> { Some(l.replace("verify=False", "verify=True")).filter(|s| s != l) }
fn fix_py_shell(l: &str) -> Option<String> { Some(l.replace("shell=True", "shell=False")).filter(|s| s != l) }
fn fix_go_tls(l: &str) -> Option<String> { Some(l.replace("InsecureSkipVerify: true", "InsecureSkipVerify: false")).filter(|s| s != l) }
fn fix_rs_tls(l: &str) -> Option<String> { Some(l.replace("danger_accept_invalid_certs(true)", "danger_accept_invalid_certs(false)")).filter(|s| s != l) }
fn fix_cors_star(l: &str) -> Option<String> { Some(l.replace("origin: '*'", "origin: process.env.CORS_ORIGIN").replace("origin: \"*\"", "origin: process.env.CORS_ORIGIN")).filter(|s| s != l) }
fn fix_eval(l: &str) -> Option<String> { if l.contains("eval(") && !l.contains("JSON.parse") { Some(l.replacen("eval(", "JSON.parse(", 1)) } else { None } }
fn fix_debug_true(l: &str) -> Option<String> { Some(l.replace("debug=True", "debug=False").replace("DEBUG = True", "DEBUG = False")).filter(|s| s != l) }
fn fix_md5(l: &str) -> Option<String> { Some(l.replace("createHash('md5')", "createHash('sha256')").replace("createHash(\"md5\")", "createHash(\"sha256\")").replace("hashlib.md5(", "hashlib.sha256(").replace("createHash('sha1')", "createHash('sha256')").replace("hashlib.sha1(", "hashlib.sha256(")).filter(|s| s != l) }
fn fix_yaml_load(l: &str) -> Option<String> { if l.contains("yaml.load(") && !l.contains("Loader") { Some(l.replace("yaml.load(", "yaml.safe_load(")) } else { None } }
fn fix_chmod(l: &str) -> Option<String> { Some(l.replace("chmod 777", "chmod 755").replace("0o777", "0o755").replace("0777", "0755")).filter(|s| s != l) }
fn fix_bind_all(l: &str) -> Option<String> { Some(l.replace("0.0.0.0", "127.0.0.1")).filter(|s| s != l) }
fn fix_curl_insecure(l: &str) -> Option<String> { Some(l.replace(" --insecure", "").replace(" -k ", " ")).filter(|s| s != l) }
fn fix_pipe_sh(l: &str) -> Option<String> { let _ = l; None }

pub static RULES: Lazy<Vec<Rule>> = Lazy::new(|| vec![
    // --- Secrets ---
    r("secret-aws-key", "critical", ALL, r"\bAKIA[0-9A-Z]{16}\b", "AWS access key ID committed in source", "move to environment/secret manager and rotate the key"),
    r("secret-private-key", "critical", ALL, r"-----BEGIN (?:RSA |EC |OPENSSH |DSA |PGP )?PRIVATE KEY-----", "private key material in source", "store keys outside the repo"),
    r("secret-github-token", "critical", ALL, r"\bgh[pousr]_[A-Za-z0-9]{36,}\b", "GitHub token in source", "rotate the token and read it from the environment"),
    r("secret-openai-key", "critical", ALL, r"\bsk-(?:proj-)?[A-Za-z0-9_-]{32,}\b", "OpenAI-style API key in source", "load from env; rotate"),
    r("secret-anthropic-key", "critical", ALL, r"\bsk-ant-[A-Za-z0-9_-]{20,}\b", "Anthropic API key in source", "load from env; rotate"),
    r("secret-slack-token", "critical", ALL, r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b", "Slack token in source", "rotate and move to env"),
    r("secret-stripe-key", "critical", ALL, r"\b[sr]k_live_[A-Za-z0-9]{16,}\b", "Stripe live key in source", "rotate and move to env"),
    r("secret-google-key", "high", ALL, r"\bAIza[0-9A-Za-z_-]{35}\b", "Google API key in source", "restrict + move to env"),
    r("secret-jwt-literal", "high", ALL, r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b", "hard-coded JWT", "never embed tokens in code"),
    r("secret-generic-assign", "high", ALL, r#"(?i)\b(?:api[_-]?key|secret|password|passwd|pwd|token|auth[_-]?token|access[_-]?token|client[_-]?secret)\b\s*[:=]\s*["'][A-Za-z0-9+/_\-=.]{12,}["']"#, "credential-looking literal assigned to a secret-named variable", "read from environment or a secret store"),
    r("secret-url-creds", "high", ALL, r"(?i)\b[a-z][a-z0-9+.-]*://[^\s/:@]+:[^\s/@]{3,}@[^\s/]+", "credentials embedded in a URL", "use env-based config; rotate"),
    // --- Injection ---
    rf("js-eval", "high", JS, r"(?:^|[^\w.])eval\s*\(", "eval() on dynamic input", "avoid eval; use JSON.parse or explicit dispatch", fix_eval),
    r("js-new-function", "high", JS, r"\bnew\s+Function\s*\(", "new Function() constructs code from strings", "avoid dynamic code generation"),
    r("js-child-process-exec", "high", JS, r"\b(?:exec|execSync)\s*\(\s*(?:`[^`]*\$\{|[^)]*\+)", "shell command built from interpolated input", "use execFile/spawn with an argv array"),
    r("js-innerhtml", "medium", JS, r#"\.innerHTML\s*=\s*[^'"`\s=]"#, "innerHTML assignment (XSS vector)", "use textContent or a sanitizer like DOMPurify"),
    r("js-dangerously-set", "medium", JS, r"dangerouslySetInnerHTML", "dangerouslySetInnerHTML (XSS vector)", "sanitize the HTML first"),
    r("js-document-write", "medium", JS, r"\bdocument\.write\s*\(", "document.write with dynamic content", "manipulate DOM nodes instead"),
    r("sql-string-concat", "high", WEB, r#"(?i)\b(?:select|insert|update|delete|drop)\b[^;\n]*?(?:\+\s*\w+|\$\{|%s|\.format\(|f["'])"#, "SQL statement built by string concatenation/interpolation", "use parameterized queries"),
    rf("py-shell-true", "high", PY, r"subprocess\.\w+\([^)]*shell\s*=\s*True", "subprocess with shell=True", "pass an argv list and shell=False", fix_py_shell),
    r("py-os-system", "high", PY, r"\bos\.(?:system|popen)\s*\(", "os.system/popen executes a shell string", "use subprocess.run([...])"),
    r("py-eval-exec", "high", PY, r"(?:^|[^\w.])(?:eval|exec)\s*\(", "eval/exec on dynamic input", "avoid dynamic code execution"),
    r("py-pickle-load", "high", PY, r"\bpickle\.loads?\s*\(", "pickle deserialization of untrusted data", "use JSON or a safe format"),
    rf("py-yaml-load", "medium", PY, r"\byaml\.load\s*\(", "yaml.load without a safe Loader", "use yaml.safe_load", fix_yaml_load).unless(&["Loader"]),
    r("go-exec-sh", "high", GO, r#"exec\.Command\(\s*"(?:sh|bash|zsh|cmd)"\s*,\s*"-c""#, "exec.Command running a shell string", "pass the program and args directly"),
    r("rs-command-sh", "medium", RS, r#"Command::new\(\s*"(?:sh|bash|zsh)"\s*\)[\s\S]{0,40}\.arg\(\s*"-c""#, "Command running a shell string", "pass args directly unless shell features are required"),
    // --- TLS / crypto ---
    rf("tls-node-reject", "critical", JS, r"NODE_TLS_REJECT_UNAUTHORIZED\W{0,6}0|rejectUnauthorized\s*:\s*false", "TLS certificate verification disabled", "re-enable verification; pin/trust the CA instead", fix_tls_reject),
    rf("tls-py-verify-false", "critical", PY, r"verify\s*=\s*False", "TLS verification disabled (verify=False)", "use verify=True or a CA bundle path", fix_py_verify),
    rf("tls-go-skip-verify", "critical", GO, r"InsecureSkipVerify\s*:\s*true", "TLS verification disabled", "set InsecureSkipVerify to false", fix_go_tls),
    rf("tls-rs-accept-invalid", "critical", RS, r"danger_accept_invalid_(?:certs|hostnames)\(true\)", "TLS verification disabled", "accept only valid certs", fix_rs_tls),
    rf("crypto-weak-hash", "medium", WEB, r#"createHash\(\s*['"](?:md5|sha1)['"]\s*\)|hashlib\.(?:md5|sha1)\(|Md5::new\(|Sha1::new\(|md5\.New\(\)|sha1\.New\(\)"#, "weak hash algorithm (MD5/SHA-1)", "use SHA-256 or better (or a password hash like argon2/bcrypt for passwords)", fix_md5),
    r("crypto-ecb", "high", WEB, r"(?i)\b(?:aes|des)[-_/]?(?:128|192|256)?[-_/]?ecb\b", "ECB mode cipher", "use AES-GCM or ChaCha20-Poly1305"),
    r("crypto-math-random-token", "medium", JS, r"(?i)(?:token|secret|nonce|salt|password|otp|session)\w*\s*=\s*[^;\n]*Math\.random\(", "Math.random used for a security token", "use crypto.randomBytes / crypto.getRandomValues"),
    r("crypto-py-random-token", "medium", PY, r"(?i)(?:token|secret|nonce|salt|password|otp|session)\w*\s*=\s*[^#\n]*\brandom\.(?:random|randint|choice|choices|randrange)\(", "random module used for a security token", "use the secrets module"),
    // --- Auth / web config ---
    rf("cors-wildcard-creds", "high", JS, r#"origin\s*:\s*['"]\*['"]"#, "CORS wildcard origin", "restrict origins to a known list", fix_cors_star),
    r("cors-reflect-origin", "medium", WEB, r"(?i)Access-Control-Allow-Origin[^\n]{0,40}(?:req\.headers\.origin|request\.headers\[.origin.\]|r\.Header\.Get\(.Origin.\))", "CORS reflects the request origin", "validate against an allowlist"),
    rf("debug-flag-on", "medium", PY, r"\b(?:debug\s*=\s*True|DEBUG\s*=\s*True)\b", "debug mode enabled", "disable debug in production", fix_debug_true),
    r("jwt-none-alg", "critical", WEB, r#"(?i)algorithms?\s*[:=]\s*\[?\s*['"]none['"]"#, "JWT 'none' algorithm accepted", "require a specific HMAC/RSA algorithm"),
    r("jwt-no-verify", "high", JS, r"jwt\.decode\s*\(", "JWT decoded without verification", "use jwt.verify with the key").unless(&["verify"]),
    r("cookie-insecure", "medium", JS, r"(?i)(?:secure\s*:\s*false|httpOnly\s*:\s*false)", "cookie without Secure/HttpOnly", "set secure and httpOnly: true"),
    r("auth-bypass-comment", "medium", ALL, r"(?i)(?://|#)\s*(?:TODO|FIXME|HACK)?\s*(?:skip|disable|bypass|remove)\s+(?:auth|authentication|authorization|permission|csrf|verification)", "comment indicates a security check was skipped", "restore the check before shipping"),
    r("csrf-disabled", "high", WEB, r"(?i)csrf[\w]*\s*(?:=\s*false|\(\s*false\s*\)|\.disable\(\)|_exempt)", "CSRF protection disabled", "keep CSRF protection for state-changing routes"),
    rf("bind-all-interfaces", "low", WEB, r#"['"]0\.0\.0\.0['"]"#, "server binds to all interfaces", "bind to 127.0.0.1 unless exposure is intended", fix_bind_all),
    // --- Filesystem / process ---
    rf("fs-world-writable", "medium", ALL, r"chmod\s+(?:-R\s+)?0?777\b|0o777\b|0777\b", "world-writable permissions", "use 0755/0644", fix_chmod),
    r("fs-path-traversal", "medium", WEB, r"(?i)(?:path\.join|os\.path\.join|filepath\.Join|readFile\w*|open)\s*\([^)\n]*(?:req\.(?:params|query|body)|request\.(?:args|form|GET|POST)|r\.URL\.Query)", "filesystem path built from request input", "normalize and ensure the path stays under the base directory"),
    r("fs-temp-predictable", "low", WEB, r#"['"]/tmp/[\w.-]+['"]"#, "predictable /tmp path", "use mkdtemp / tempfile"),
    rf("sh-curl-pipe-sh", "high", ALL, r"\b(?:curl|wget)\b[^|\n]*\|\s*(?:sudo\s+)?(?:ba|z|da)?sh\b", "piping a download straight into a shell", "download, verify checksum, then execute", fix_pipe_sh),
    rf("sh-curl-insecure", "medium", ALL, r"\bcurl\b[^\n]*\s(?:-k|--insecure)\b", "curl with TLS verification disabled", "drop --insecure", fix_curl_insecure),
    r("sh-rm-rf-var", "high", SH, r"rm\s+-[a-zA-Z]*r[a-zA-Z]*f?\s+(?:\$\{?\w+\}?/?|/\s|~/?\s)", "recursive delete rooted at a variable or /", "quote and validate the path; guard against empty variables"),
    r("sh-sudo-nopasswd", "high", ALL, r"NOPASSWD:\s*ALL", "passwordless sudo for all commands", "scope sudoers to specific commands"),
    // --- Logging / data ---
    r("log-secret", "medium", WEB, r"(?i)(?:console\.(?:log|debug|info)|print|logger?\.(?:info|debug|warn|error)|println!|log\.Printf?)\s*\([^)\n]*\b(?:password|passwd|secret|token|api[_-]?key|authorization|private[_-]?key)\b", "logging a secret-named value", "redact sensitive fields before logging"),
    r("regex-redos", "low", WEB, r"\((?:\[[^\]]+\]|\\w|\\d|\.)[*+]\)[*+]", "nested quantifier regex (ReDoS risk)", "rewrite without nested unbounded quantifiers"),
    r("http-cleartext", "low", WEB, r#"['"]http://[^'"\s]+"#, "cleartext http:// URL", "prefer https://").unless(&["localhost", "127.0.0.1", "0.0.0.0", "[::1]", "schemas.", "w3.org", "json-schema.org", "example.", "xmlns"]),
    r("docker-latest-root", "low", &[Lang::Other], r"(?m)^FROM\s+[\w./-]+(?::latest)?\s*$", "Dockerfile FROM without a pinned tag/digest", "pin a version or digest"),
    r("gha-unpinned-action", "low", &[Lang::Yaml], r"uses:\s*[\w./-]+@(?:main|master|latest|v?\d+)\s*$", "GitHub Action pinned to a mutable ref", "pin to a commit SHA"),
    r("tf-public-acl", "medium", &[Lang::Other], r#"acl\s*=\s*"public-read(?:-write)?""#, "public bucket ACL", "keep buckets private"),
]);

pub fn applies(rule: &Rule, lang: Lang) -> bool { rule.langs.is_empty() || rule.langs.contains(&lang) }

/// Fixable rewrite for `rule` applied to a single source line; None when the rule has no safe rewrite.
pub fn apply_fix(rule: &str, line: &str) -> Option<String> { RULES.iter().find(|r| r.id == rule).and_then(|r| r.fix).and_then(|f| f(line)) }

fn is_test_or_fixture(path: &str) -> bool { let p = path.to_lowercase(); p.contains("/test") || p.contains("__tests__") || p.contains(".test.") || p.contains(".spec.") || p.contains("/fixtures/") || p.contains("/mocks/") || p.ends_with(".md") }
fn is_comment(lang: Lang, text: &str) -> bool { let t = text.trim_start(); match lang { Lang::Python | Lang::Shell | Lang::Yaml | Lang::Toml => t.starts_with('#'), Lang::Markdown => true, _ => t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") } }

pub fn scan_line(path: &str, lang: Lang, line_no: u32, text: &str) -> Vec<VerifyFinding> {
    let mut out = vec![];
    if text.len() > 2000 { return out; }
    let commenty = is_comment(lang, text);
    for rule in RULES.iter() {
        if !applies(rule, lang) { continue; }
        if commenty && !rule.id.starts_with("secret-") && rule.id != "auth-bypass-comment" { continue; }
        if rule.unless.iter().any(|u| text.contains(u)) { continue; }
        if rule.re.is_match(text) {
            let mut sev = rule.severity.to_string();
            if is_test_or_fixture(path) && sev != "critical" { sev = "low".into(); }
            if rule.id.starts_with("secret-") && (text.contains("example") || text.contains("placeholder") || text.contains("xxxx") || text.contains("REPLACE") || text.contains("your-") || text.contains("<") ) { continue; }
            let mut f = VerifyFinding::new(&sev, rule.message).at(path, Some(line_no)).rule(rule.id).suggest(rule.suggestion).evidence(text.trim().chars().take(240).collect::<String>());
            f.fixable = Some(rule.fix.map(|fx| fx(text).is_some()).unwrap_or(false));
            out.push(f);
        }
    }
    out
}

pub async fn run(ctx: &CheckCtx, check: &mut VerifyCheck) -> Result<()> {
    let mut scanned_lines = 0usize;
    for f in &ctx.scope.files {
        if f.class == FileClass::Lockfile || f.after.is_none() { continue; }
        for (ln, text) in &f.added_lines { scanned_lines += 1; check.findings.extend(scan_line(&f.path, f.lang, *ln, text)); if check.findings.len() >= 200 { break; } }
    }
    // Fallback: a created file with no diff lines gets scanned whole.
    for f in ctx.scope.files.iter().filter(|f| f.added_lines.is_empty() && f.before.is_none()) { if let Some(after) = &f.after { for (i, text) in after.lines().enumerate() { scanned_lines += 1; check.findings.extend(scan_line(&f.path, f.lang, i as u32 + 1, text)); } } }
    check.metric("lines_scanned", scanned_lines as f64);
    let regex_count = check.findings.len();
    // Optional semgrep (bounded; only on touched files; never fails the check on its own errors).
    if ctx.settings.verify.use_semgrep_if_available && shell::which("semgrep") && !ctx.scope.files.is_empty() && !ctx.privacy() {
        let files: Vec<String> = ctx.scope.files.iter().filter(|f| f.after.is_some() && f.class != FileClass::Lockfile).map(|f| format!("'{}'", f.path)).collect();
        let cmd = format!("semgrep scan --config p/security-audit --config p/secrets --json --quiet --metrics=off --timeout 20 {}", files.join(" "));
        check.command = Some(cmd.clone());
        if let Ok(out) = shell::run(&ctx.root, &cmd, std::time::Duration::from_secs(90), &ctx.cancel, &ctx.settings, &[("SEMGREP_SEND_METRICS", "off")]).await {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(out.output.trim().lines().find(|l| l.starts_with('{')).unwrap_or("")) {
                for res in v["results"].as_array().cloned().unwrap_or_default() {
                    let sev = match res["extra"]["severity"].as_str().unwrap_or("") { "ERROR" => "high", "WARNING" => "medium", _ => "low" };
                    let path = crate::paths::rel_to(&ctx.root, std::path::Path::new(res["path"].as_str().unwrap_or("")));
                    let line = res["start"]["line"].as_u64().map(|l| l as u32);
                    let msg = res["extra"]["message"].as_str().unwrap_or("").lines().next().unwrap_or("").to_string();
                    let id = res["check_id"].as_str().unwrap_or("semgrep").rsplit('.').next().unwrap_or("semgrep").to_string();
                    if check.findings.iter().any(|f| f.path.as_deref() == Some(path.as_str()) && f.line == line) { continue; }
                    check.findings.push(VerifyFinding::new(sev, msg).at(&path, line).rule(&format!("semgrep:{id}")));
                    if check.findings.len() >= 250 { break; }
                }
            }
        }
    }
    let crit = super::count_by(check, "critical"); let high = super::count_by(check, "high"); let med = super::count_by(check, "medium");
    check.metric("critical", crit as f64); check.metric("high", high as f64); check.metric("medium", med as f64);
    check.summary = if check.findings.is_empty() { format!("{} rules × {} added lines: clean", RULES.len(), scanned_lines) } else { format!("{} finding(s): {} critical, {} high, {} medium{}", check.findings.len(), crit, high, med, if check.findings.len() > regex_count { " (incl. semgrep)" } else { "" }) };
    check.settle(false);
    Ok(())
}

#[cfg(test)]
mod t {
    use super::*;
    fn ids(path: &str, lang: Lang, s: &str) -> Vec<String> { scan_line(path, lang, 1, s).into_iter().map(|f| f.rule.unwrap()).collect() }
    #[test]
    fn rules_compile_and_fire() {
        assert_eq!(RULES.len() >= 40, true);
        assert!(ids("src/a.ts", Lang::Ts, "const key = 'AKIAIOSFODNN7REALKEY';").contains(&"secret-aws-key".to_string()));
        assert!(ids("app.py", Lang::Python, "d = yaml.load(s, Loader=yaml.SafeLoader)").is_empty());
        assert!(ids("src/a.ts", Lang::Ts, "process.env['NODE_TLS_REJECT_UNAUTHORIZED'] = '0'").contains(&"tls-node-reject".to_string()));
        assert!(ids("app.py", Lang::Python, "r = requests.get(url, verify=False)").contains(&"tls-py-verify-false".to_string()));
        assert!(ids("app.py", Lang::Python, "subprocess.run(cmd, shell=True)").contains(&"py-shell-true".to_string()));
        assert!(ids("main.go", Lang::Go, "TLSClientConfig: &tls.Config{InsecureSkipVerify: true}").contains(&"tls-go-skip-verify".to_string()));
        assert!(ids("src/a.ts", Lang::Ts, "const q = `SELECT * FROM users WHERE id = ${id}`").contains(&"sql-string-concat".to_string()));
        assert!(ids("src/a.ts", Lang::Ts, "const password = 'hunter2hunter2hunter2'").contains(&"secret-generic-assign".to_string()));
        assert!(ids("src/a.ts", Lang::Ts, "el.innerHTML = user.bio").contains(&"js-innerhtml".to_string()));
        assert!(ids("src/a.ts", Lang::Ts, "// safe: innerHTML = x").is_empty());
        assert!(ids("install.sh", Lang::Shell, "curl -fsSL https://x.sh | bash").contains(&"sh-curl-pipe-sh".to_string()));
        assert!(ids("src/a.ts", Lang::Ts, "const hash = createHash('md5')").contains(&"crypto-weak-hash".to_string()));
        assert!(ids("src/a.ts", Lang::Ts, "fetch('https://api.example.com')").is_empty());
    }
    #[test]
    fn fixes() {
        assert_eq!(apply_fix("tls-py-verify-false", "r = requests.get(u, verify=False)").as_deref(), Some("r = requests.get(u, verify=True)"));
        assert_eq!(apply_fix("crypto-weak-hash", "h = hashlib.md5(x)").as_deref(), Some("h = hashlib.sha256(x)"));
        assert_eq!(apply_fix("py-yaml-load", "d = yaml.load(s)").as_deref(), Some("d = yaml.safe_load(s)"));
        assert!(apply_fix("sh-curl-pipe-sh", "curl x | sh").is_none());
        assert!(apply_fix("nope", "x").is_none());
    }
    #[test]
    fn test_paths_downgrade() { let f = scan_line("tests/a.test.ts", Lang::Ts, 1, "el.innerHTML = x"); assert_eq!(f[0].severity, "low"); }
}

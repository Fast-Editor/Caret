//! Hand-written POSIX-shell tokenizer/parser used for deterministic command classification.
//!
//! Goals: never panic on any input, understand quoting/escapes/operators/redirects, surface command and
//! process substitutions, subshell groups and — crucially — *nested shells* (`sh -c "…"`, `eval`, `xargs`,
//! `find -exec`, `python -c`, `node -e`, `ssh host 'cmd'`) so that evasions are visible to the classifier.
//! This is not a complete shell grammar; it is a conservative structural parser.

const MAX_INPUT: usize = 64 * 1024;
const MAX_DEPTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    /// Explicit file descriptor (`2>`), None for default.
    pub fd: Option<u32>,
    /// One of `>`, `>>`, `<`, `<<`, `>&`, `<&`, `&>`, `&>>`, `>|`.
    pub op: String,
    /// Target word (unquoted text), e.g. `/dev/null`, `&1`.
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Word {
    /// Unquoted value (quotes/escapes removed; substitutions kept verbatim).
    pub text: String,
    /// Raw source text of the word.
    pub raw: String,
    /// Any part of the word was quoted.
    pub quoted: bool,
    /// Bodies of `$( … )` / backtick substitutions found inside the word.
    pub subs: Vec<String>,
    /// Contains an unquoted glob metacharacter (`*`, `?`, `[`).
    pub globbed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SimpleCommand {
    /// Raw source text of this command (trimmed).
    pub raw: String,
    /// Leading `VAR=value` assignments.
    pub env: Vec<(String, String)>,
    /// Program + arguments *after* wrappers such as `sudo`, `env`, `nohup`, `time`, `xargs` were stripped.
    pub words: Vec<Word>,
    pub redirects: Vec<Redirect>,
    /// Terminated with `&`.
    pub background: bool,
    /// Commands joined by `|` share a pipeline id.
    pub pipeline: usize,
    pub depth: usize,
    /// Commands discovered inside this one: substitutions, subshell groups, `-c` payloads, `eval`, `xargs`, `find -exec`…
    pub nested: Vec<SimpleCommand>,
    /// Wrappers / carriers seen on the way to the real program (`sudo`, `env`, `xargs`, `sh -c`, `eval`, `ssh`…).
    pub via: Vec<String>,
    /// `(interpreter, payload)` for inline scripts such as `python -c '…'`.
    pub script: Option<(String, String)>,
    /// True when this node is a `( … )` group placeholder.
    pub group: bool,
}

impl SimpleCommand {
    /// Program basename (after wrappers), with leading backslash (`\rm`) and path stripped.
    pub fn program(&self) -> Option<String> {
        let w = self.words.first()?;
        let t = w.text.trim_start_matches('\\');
        let base = t.rsplit('/').next().unwrap_or(t);
        if base.is_empty() { None } else { Some(base.to_string()) }
    }
    /// Arguments after the program (unquoted text).
    pub fn args(&self) -> Vec<&str> { self.words.iter().skip(1).map(|w| w.text.as_str()).collect() }
    /// Non-option arguments (do not start with `-`), excluding a lone `--` separator.
    pub fn operands(&self) -> Vec<&Word> {
        let mut seen_dd = false;
        self.words.iter().skip(1).filter(|w| {
            if w.text == "--" && !seen_dd { seen_dd = true; return false; }
            seen_dd || !w.text.starts_with('-') || w.text == "-"
        }).collect()
    }
    /// All short flags from `-abc` style options and long options (`--force`) as they appear.
    pub fn flags(&self) -> Vec<String> {
        let mut out = vec![];
        for w in self.words.iter().skip(1) {
            let t = &w.text;
            if t == "--" { break; }
            if t.starts_with("--") { out.push(t.clone()); }
            else if t.starts_with('-') && t.len() > 1 { for c in t[1..].chars() { out.push(format!("-{c}")); } }
        }
        out
    }
    pub fn has_flag(&self, flag: &str) -> bool { self.flags().iter().any(|f| f == flag) }
    /// Whole command text including nested bodies, lowercased — for substring heuristics.
    pub fn full_text_lower(&self) -> String {
        let mut s = self.raw.to_lowercase();
        for n in &self.nested { s.push('\n'); s.push_str(&n.full_text_lower()); }
        if let Some((_, p)) = &self.script { s.push('\n'); s.push_str(&p.to_lowercase()); }
        s
    }
}

// ───────────────────────────── tokenizer ─────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Tok { Word(Word), Op(String), Redir { fd: Option<u32>, op: String }, LParen, RParen, Newline }

#[derive(Debug, Clone)]
struct Spanned { tok: Tok, start: usize, end: usize }

fn is_op_start(c: char) -> bool { matches!(c, '|' | '&' | ';' | '(' | ')' | '<' | '>' | '\n') }

/// Scan a balanced region starting *after* `open` at `i`, returning the index of the matching close and the body.
fn scan_balanced(chars: &[char], mut i: usize, open: char, close: char) -> (usize, String) {
    let mut depth = 1usize; let mut body = String::new();
    let (mut sq, mut dq) = (false, false);
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && !sq && i + 1 < chars.len() { body.push(c); body.push(chars[i + 1]); i += 2; continue; }
        if c == '\'' && !dq { sq = !sq; }
        else if c == '"' && !sq { dq = !dq; }
        else if !sq && !dq {
            if c == open { depth += 1; }
            else if c == close { depth -= 1; if depth == 0 { return (i, body); } }
        }
        body.push(c); i += 1;
    }
    (chars.len(), body)
}

fn tokenize(src: &str) -> Vec<Spanned> {
    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();
    let mut toks: Vec<Spanned> = vec![];
    let mut i = 0usize;
    let mut at_word_start = true; // for comment detection
    while i < n {
        let c = chars[i];
        if c == ' ' || c == '\t' || c == '\r' { i += 1; at_word_start = true; continue; }
        if c == '#' && at_word_start { while i < n && chars[i] != '\n' { i += 1; } continue; }
        // fd-prefixed redirect: `2>`, `2>&1`, `1>>`
        if c.is_ascii_digit() {
            let mut j = i; while j < n && chars[j].is_ascii_digit() { j += 1; }
            if j < n && (chars[j] == '>' || chars[j] == '<') && (i == 0 || chars[i - 1].is_whitespace() || is_op_start(chars[i - 1])) {
                let fd: u32 = chars[i..j].iter().collect::<String>().parse().unwrap_or(1);
                let (op, len) = redirect_op(&chars, j);
                toks.push(Spanned { tok: Tok::Redir { fd: Some(fd), op }, start: i, end: j + len });
                i = j + len; at_word_start = false; continue;
            }
        }
        if is_op_start(c) {
            let start = i;
            let two: String = chars[i..(i + 2).min(n)].iter().collect();
            let three: String = chars[i..(i + 3).min(n)].iter().collect();
            let (tok, len) = if c == '\n' { (Tok::Newline, 1) }
                else if c == '(' { (Tok::LParen, 1) }
                else if c == ')' { (Tok::RParen, 1) }
                else if three == "&>>" { (Tok::Redir { fd: None, op: "&>>".into() }, 3) }
                else if two == "&&" || two == "||" || two == "|&" || two == ";;" { (Tok::Op(two.clone()), 2) }
                else if two == "&>" { (Tok::Redir { fd: None, op: "&>".into() }, 2) }
                else if c == '>' || c == '<' { let (op, len) = redirect_op(&chars, i); (Tok::Redir { fd: None, op }, len) }
                else { (Tok::Op(c.to_string()), 1) };
            toks.push(Spanned { tok, start, end: start + len });
            i += len; at_word_start = true; continue;
        }
        // word
        let start = i;
        let mut w = Word::default();
        let (mut sq, mut dq) = (false, false);
        while i < n {
            let c = chars[i];
            if sq {
                if c == '\'' { sq = false; } else { w.text.push(c); }
                i += 1; continue;
            }
            if dq {
                match c {
                    '"' => { dq = false; i += 1; }
                    '\\' if i + 1 < n && matches!(chars[i + 1], '"' | '\\' | '$' | '`' | '\n') => { w.text.push(chars[i + 1]); i += 2; }
                    '$' if i + 1 < n && chars[i + 1] == '(' => { let (end, body) = scan_balanced(&chars, i + 2, '(', ')'); w.subs.push(body.clone()); w.text.push_str("$("); w.text.push_str(&body); w.text.push(')'); i = (end + 1).min(n); }
                    '`' => { let (end, body) = scan_backtick(&chars, i + 1); w.subs.push(body.clone()); w.text.push('`'); w.text.push_str(&body); w.text.push('`'); i = (end + 1).min(n); }
                    _ => { w.text.push(c); i += 1; }
                }
                continue;
            }
            // unquoted
            if c.is_whitespace() || is_op_start(c) { break; }
            match c {
                '\'' => { sq = true; w.quoted = true; i += 1; }
                '"' => { dq = true; w.quoted = true; i += 1; }
                '\\' => { if i + 1 < n { w.text.push(chars[i + 1]); w.quoted = true; i += 2; } else { i += 1; } }
                '$' if i + 1 < n && chars[i + 1] == '(' => {
                    // process substitution `<(` / `>(` is handled below; here `$(`
                    let (end, body) = scan_balanced(&chars, i + 2, '(', ')');
                    w.subs.push(body.clone()); w.text.push_str("$("); w.text.push_str(&body); w.text.push(')'); i = (end + 1).min(n);
                }
                '$' if i + 1 < n && chars[i + 1] == '{' => { let (end, body) = scan_balanced(&chars, i + 2, '{', '}'); w.text.push_str("${"); w.text.push_str(&body); w.text.push('}'); i = (end + 1).min(n); }
                '`' => { let (end, body) = scan_backtick(&chars, i + 1); w.subs.push(body.clone()); w.text.push('`'); w.text.push_str(&body); w.text.push('`'); i = (end + 1).min(n); }
                '*' | '?' | '[' => { w.globbed = true; w.text.push(c); i += 1; }
                _ => { w.text.push(c); i += 1; }
            }
        }
        // process substitution `<(cmd)` / `>(cmd)` directly after the word boundary
        if w.text.is_empty() && i < n && (chars[i] == '<' || chars[i] == '>') && i + 1 < n && chars[i + 1] == '(' {
            let (end, body) = scan_balanced(&chars, i + 2, '(', ')');
            w.subs.push(body.clone()); w.text = format!("{}({})", chars[i], body); w.quoted = true; i = (end + 1).min(n);
        }
        w.raw = chars[start..i.min(n)].iter().collect();
        if w.raw.is_empty() { i += 1; continue; } // defensive: guarantee progress
        toks.push(Spanned { tok: Tok::Word(w), start, end: i });
        at_word_start = false;
    }
    toks
}

fn scan_backtick(chars: &[char], mut i: usize) -> (usize, String) {
    let mut body = String::new();
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() { body.push(chars[i + 1]); i += 2; continue; }
        if c == '`' { return (i, body); }
        body.push(c); i += 1;
    }
    (chars.len(), body)
}

fn redirect_op(chars: &[char], i: usize) -> (String, usize) {
    let s: String = chars[i..(i + 3).min(chars.len())].iter().collect();
    for cand in ["<<<", ">>|", "<<-"] { if s.starts_with(cand) { return (cand.into(), 3); } }
    let two: String = s.chars().take(2).collect();
    for cand in [">>", "<<", ">&", "<&", ">|", "<>"] { if two == cand { return (cand.into(), 2); } }
    (chars[i].to_string(), 1)
}

// ───────────────────────────── parser ─────────────────────────────

/// Parse a command line into its top-level simple commands (nested commands attached).
pub fn parse(cmd: &str) -> Vec<SimpleCommand> { parse_depth(cmd, 0) }

fn parse_depth(cmd: &str, depth: usize) -> Vec<SimpleCommand> {
    if depth > MAX_DEPTH || cmd.trim().is_empty() { return vec![]; }
    let src: String = if cmd.len() > MAX_INPUT { cmd.chars().take(MAX_INPUT).collect() } else { cmd.to_string() };
    let chars: Vec<char> = src.chars().collect();
    let toks = tokenize(&src);
    let mut out: Vec<SimpleCommand> = vec![];
    let mut pipeline = 0usize;
    let mut cur: Option<(usize, SimpleCommand)> = None; // (start char index, command)
    let mut i = 0usize;
    let finish = |cur: &mut Option<(usize, SimpleCommand)>, end: usize, out: &mut Vec<SimpleCommand>, chars: &[char], depth: usize| {
        if let Some((start, mut c)) = cur.take() {
            if c.words.is_empty() && c.env.is_empty() && c.nested.is_empty() && !c.group { return; }
            let e = end.min(chars.len()).max(start);
            c.raw = chars[start..e].iter().collect::<String>().trim().to_string();
            c.depth = depth;
            post_process(&mut c, depth);
            out.push(c);
        }
    };
    while i < toks.len() {
        let t = &toks[i];
        match &t.tok {
            Tok::Word(w) => {
                let (start, c) = cur.get_or_insert_with(|| (t.start, SimpleCommand { pipeline, ..Default::default() }));
                let _ = start;
                // leading env assignment?
                if c.words.is_empty() && !w.quoted { if let Some((k, v)) = split_assignment(&w.text) { c.env.push((k, v)); i += 1; continue; } }
                c.words.push(w.clone());
                i += 1;
            }
            Tok::Redir { fd, op } => {
                let (_, c) = cur.get_or_insert_with(|| (t.start, SimpleCommand { pipeline, ..Default::default() }));
                let target = if let Some(Spanned { tok: Tok::Word(tw), .. }) = toks.get(i + 1) { i += 1; tw.text.clone() } else { String::new() };
                c.redirects.push(Redirect { fd: *fd, op: op.clone(), target });
                i += 1;
            }
            Tok::Op(op) => {
                let end = t.start;
                match op.as_str() {
                    "|" | "|&" => { finish(&mut cur, end, &mut out, &chars, depth); }
                    "&" => { if let Some((_, c)) = cur.as_mut() { c.background = true; } finish(&mut cur, end, &mut out, &chars, depth); pipeline += 1; }
                    _ => { finish(&mut cur, end, &mut out, &chars, depth); pipeline += 1; }
                }
                i += 1;
            }
            Tok::Newline => { finish(&mut cur, t.start, &mut out, &chars, depth); pipeline += 1; i += 1; }
            Tok::LParen => {
                // find matching RParen in token stream
                let mut d = 1usize; let mut j = i + 1;
                while j < toks.len() { match toks[j].tok { Tok::LParen => d += 1, Tok::RParen => { d -= 1; if d == 0 { break; } } _ => {} } j += 1; }
                let inner_start = t.end; let inner_end = toks.get(j).map(|s| s.start).unwrap_or(chars.len());
                let inner: String = chars[inner_start.min(chars.len())..inner_end.min(chars.len())].iter().collect();
                let (_, c) = cur.get_or_insert_with(|| (t.start, SimpleCommand { pipeline, ..Default::default() }));
                c.group = true;
                c.nested.extend(parse_depth(&inner, depth + 1));
                i = j + 1;
            }
            Tok::RParen => { i += 1; } // stray
        }
    }
    finish(&mut cur, chars.len(), &mut out, &chars, depth);
    out
}

fn split_assignment(w: &str) -> Option<(String, String)> {
    let eq = w.find('=')?;
    let (k, v) = (&w[..eq], &w[eq + 1..]);
    if k.is_empty() || !k.chars().next().map(|c| c.is_ascii_alphabetic() || c == '_').unwrap_or(false) || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') { return None; }
    Some((k.to_string(), v.to_string()))
}

const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "fish", "ash", "csh", "tcsh"];
const INTERPRETERS: &[&str] = &["python", "python3", "python2", "node", "nodejs", "ruby", "perl", "php", "deno", "bun", "lua", "osascript"];

/// Strip wrappers, detect nested shells / scripts / substitutions and attach them as `nested`.
fn post_process(c: &mut SimpleCommand, depth: usize) {
    // substitutions inside any word
    let mut subs: Vec<String> = vec![];
    for w in &c.words { subs.extend(w.subs.iter().cloned()); }
    for r in &c.redirects { if r.target.contains("$(") { if let Some(s) = r.target.find("$(") { subs.push(r.target[s + 2..].trim_end_matches(')').to_string()); } } }
    for s in subs { c.via.push("substitution".into()); c.nested.extend(parse_depth(&s, depth + 1)); }

    let mut guard = 0;
    loop {
        guard += 1; if guard > 12 { break; }
        let Some(prog) = c.program() else { break };
        match prog.as_str() {
            "sudo" | "doas" => {
                c.via.push(prog.clone());
                // drop options; -u/-g/-C/-h/-p/-r/-t/-U take an argument
                let mut idx = 1;
                while idx < c.words.len() {
                    let t = c.words[idx].text.clone();
                    if t == "--" { idx += 1; break; }
                    if t.starts_with('-') { if matches!(t.as_str(), "-u" | "-g" | "-C" | "-h" | "-p" | "-r" | "-t" | "-U" | "--user" | "--group") { idx += 2; } else { idx += 1; } continue; }
                    if split_assignment(&t).is_some() { idx += 1; continue; }
                    break;
                }
                c.words.drain(1..idx.min(c.words.len())); c.words.remove(0);
            }
            "env" | "nohup" | "time" | "nice" | "ionice" | "command" | "builtin" | "exec" | "caffeinate" | "stdbuf" | "unbuffer" | "chronic" | "watch" | "timeout" | "gtimeout" => {
                c.via.push(prog.clone());
                let takes_arg = matches!(prog.as_str(), "timeout" | "gtimeout" | "watch" | "nice" | "ionice" | "stdbuf");
                let mut idx = 1;
                while idx < c.words.len() {
                    let t = c.words[idx].text.clone();
                    if t == "--" { idx += 1; break; }
                    if t.starts_with('-') { if matches!(t.as_str(), "-n" | "-i" | "-o" | "-e" | "-u" | "-C" | "-s" | "-k" | "--signal" | "--kill-after" | "-c" | "--class") && takes_arg { idx += 2; } else { idx += 1; } continue; }
                    if prog == "env" && split_assignment(&t).is_some() { c.env.push(split_assignment(&t).unwrap()); idx += 1; continue; }
                    if matches!(prog.as_str(), "timeout" | "gtimeout") && t.chars().all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | 's' | 'm' | 'h' | 'd')) { idx += 1; continue; } // duration
                    break;
                }
                if idx >= c.words.len() { break; } // bare `env` / `time` etc.: keep as the program
                c.words.drain(0..idx);
            }
            "npx" | "bunx" | "uvx" => {
                let mut idx = 1;
                while idx < c.words.len() {
                    let t = c.words[idx].text.clone();
                    if t == "--" { idx += 1; break; }
                    if t.starts_with('-') { if matches!(t.as_str(), "-p" | "--package" | "-c" | "--call" | "--from" | "--with" | "--python" | "-w") { idx += 2; } else { idx += 1; } continue; }
                    break;
                }
                if idx >= c.words.len() { break; }
                c.via.push(prog.clone());
                c.words.drain(0..idx);
            }
            "npm" | "pnpm" | "yarn" | "bun" if c.words.get(1).map(|w| matches!(w.text.as_str(), "exec" | "x" | "dlx")).unwrap_or(false) => {
                let mut idx = 2;
                while idx < c.words.len() {
                    let t = c.words[idx].text.clone();
                    if t == "--" { idx += 1; break; }
                    if t.starts_with('-') { if matches!(t.as_str(), "-p" | "--package" | "-c" | "--call") { idx += 2; } else { idx += 1; } continue; }
                    break;
                }
                if idx >= c.words.len() { break; }
                c.via.push(format!("{prog} {}", if prog == "npm" { "exec" } else { "dlx" }));
                c.words.drain(0..idx);
            }
            "xargs" => {
                c.via.push("xargs".into());
                let mut idx = 1;
                while idx < c.words.len() {
                    let t = c.words[idx].text.clone();
                    if t.starts_with('-') {
                        if matches!(t.as_str(), "-n" | "-I" | "-L" | "-P" | "-d" | "-s" | "-E" | "--max-args" | "--max-procs" | "--delimiter" | "--replace") { idx += 2; } else { idx += 1; }
                        continue;
                    }
                    break;
                }
                if idx >= c.words.len() { c.words = vec![Word { text: "echo".into(), raw: "echo".into(), ..Default::default() }]; break; }
                c.words.drain(0..idx);
            }
            "su" => {
                c.via.push("su".into());
                if let Some(pos) = c.words.iter().position(|w| w.text == "-c") { if let Some(p) = c.words.get(pos + 1) { let payload = p.text.clone(); c.nested.extend(parse_depth(&payload, depth + 1)); } }
                break;
            }
            "ssh" => {
                c.via.push("ssh".into());
                // remote command = first operand after host
                let ops: Vec<String> = c.operands().iter().map(|w| w.text.clone()).collect();
                if ops.len() >= 2 { let payload = ops[1..].join(" "); c.nested.extend(parse_depth(&payload, depth + 1)); }
                break;
            }
            "eval" => {
                c.via.push("eval".into());
                let payload = c.args().join(" ");
                c.nested.extend(parse_depth(&payload, depth + 1));
                break;
            }
            "find" | "fd" | "gfind" => {
                let ws: Vec<String> = c.words.iter().map(|w| w.text.clone()).collect();
                let mut k = 1;
                while k < ws.len() {
                    if matches!(ws[k].as_str(), "-exec" | "-execdir" | "-ok" | "-okdir" | "--exec" | "-x" | "-X") {
                        let mut j = k + 1; let mut parts = vec![];
                        while j < ws.len() && !matches!(ws[j].as_str(), ";" | "\\;" | "+" | "\\+") { parts.push(ws[j].clone()); j += 1; }
                        if !parts.is_empty() { c.via.push("find-exec".into()); c.nested.extend(parse_depth(&parts.join(" "), depth + 1)); }
                        k = j;
                    }
                    k += 1;
                }
                break;
            }
            p if SHELLS.contains(&p) => {
                // `sh -c payload`, `bash -lc payload`, `zsh -ic payload`; also `sh script.sh`
                let ws: Vec<String> = c.words.iter().map(|w| w.text.clone()).collect();
                let mut payload: Option<String> = None;
                let mut k = 1;
                while k < ws.len() {
                    let t = &ws[k];
                    if t.starts_with("--") { k += 1; continue; }
                    if t.starts_with('-') && t.contains('c') {
                        // next non-option word is the command string
                        let mut j = k + 1;
                        while j < ws.len() && ws[j].starts_with('-') && ws[j] != "-" { j += 1; }
                        if j < ws.len() { payload = Some(ws[j].clone()); }
                        break;
                    }
                    if !t.starts_with('-') { c.via.push("script-file".into()); break; }
                    k += 1;
                }
                if let Some(p) = payload { c.via.push(format!("{prog} -c")); c.nested.extend(parse_depth(&p, depth + 1)); }
                break;
            }
            p if INTERPRETERS.contains(&p) => {
                let ws: Vec<String> = c.words.iter().map(|w| w.text.clone()).collect();
                let mut k = 1;
                while k < ws.len() {
                    let t = &ws[k];
                    if matches!(t.as_str(), "-c" | "-e" | "--eval" | "-p" | "--print" | "-r") || (p == "osascript" && t == "-e") {
                        if let Some(payload) = ws.get(k + 1) { c.script = Some((prog.clone(), payload.clone())); c.via.push(format!("{prog} -c")); c.nested.extend(extract_embedded_shell(payload, depth + 1)); }
                        break;
                    }
                    if !t.starts_with('-') { break; }
                    k += 1;
                }
                break;
            }
            _ => break,
        }
    }
}

/// Pull shell commands out of inline scripts: `os.system("rm -rf x")`, `subprocess.run("…", shell=True)`,
/// `execSync("…")`, `child_process.exec("…")`, backtick/system() calls in ruby/perl/php.
fn extract_embedded_shell(payload: &str, depth: usize) -> Vec<SimpleCommand> {
    let mut out = vec![];
    let lower = payload.to_lowercase();
    let markers = ["os.system(", "subprocess.run(", "subprocess.call(", "subprocess.popen(", "subprocess.check_output(", "subprocess.check_call(", "os.popen(", "execsync(", "exec(", "spawnsync(", "spawn(", "system(", "shell_exec(", "passthru(", "%x(", "deno.run(", "deno.command("];
    for m in markers {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(m) {
            let start = from + pos + m.len();
            // take the first quoted string after the marker
            let rest = &payload[start..];
            if let Some(q) = rest.find(|ch| ch == '"' || ch == '\'' || ch == '`') {
                let quote = rest.as_bytes()[q] as char;
                let body: String = rest[q + 1..].chars().take_while(|ch| *ch != quote).collect();
                if !body.trim().is_empty() { out.extend(parse_depth(&body, depth)); }
            }
            from = start;
            if out.len() > 32 { break; }
        }
    }
    // generic hints even without a quoted literal
    if out.is_empty() && (lower.contains("shutil.rmtree") || lower.contains("os.remove(") || lower.contains("fs.rm(") || lower.contains("rimraf") || lower.contains("fs.unlink")) {
        out.extend(parse_depth("rm -r <script-managed-path>", depth));
    }
    out
}

/// Depth-first flatten of a command tree.
pub fn flatten(cmds: &[SimpleCommand]) -> Vec<&SimpleCommand> {
    fn walk<'a>(c: &'a SimpleCommand, out: &mut Vec<&'a SimpleCommand>) { out.push(c); for n in &c.nested { walk(n, out); } }
    let mut out = vec![];
    for c in cmds { walk(c, &mut out); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progs(cmd: &str) -> Vec<String> { parse(cmd).iter().filter_map(|c| c.program()).collect() }

    #[test]
    fn splits_operators_and_pipelines() {
        let cs = parse("ls -la && cat x | grep y; echo 'a;b' || true & sleep 1");
        let p: Vec<_> = cs.iter().filter_map(|c| c.program()).collect();
        assert_eq!(p, vec!["ls", "cat", "grep", "echo", "true", "sleep"]);
        assert_eq!(cs[1].pipeline, cs[2].pipeline);
        assert_ne!(cs[0].pipeline, cs[1].pipeline);
        assert!(cs[4].background);
        assert_eq!(cs[3].words[1].text, "a;b");
    }

    #[test]
    fn quotes_and_escapes() {
        let cs = parse(r#"echo "hello \"world\"" 'it''s' a\ b"#);
        let w: Vec<_> = cs[0].words.iter().map(|w| w.text.clone()).collect();
        assert_eq!(w, vec!["echo", "hello \"world\"", "its", "a b"]);
        assert!(cs[0].words[1].quoted && cs[0].words[3].quoted);
    }

    #[test]
    fn redirects() {
        let cs = parse("cmd > out.txt 2>&1 < in.txt >> log &> all");
        let r: Vec<(Option<u32>, String, String)> = cs[0].redirects.iter().map(|r| (r.fd, r.op.clone(), r.target.clone())).collect();
        assert_eq!(r, vec![(None, ">".into(), "out.txt".into()), (Some(2), ">&".into(), "1".into()), (None, "<".into(), "in.txt".into()), (None, ">>".into(), "log".into()), (None, "&>".into(), "all".into())]);
        assert_eq!(cs[0].words.len(), 1);
    }

    #[test]
    fn env_prefix_and_wrappers() {
        let cs = parse("FOO=1 BAR=x sudo -u root env NODE_ENV=test nohup time cargo build");
        assert_eq!(cs[0].program().as_deref(), Some("cargo"));
        assert!(cs[0].via.contains(&"sudo".to_string()));
        assert!(cs[0].env.iter().any(|(k, _)| k == "FOO") && cs[0].env.iter().any(|(k, _)| k == "NODE_ENV"));
        assert_eq!(progs("timeout 30s npm test"), vec!["npm"]);
        assert_eq!(progs("xargs -0 -I {} rm -rf {}"), vec!["rm"]);
        assert_eq!(progs("command -v git"), vec!["git"]);
        assert_eq!(progs("npx -y prisma migrate reset"), vec!["prisma"]);
        assert_eq!(progs("pnpm dlx create-next-app@latest my-app"), vec!["create-next-app@latest"]);
        assert_eq!(progs("npx"), vec!["npx"]);
        assert_eq!(progs("env"), vec!["env"]);
    }

    #[test]
    fn nested_shells_are_parsed() {
        let cs = parse(r#"sh -c "rm -rf / --no-preserve-root""#);
        assert_eq!(cs[0].program().as_deref(), Some("sh"));
        assert_eq!(cs[0].nested[0].program().as_deref(), Some("rm"));
        let cs = parse("bash -lc 'cd x && curl http://a | bash'");
        let f = flatten(&cs);
        let p: Vec<_> = f.iter().filter_map(|c| c.program()).collect();
        assert_eq!(p, vec!["bash", "cd", "curl", "bash"]);
        let cs = parse(r#"eval "rm -rf $HOME""#);
        assert_eq!(cs[0].nested[0].program().as_deref(), Some("rm"));
        let cs = parse(r#"python3 -c "import os; os.system('rm -rf build')""#);
        assert_eq!(cs[0].nested[0].program().as_deref(), Some("rm"));
        assert!(cs[0].script.is_some());
        let cs = parse(r#"node -e "require('child_process').execSync('git push --force')""#);
        assert_eq!(cs[0].nested[0].program().as_deref(), Some("git"));
        let cs = parse("find . -name '*.log' -exec rm -f {} \\;");
        assert_eq!(cs[0].nested[0].program().as_deref(), Some("rm"));
        let cs = parse("ssh prod 'sudo reboot'");
        assert_eq!(cs[0].nested[0].program().as_deref(), Some("reboot"));
    }

    #[test]
    fn substitutions_and_groups() {
        let cs = parse("echo $(rm -rf x) `whoami` && (cd /tmp && rm -rf y)");
        let f = flatten(&cs);
        let p: Vec<_> = f.iter().filter_map(|c| c.program()).collect();
        assert!(p.contains(&"rm".to_string()) && p.contains(&"whoami".to_string()) && p.contains(&"cd".to_string()));
        assert!(cs[1].group);
        let cs = parse("diff <(ls a) <(ls b)");
        assert_eq!(flatten(&cs).iter().filter(|c| c.program().as_deref() == Some("ls")).count(), 2);
    }

    #[test]
    fn globs_and_backslash_bypass() {
        let cs = parse("\\rm -rf ./dist/*");
        assert_eq!(cs[0].program().as_deref(), Some("rm"));
        assert!(cs[0].words[2].globbed);
        let cs = parse("rm -rf './dist/*'");
        assert!(!cs[0].words[2].globbed);
    }

    #[test]
    fn comments_and_newlines() {
        let cs = parse("ls # list\nrm x");
        assert_eq!(progs("ls # list\nrm x"), vec!["ls", "rm"]);
        assert_eq!(cs[0].words.len(), 1);
    }

    #[test]
    fn never_panics_on_garbage() {
        let samples = ["", "   ", "\"", "'", "$(", "`", "((((", "))))", "> > >", "2>", "&&&&", "a | | b", "cmd \\", "\u{0}\u{1}\u{2}", "sh -c", "xargs", "env", "sudo", "find -exec", "$(((", "a=", "=b", "<(", "ls <("];
        for s in samples { let _ = parse(s); }
        // pseudo-random bytes
        let mut x: u64 = 0x9E3779B97F4A7C15;
        for _ in 0..2000 {
            let mut s = String::new();
            for _ in 0..40 { x ^= x << 13; x ^= x >> 7; x ^= x << 17; let b = (x % 128) as u8; s.push(if b < 32 { ' ' } else { b as char }); }
            let _ = flatten(&parse(&s));
        }
    }

    #[test]
    fn operands_and_flags() {
        let c = &parse("rm -rf -- -weird ./a")[0];
        assert_eq!(c.operands().iter().map(|w| w.text.clone()).collect::<Vec<_>>(), vec!["-weird", "./a"]);
        assert!(c.has_flag("-r") && c.has_flag("-f"));
        let c = &parse("git push --force origin main")[0];
        assert!(c.has_flag("--force"));
    }
}

//! Minimal YAML-frontmatter parser/renderer for rules (.mdc), skills, commands, subagents and plans.
//! Supports the flat subset these files use: `key: value`, quoted strings, booleans, numbers,
//! inline lists `[a, b]`, block lists (`- item`), block scalars (`|` / `>`), comments.
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum FmValue { Str(String), Bool(bool), Num(f64), List(Vec<String>), Null }

#[derive(Debug, Clone, Default)]
pub struct Frontmatter { pub map: BTreeMap<String, FmValue> }

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&FmValue> {
        self.map.get(key).or_else(|| {
            // tolerate kebab/camel variants: argument-hint ↔ argumentHint, always-apply ↔ alwaysApply
            let alt = if key.contains('-') { kebab_to_camel(key) } else { camel_to_kebab(key) };
            self.map.get(&alt)
        })
    }
    pub fn get_str(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            FmValue::Str(s) => Some(s.clone()),
            FmValue::Bool(b) => Some(b.to_string()),
            FmValue::Num(n) => Some(n.to_string()),
            FmValue::List(l) => Some(l.join(", ")),
            FmValue::Null => None,
        }
    }
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            FmValue::Bool(b) => Some(*b),
            FmValue::Str(s) => match s.trim().to_ascii_lowercase().as_str() { "true" | "yes" | "on" => Some(true), "false" | "no" | "off" => Some(false), _ => None },
            _ => None,
        }
    }
    /// Lists: inline/block lists as-is; strings are split on commas (Cursor's `globs: *.ts, *.tsx`).
    pub fn get_list(&self, key: &str) -> Vec<String> {
        match self.get(key) {
            Some(FmValue::List(l)) => l.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            Some(FmValue::Str(s)) => s.split(',').map(|x| x.trim().trim_matches(|c| c == '"' || c == '\'').to_string()).filter(|x| !x.is_empty()).collect(),
            _ => vec![],
        }
    }
    pub fn is_empty(&self) -> bool { self.map.is_empty() }
}

fn kebab_to_camel(s: &str) -> String {
    let mut out = String::new(); let mut up = false;
    for c in s.chars() { if c == '-' { up = true; } else if up { out.extend(c.to_uppercase()); up = false; } else { out.push(c); } }
    out
}
fn camel_to_kebab(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() { if c.is_ascii_uppercase() { out.push('-'); out.extend(c.to_lowercase()); } else { out.push(c); } }
    out
}

fn unquote(v: &str) -> String {
    let t = v.trim();
    if t.len() >= 2 && ((t.starts_with('"') && t.ends_with('"')) || (t.starts_with('\'') && t.ends_with('\''))) {
        let inner = &t[1..t.len() - 1];
        if t.starts_with('"') { inner.replace("\\\"", "\"").replace("\\n", "\n").replace("\\\\", "\\") } else { inner.replace("''", "'") }
    } else { t.to_string() }
}

fn scalar(v: &str) -> FmValue {
    let t = v.trim();
    if t.is_empty() || t == "~" || t == "null" { return FmValue::Null; }
    if t.starts_with('[') && t.ends_with(']') {
        let inner = &t[1..t.len() - 1];
        return FmValue::List(split_inline_list(inner).into_iter().map(|s| unquote(&s)).filter(|s| !s.is_empty()).collect());
    }
    if t.starts_with('"') || t.starts_with('\'') { return FmValue::Str(unquote(t)); }
    match t { "true" | "True" | "TRUE" | "yes" | "Yes" => return FmValue::Bool(true), "false" | "False" | "FALSE" | "no" | "No" => return FmValue::Bool(false), _ => {} }
    if let Ok(n) = t.parse::<f64>() { if !t.starts_with('+') && !t.contains('_') { return FmValue::Num(n); } }
    // strip trailing comment
    let mut s = t.to_string();
    if let Some(i) = s.find(" #") { s.truncate(i); }
    FmValue::Str(s.trim().to_string())
}

fn split_inline_list(s: &str) -> Vec<String> {
    let mut out = vec![]; let mut cur = String::new(); let mut quote: Option<char> = None;
    for c in s.chars() {
        match quote {
            Some(q) => { cur.push(c); if c == q { quote = None; } }
            None => match c { '"' | '\'' => { quote = Some(c); cur.push(c); } ',' => { out.push(cur.trim().to_string()); cur.clear(); } _ => cur.push(c) },
        }
    }
    if !cur.trim().is_empty() { out.push(cur.trim().to_string()); }
    out
}

/// Parse a document. Returns the frontmatter (None when the file has none or it is malformed) and the body.
pub fn parse(text: &str) -> (Option<Frontmatter>, &str) {
    let t = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = t.split_inclusive('\n');
    let first = match lines.next() { Some(l) => l, None => return (None, text) };
    if first.trim_end() != "---" { return (None, text); }
    let mut offset = first.len();
    let mut fm = Frontmatter::default();
    let mut pending_key: Option<String> = None; // key awaiting block list / block scalar
    let mut block: Option<(String, char, Vec<String>)> = None; // (key, style, lines)
    let mut closed = false;
    for line in lines {
        offset += line.len();
        let raw = line.trim_end_matches(['\n', '\r']);
        if raw.trim_end() == "---" || raw.trim_end() == "..." { closed = true; break; }
        if let Some((key, style, buf)) = block.as_mut() {
            if raw.starts_with(' ') || raw.starts_with('\t') || raw.trim().is_empty() { buf.push(raw.trim_start().to_string()); continue; }
            let joined = if *style == '|' { buf.join("\n") } else { buf.join(" ") };
            fm.map.insert(key.clone(), FmValue::Str(joined.trim().to_string()));
            block = None;
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') { continue; }
        if let Some(key) = pending_key.clone() {
            if let Some(item) = trimmed.strip_prefix("- ").or_else(|| if trimmed == "-" { Some("") } else { None }) {
                let entry = fm.map.entry(key).or_insert_with(|| FmValue::List(vec![]));
                if let FmValue::List(l) = entry { let v = unquote(item); if !v.is_empty() { l.push(v); } }
                continue;
            }
            // not a list → key had null value
            fm.map.entry(key).or_insert(FmValue::Null);
            pending_key = None;
        }
        if raw.starts_with(' ') || raw.starts_with('\t') { continue; } // nested maps unsupported; ignore
        if let Some((k, v)) = trimmed.split_once(':') {
            let key = k.trim().to_string();
            let val = v.trim();
            if val.is_empty() { pending_key = Some(key); continue; }
            if val == "|" || val == ">" || val == "|-" || val == ">-" { block = Some((key, val.chars().next().unwrap(), vec![])); continue; }
            fm.map.insert(key, scalar(val));
        }
    }
    if let Some(key) = pending_key { fm.map.entry(key).or_insert(FmValue::Null); }
    if let Some((key, style, buf)) = block { let joined = if style == '|' { buf.join("\n") } else { buf.join(" ") }; fm.map.insert(key, FmValue::Str(joined.trim().to_string())); }
    if !closed { return (None, text); }
    let body = &t[offset.min(t.len())..];
    let body = body.strip_prefix('\n').unwrap_or(body);
    (Some(fm), body)
}

fn needs_quotes(s: &str) -> bool {
    s.is_empty() || s.contains(": ") || s.contains(" #") || s.starts_with(['[', '{', '"', '\'', '*', '&', '!', '|', '>', '%', '@', '`', '-', '?']) || s.ends_with(':') || matches!(s, "true" | "false" | "yes" | "no" | "null" | "~") || s.parse::<f64>().is_ok() || s.contains('\n')
}

/// Render a frontmatter block. Lists render inline (`[a, b]`); globs for Cursor compatibility are rendered unquoted comma-separated when `cursor_globs` is used by the caller.
pub fn render(fields: &[(&str, FmValue)]) -> String {
    let mut out = String::from("---\n");
    for (k, v) in fields {
        match v {
            FmValue::Null => out.push_str(&format!("{k}:\n")),
            FmValue::Bool(b) => out.push_str(&format!("{k}: {b}\n")),
            FmValue::Num(n) => out.push_str(&format!("{k}: {n}\n")),
            FmValue::Str(s) => { if needs_quotes(s) { out.push_str(&format!("{k}: {}\n", serde_json::to_string(s).unwrap_or_default())); } else { out.push_str(&format!("{k}: {s}\n")); } }
            FmValue::List(l) => { if l.is_empty() { out.push_str(&format!("{k}: []\n")); } else { out.push_str(&format!("{k}: [{}]\n", l.iter().map(|s| if needs_quotes(s) || s.contains(',') { serde_json::to_string(s).unwrap_or_default() } else { s.clone() }).collect::<Vec<_>>().join(", "))); } }
        }
    }
    out.push_str("---\n");
    out
}

/// First markdown heading (`# Title`) in a body, if any.
pub fn first_heading(body: &str) -> Option<String> {
    body.lines().find_map(|l| l.trim().strip_prefix('#').map(|h| h.trim_start_matches('#').trim().to_string())).filter(|s| !s.is_empty())
}

pub fn slugify(s: &str) -> String {
    let mut out = String::new(); let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() { out.push(c.to_ascii_lowercase()); dash = false; }
        else if !dash && !out.is_empty() { out.push('-'); dash = true; }
    }
    let t = out.trim_end_matches('-').to_string();
    if t.is_empty() { "untitled".into() } else { t.chars().take(64).collect::<String>().trim_end_matches('-').to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_cursor_style_rule_frontmatter() {
        let doc = "---\ndescription: Use the repo conventions\nglobs: *.ts, src/**/*.tsx\nalwaysApply: false\n---\n# Body\nhello\n";
        let (fm, body) = parse(doc);
        let fm = fm.unwrap();
        assert_eq!(fm.get_str("description").as_deref(), Some("Use the repo conventions"));
        assert_eq!(fm.get_list("globs"), vec!["*.ts", "src/**/*.tsx"]);
        assert_eq!(fm.get_bool("alwaysApply"), Some(false));
        assert_eq!(body, "# Body\nhello\n");
    }
    #[test]
    fn parses_lists_quotes_blocks_and_variants() {
        let doc = "---\nname: \"My: Skill\"\ntags:\n  - a\n  - 'b c'\ninline: [x, \"y,z\"]\nargument-hint: <file>\nreadonly: yes\ndesc: |\n  line one\n  line two\n---\nbody";
        let (fm, body) = parse(doc);
        let fm = fm.unwrap();
        assert_eq!(fm.get_str("name").as_deref(), Some("My: Skill"));
        assert_eq!(fm.get_list("tags"), vec!["a", "b c"]);
        assert_eq!(fm.get_list("inline"), vec!["x", "y,z"]);
        assert_eq!(fm.get_str("argumentHint").as_deref(), Some("<file>"));
        assert_eq!(fm.get_bool("readonly"), Some(true));
        assert_eq!(fm.get_str("desc").as_deref(), Some("line one\nline two"));
        assert_eq!(body, "body");
    }
    #[test]
    fn no_or_unclosed_frontmatter_returns_whole_text() {
        assert!(parse("# just markdown").0.is_none());
        let (fm, body) = parse("---\nkey: v\nno closing");
        assert!(fm.is_none()); assert!(body.starts_with("---"));
    }
    #[test]
    fn render_roundtrips() {
        let s = render(&[("title", FmValue::Str("A: plan".into())), ("status", FmValue::Str("draft".into())), ("globs", FmValue::List(vec!["*.ts".into()])), ("alwaysApply", FmValue::Bool(true))]);
        let (fm, _) = parse(&format!("{s}body"));
        let fm = fm.unwrap();
        assert_eq!(fm.get_str("title").as_deref(), Some("A: plan"));
        assert_eq!(fm.get_list("globs"), vec!["*.ts"]);
        assert_eq!(fm.get_bool("alwaysApply"), Some(true));
        assert_eq!(slugify("Hello, World! Plan #2"), "hello-world-plan-2");
    }
}

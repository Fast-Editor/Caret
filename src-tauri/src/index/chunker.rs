//! Language-aware heuristic chunking. Splits on top-level definition boundaries per language,
//! falls back to overlapping line windows, and caps chunk size.
use once_cell::sync::Lazy;
use regex::Regex;

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// 1-based inclusive line range in the source file.
    pub start_line: u32,
    pub end_line: u32,
    /// Indexed text: breadcrumb header line + body.
    pub text: String,
    pub name: Option<String>,
    pub kind: Option<String>,
}

pub const MAX_CHUNK_CHARS: usize = 2000;

struct LangRules { exts: &'static [&'static str], boundaries: Vec<(Regex, &'static str)>, top_level_only: bool }

fn rx(s: &str) -> Regex { Regex::new(s).expect("valid chunker regex") }

static RULES: Lazy<Vec<LangRules>> = Lazy::new(|| vec![
    LangRules { exts: &["rs"], top_level_only: true, boundaries: vec![
        (rx(r"^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+\S+\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)"), "function"),
        (rx(r"^(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|union)\s+([A-Za-z_][A-Za-z0-9_]*)"), "struct"),
        (rx(r"^(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)"), "trait"),
        (rx(r"^(?:unsafe\s+)?impl(?:<[^>]*>)?\s+(?:[A-Za-z_][A-Za-z0-9_:<>, ]*\s+for\s+)?([A-Za-z_][A-Za-z0-9_]*)"), "impl"),
        (rx(r"^(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)"), "module"),
        (rx(r"^(?:pub(?:\([^)]*\))?\s+)?(?:type|const|static)\s+([A-Za-z_][A-Za-z0-9_]*)"), "const"),
        (rx(r"^#\[cfg\(test\)\]"), "tests"),
    ]},
    LangRules { exts: &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"], top_level_only: true, boundaries: vec![
        (rx(r"^(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s*([A-Za-z_$][A-Za-z0-9_$]*)"), "function"),
        (rx(r"^(?:export\s+)?(?:default\s+)?(?:abstract\s+)?class\s+([A-Za-z_$][A-Za-z0-9_$]*)"), "class"),
        (rx(r"^(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*(?::[^=]+)?=\s*(?:async\s*)?(?:\([^)]*\)|[A-Za-z_$][A-Za-z0-9_$]*)\s*(?::\s*[^=]+)?=>"), "function"),
        (rx(r"^(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*(?::[^=]+)?=\s*(?:function|React\.|create|define|new\s)"), "variable"),
        (rx(r"^(?:export\s+)?(?:interface|type|enum)\s+([A-Za-z_$][A-Za-z0-9_$]*)"), "type"),
        (rx(r"^(?:export\s+)?(?:declare\s+)?(?:namespace|module)\s+([A-Za-z_$][A-Za-z0-9_$.]*)"), "module"),
        (rx(r"^(?:describe|it|test)\s*\(\s*['\x22`]([^'\x22`]+)"), "test"),
    ]},
    LangRules { exts: &["py", "pyi"], top_level_only: true, boundaries: vec![
        (rx(r"^(?:async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)"), "function"),
        (rx(r"^class\s+([A-Za-z_][A-Za-z0-9_]*)"), "class"),
        (rx(r"^@([A-Za-z_][A-Za-z0-9_.]*)"), "decorator"),
    ]},
    LangRules { exts: &["go"], top_level_only: true, boundaries: vec![
        (rx(r"^func\s+(?:\([^)]*\)\s*)?([A-Za-z_][A-Za-z0-9_]*)"), "function"),
        (rx(r"^type\s+([A-Za-z_][A-Za-z0-9_]*)"), "type"),
        (rx(r"^(?:var|const)\s+(?:\(|([A-Za-z_][A-Za-z0-9_]*))"), "variable"),
    ]},
    LangRules { exts: &["java", "kt", "kts", "scala", "cs", "swift", "dart"], top_level_only: false, boundaries: vec![
        (rx(r"^\s*(?:(?:public|private|protected|internal|static|final|abstract|sealed|open|data|override|async|partial|suspend|fun|export)\s+)*(?:class|interface|enum|record|struct|object|protocol|extension|trait)\s+([A-Za-z_][A-Za-z0-9_]*)"), "class"),
        (rx(r"^\s{0,8}(?:(?:public|private|protected|internal|static|final|abstract|override|async|synchronized|suspend|virtual|inline)\s+)*(?:fun\s+|func\s+)?(?:[A-Za-z_][A-Za-z0-9_<>\[\],?. ]*\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*\([^;]*$"), "method"),
    ]},
    LangRules { exts: &["c", "h", "cc", "cpp", "cxx", "hpp", "hh", "m", "mm"], top_level_only: true, boundaries: vec![
        (rx(r"^(?:[A-Za-z_][A-Za-z0-9_:<>,*& ]*\s+)?\**([A-Za-z_~][A-Za-z0-9_:]*)\s*\([^;]*\)?\s*(?:const)?\s*\{?\s*$"), "function"),
        (rx(r"^(?:typedef\s+)?(?:struct|class|enum|union|namespace)\s+([A-Za-z_][A-Za-z0-9_]*)"), "struct"),
        (rx(r"^#define\s+([A-Za-z_][A-Za-z0-9_]*)"), "macro"),
    ]},
    LangRules { exts: &["rb"], top_level_only: false, boundaries: vec![
        (rx(r"^\s*def\s+(?:self\.)?([A-Za-z_][A-Za-z0-9_?!=]*)"), "method"),
        (rx(r"^\s*(?:class|module)\s+([A-Za-z_][A-Za-z0-9_:]*)"), "class"),
    ]},
    LangRules { exts: &["php"], top_level_only: false, boundaries: vec![
        (rx(r"^\s*(?:(?:public|private|protected|static|abstract|final)\s+)*function\s+&?([A-Za-z_][A-Za-z0-9_]*)"), "function"),
        (rx(r"^\s*(?:abstract\s+|final\s+)?(?:class|interface|trait|enum)\s+([A-Za-z_][A-Za-z0-9_]*)"), "class"),
    ]},
    LangRules { exts: &["md", "mdx", "markdown"], top_level_only: true, boundaries: vec![
        (rx(r"^#{1,3}\s+(.+?)\s*#*\s*$"), "heading"),
    ]},
    LangRules { exts: &["sh", "bash", "zsh"], top_level_only: true, boundaries: vec![
        (rx(r"^(?:function\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*\(\)\s*\{?"), "function"),
    ]},
    LangRules { exts: &["lua"], top_level_only: true, boundaries: vec![
        (rx(r"^(?:local\s+)?function\s+([A-Za-z_][A-Za-z0-9_.:]*)"), "function"),
    ]},
]);

fn rules_for(path: &str) -> Option<&'static LangRules> {
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    RULES.iter().find(|r| r.exts.contains(&ext.as_str()))
}

struct Boundary { line: usize, name: String, kind: &'static str }

fn find_boundaries(rules: &LangRules, lines: &[&str]) -> Vec<Boundary> {
    let mut out = vec![];
    for (i, line) in lines.iter().enumerate() {
        if line.trim().is_empty() { continue; }
        if rules.top_level_only && (line.starts_with(' ') || line.starts_with('\t')) { continue; }
        for (re, kind) in &rules.boundaries {
            if let Some(c) = re.captures(line) {
                let name = c.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_else(|| kind.to_string());
                // decorators attach to the next definition: treat as boundary only if the next def isn't already one
                out.push(Boundary { line: i, name, kind });
                break;
            }
        }
    }
    // Merge decorator/attribute lines into the following definition.
    let mut merged: Vec<Boundary> = Vec::with_capacity(out.len());
    for b in out {
        if let Some(prev) = merged.last_mut() {
            if (prev.kind == "decorator" || prev.kind == "tests") && b.line > prev.line && b.line - prev.line <= 3 {
                prev.name = b.name; prev.kind = if prev.kind == "tests" { "tests" } else { b.kind };
                continue;
            }
        }
        merged.push(b);
    }
    merged
}

/// Split `[start, end)` into windows of `max_lines` with `overlap`; also enforce `MAX_CHUNK_CHARS`.
fn windows(lines: &[&str], start: usize, end: usize, max_lines: usize, overlap: usize, out: &mut Vec<(usize, usize)>) {
    let max_lines = max_lines.max(5);
    let overlap = overlap.min(max_lines / 2);
    let mut s = start;
    while s < end {
        let mut e = (s + max_lines).min(end);
        // shrink if too many chars
        let mut chars: usize = lines[s..e].iter().map(|l| l.len() + 1).sum();
        while chars > MAX_CHUNK_CHARS && e - s > 1 {
            e -= 1;
            chars = lines[s..e].iter().map(|l| l.len() + 1).sum();
        }
        // a single enormous line: hard split by chars is handled at emit time
        out.push((s, e));
        if e >= end { break; }
        let next = if e - s > overlap { e - overlap } else { e };
        s = if next <= s { s + 1 } else { next };
    }
}

pub fn chunk_file(rel_path: &str, content: &str, max_lines: usize, overlap: usize) -> Vec<Chunk> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() { return vec![]; }
    let mut ranges: Vec<(usize, usize, Option<String>, Option<String>)> = vec![];
    match rules_for(rel_path) {
        Some(rules) => {
            let bounds = find_boundaries(rules, &lines);
            if bounds.is_empty() {
                let mut w = vec![]; windows(&lines, 0, lines.len(), max_lines, overlap, &mut w);
                ranges.extend(w.into_iter().map(|(s, e)| (s, e, None, None)));
            } else {
                // preamble before first boundary
                if bounds[0].line > 0 {
                    let mut w = vec![]; windows(&lines, 0, bounds[0].line, max_lines, overlap, &mut w);
                    ranges.extend(w.into_iter().map(|(s, e)| (s, e, None, Some("preamble".to_string()))));
                }
                for (i, b) in bounds.iter().enumerate() {
                    let end = bounds.get(i + 1).map(|n| n.line).unwrap_or(lines.len());
                    let mut w = vec![]; windows(&lines, b.line, end, max_lines, overlap, &mut w);
                    for (j, (s, e)) in w.into_iter().enumerate() {
                        let name = if j == 0 { b.name.clone() } else { format!("{} (cont. {})", b.name, j + 1) };
                        ranges.push((s, e, Some(name), Some(b.kind.to_string())));
                    }
                }
            }
        }
        None => {
            let mut w = vec![]; windows(&lines, 0, lines.len(), max_lines, overlap, &mut w);
            ranges.extend(w.into_iter().map(|(s, e)| (s, e, None, None)));
        }
    }
    // merge tiny trailing fragments into previous chunk when they share no name
    let mut merged: Vec<(usize, usize, Option<String>, Option<String>)> = Vec::with_capacity(ranges.len());
    for r in ranges {
        if let Some(prev) = merged.last_mut() {
            let prev_len = prev.1 - prev.0; let cur_len = r.1 - r.0;
            if cur_len < 4 && r.2.is_none() && prev_len + cur_len <= max_lines.max(5) && prev.1 == r.0 { prev.1 = r.1; continue; }
        }
        merged.push(r);
    }
    merged.into_iter().filter(|(s, e, _, _)| e > s).map(|(s, e, name, kind)| {
        let mut body = lines[s..e].join("\n");
        if body.len() > MAX_CHUNK_CHARS * 2 { // pathological single long line(s)
            let cut = body.char_indices().nth(MAX_CHUNK_CHARS * 2).map(|(i, _)| i).unwrap_or(body.len());
            body.truncate(cut);
        }
        let header = match &name { Some(n) => format!("{rel_path} › {n}"), None => rel_path.to_string() };
        Chunk { start_line: (s + 1) as u32, end_line: e as u32, text: format!("{header}\n{body}"), name, kind }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_boundaries() {
        let src = "use std::io;\n\npub struct Foo {\n    a: u32,\n}\n\nimpl Foo {\n    pub fn new() -> Self { Foo { a: 1 } }\n}\n\nfn helper() {}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() {}\n}\n";
        let chunks = chunk_file("src/lib.rs", src, 60, 5);
        let names: Vec<_> = chunks.iter().map(|c| c.name.clone().unwrap_or_default()).collect();
        assert_eq!(names, vec!["", "Foo", "Foo", "helper", "tests"]);
        assert_eq!(chunks[1].start_line, 3);
        assert!(chunks[1].text.starts_with("src/lib.rs › Foo\npub struct Foo"));
        assert_eq!(chunks.last().unwrap().kind.as_deref(), Some("tests"));
    }

    #[test]
    fn typescript_and_python_boundaries() {
        let ts = "import x from 'y'\n\nexport const add = (a: number, b: number) => a + b\n\nexport default class Store {\n  get() {}\n}\n\nfunction util() {}\n";
        let c = chunk_file("a.ts", ts, 60, 5);
        let names: Vec<_> = c.iter().filter_map(|c| c.name.clone()).collect();
        assert_eq!(names, vec!["add", "Store", "util"]);
        let py = "import os\n\n@dataclass\nclass A:\n    x: int\n\n    def m(self):\n        pass\n\ndef f():\n    return 1\n";
        let c = chunk_file("m.py", py, 60, 5);
        let names: Vec<_> = c.iter().filter_map(|c| c.name.clone()).collect();
        assert_eq!(names, vec!["A", "f"]);
        assert_eq!(c[1].start_line, 3); // decorator attached
    }

    #[test]
    fn windows_fallback_and_overlap() {
        let big: String = (1..=130).map(|i| format!("line {i}\n")).collect();
        let c = chunk_file("notes.txt", &big, 50, 10);
        assert_eq!(c.len(), 3);
        assert_eq!((c[0].start_line, c[0].end_line), (1, 50));
        assert_eq!((c[1].start_line, c[1].end_line), (41, 90));
        assert_eq!((c[2].start_line, c[2].end_line), (81, 130));
    }

    #[test]
    fn caps_chunk_chars() {
        let long_lines: String = (0..40).map(|i| format!("{} {}\n", i, "x".repeat(300))).collect();
        let c = chunk_file("data.txt", &long_lines, 60, 5);
        assert!(c.len() >= 6);
        assert!(c.iter().all(|c| c.text.len() <= MAX_CHUNK_CHARS + 64));
    }

    #[test]
    fn markdown_headings() {
        let md = "# Title\nintro\n\n## Install\nrun it\n\n## Usage\nuse it\n";
        let c = chunk_file("README.md", md, 60, 5);
        let names: Vec<_> = c.iter().filter_map(|c| c.name.clone()).collect();
        assert_eq!(names, vec!["Title", "Install", "Usage"]);
    }
}

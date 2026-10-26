//! Pure helpers for `edit_file`: locating `old_string` (exact → whitespace-normalized → fuzzy unique) and diffs.
use similar::{ChangeTag, TextDiff};

#[derive(Debug, PartialEq)]
pub enum MatchResult { Found { start: usize, end: usize, method: &'static str }, NotFound, Ambiguous(usize) }

fn find_all(hay: &str, needle: &str) -> Vec<usize> {
    let mut v = vec![]; let mut from = 0;
    while let Some(i) = hay[from..].find(needle) { v.push(from + i); from += i + needle.len().max(1); if v.len() > 50 { break; } }
    v
}

/// Normalize for whitespace-insensitive matching: trim each line's trailing ws, collapse runs of spaces/tabs inside lines.
fn norm_line(l: &str) -> String {
    let mut out = String::with_capacity(l.len()); let mut ws = false;
    for c in l.trim_end().chars() { if c == ' ' || c == '\t' { if !ws { out.push(' '); ws = true; } } else { out.push(c); ws = false; } }
    out
}

/// Map a line-based match back to byte offsets in the original content.
fn line_span(content: &str, start_line: usize, line_count: usize) -> (usize, usize) {
    let mut offsets = vec![0usize];
    for (i, b) in content.bytes().enumerate() { if b == b'\n' { offsets.push(i + 1); } }
    let start = offsets.get(start_line).copied().unwrap_or(content.len());
    let end_line = start_line + line_count;
    let end = if end_line < offsets.len() { offsets[end_line].saturating_sub(1).max(start) } else { content.len() };
    // include trailing newline when the original block had one and we're not at EOF
    (start, end.min(content.len()))
}

pub fn locate(content: &str, old: &str) -> MatchResult {
    if old.is_empty() { return MatchResult::NotFound; }
    let exact = find_all(content, old);
    match exact.len() { 1 => return MatchResult::Found { start: exact[0], end: exact[0] + old.len(), method: "exact" }, n if n > 1 => return MatchResult::Ambiguous(n), _ => {} }
    // Line-trimmed matching
    let c_lines: Vec<&str> = content.split('\n').collect();
    let o_lines: Vec<&str> = old.trim_matches('\n').split('\n').collect();
    if o_lines.is_empty() { return MatchResult::NotFound; }
    let o_norm: Vec<String> = o_lines.iter().map(|l| norm_line(l).trim().to_string()).collect();
    let c_norm: Vec<String> = c_lines.iter().map(|l| norm_line(l).trim().to_string()).collect();
    let mut hits = vec![];
    if o_norm.len() <= c_norm.len() {
        for i in 0..=(c_norm.len() - o_norm.len()) { if c_norm[i..i + o_norm.len()] == o_norm[..] { hits.push(i); if hits.len() > 50 { break; } } }
    }
    match hits.len() {
        1 => { let (s, e) = line_span(content, hits[0], o_lines.len()); return MatchResult::Found { start: s, end: e, method: "whitespace" }; }
        n if n > 1 => return MatchResult::Ambiguous(n),
        _ => {}
    }
    // Fuzzy: best window by normalized Levenshtein-ish similarity (line-level), accept if ≥ 0.9 and unique.
    if o_norm.len() >= 2 && o_norm.len() <= c_norm.len() {
        let mut best: Vec<(f32, usize)> = vec![];
        for i in 0..=(c_norm.len() - o_norm.len()) {
            let window = &c_norm[i..i + o_norm.len()];
            let same = window.iter().zip(o_norm.iter()).filter(|(a, b)| a == b).count();
            let sim = same as f32 / o_norm.len() as f32;
            if sim >= 0.85 { best.push((sim, i)); }
        }
        best.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(&(top, i)) = best.first() {
            let ties = best.iter().filter(|(s, _)| (*s - top).abs() < 1e-6).count();
            if ties == 1 && (top >= 0.9 || (o_norm.len() >= 6 && top >= 0.85)) { let (s, e) = line_span(content, i, o_lines.len()); return MatchResult::Found { start: s, end: e, method: "fuzzy" }; }
            if ties > 1 { return MatchResult::Ambiguous(ties); }
        }
    }
    MatchResult::NotFound
}

/// Apply a located replacement, preserving the indentation style of the matched block for whitespace/fuzzy matches.
pub fn apply(content: &str, start: usize, end: usize, new: &str) -> String {
    let mut out = String::with_capacity(content.len() + new.len());
    out.push_str(&content[..start]);
    out.push_str(new);
    out.push_str(&content[end..]);
    out
}

pub fn unified_diff(path: &str, before: &str, after: &str) -> (String, u32, u32) {
    let diff = TextDiff::from_lines(before, after);
    let (mut add, mut del) = (0u32, 0u32);
    for c in diff.iter_all_changes() { match c.tag() { ChangeTag::Insert => add += 1, ChangeTag::Delete => del += 1, _ => {} } }
    let text = diff.unified_diff().context_radius(3).header(&format!("a/{path}"), &format!("b/{path}")).to_string();
    (text, add, del)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_match() {
        let c = "fn a() {}\nfn b() {}\n";
        assert_eq!(locate(c, "fn b() {}"), MatchResult::Found { start: 10, end: 19, method: "exact" });
    }
    #[test]
    fn ambiguous_detected() {
        let c = "x = 1\ny = 2\nx = 1\n";
        assert_eq!(locate(c, "x = 1"), MatchResult::Ambiguous(2));
    }
    #[test]
    fn whitespace_insensitive() {
        let c = "    if (a) {\n        return 1;\n    }\n";
        match locate(c, "if (a) {\n  return 1;\n}") { MatchResult::Found { method, start, end } => { assert_eq!(method, "whitespace"); assert_eq!(&c[start..end], "    if (a) {\n        return 1;\n    }"); } r => panic!("{r:?}") }
    }
    #[test]
    fn fuzzy_unique() {
        let c = "a\nb\nc\nd\ne\nf\ng\nh\n";
        match locate(c, "a\nb\nc\nX\ne\nf\ng") { MatchResult::Found { method, .. } => assert_eq!(method, "fuzzy"), r => panic!("{r:?}") }
    }
    #[test]
    fn not_found() { assert_eq!(locate("hello\nworld\n", "nothing here"), MatchResult::NotFound); }
    #[test]
    fn diff_counts() {
        let (d, a, r) = unified_diff("x.rs", "a\nb\n", "a\nc\nd\n");
        assert_eq!((a, r), (2, 1)); assert!(d.contains("+c"));
    }
}

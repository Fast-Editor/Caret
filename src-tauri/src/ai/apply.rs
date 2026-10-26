//! Apply a chat code block to a file: heuristics first, then model-assisted (full rewrite or SEARCH/REPLACE).
use crate::ai::llm::{self, LlmRequest, Message, SystemBlock};
use crate::ai::util::{extract_tag, pick_model, strip_fences, workspace_root};
use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use serde::{Deserialize, Serialize};
use similar::TextDiff;
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest { pub path: String, pub code: String, #[serde(default)] pub language: Option<String>, #[serde(default)] pub instructions: Option<String>, #[serde(default)] pub session_id: Option<String>, #[serde(default)] pub model_id: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult { pub path: String, pub before: String, pub after: String, pub diff: String, pub method: String, #[serde(skip_serializing_if = "Option::is_none")] pub change_id: Option<String> }

pub fn unified_diff(path: &str, before: &str, after: &str) -> String {
    TextDiff::from_lines(before, after).unified_diff().context_radius(3).header(&format!("a/{path}"), &format!("b/{path}")).to_string()
}

fn is_ellipsis_marker(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() { return false; }
    let stripped = t.trim_start_matches(['/', '#', '-', '*', '<', '!', ';', '%']).trim_start().trim_end_matches(['*', '/', '>', '-']).trim();
    let s = stripped.to_ascii_lowercase();
    (s.starts_with("...") || s.starts_with("…")) && s.len() < 60 || s == "existing code" || s.contains("rest of the code") || s.contains("rest of file") || s.contains("existing code ...") || s.contains("existing code…")
}

fn norm(l: &str) -> String { l.split_whitespace().collect::<Vec<_>>().join(" ") }

/// Find `needle` lines inside `hay` lines starting at `from`; exact → whitespace-insensitive → fuzzy (ratio ≥ 0.9).
fn find_block(hay: &[&str], needle: &[&str], from: usize) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() { return None; }
    let n = needle.len();
    for i in from..=hay.len() - n { if hay[i..i + n] == *needle { return Some(i); } }
    let nn: Vec<String> = needle.iter().map(|l| norm(l)).collect();
    for i in from..=hay.len() - n { if (0..n).all(|k| norm(hay[i + k]) == nn[k]) { return Some(i); } }
    let needle_text = needle.join("\n");
    let mut best: Option<(usize, f32)> = None;
    for i in from..=hay.len() - n {
        let window = hay[i..i + n].join("\n");
        let r = TextDiff::from_chars(&window, &needle_text).ratio();
        if r >= 0.9 && best.map(|(_, br)| r > br).unwrap_or(true) { best = Some((i, r)); }
    }
    best.map(|(i, _)| i)
}

/// Apply `<<<<<<< SEARCH … ======= … >>>>>>> REPLACE` blocks. Returns the new text and number applied.
pub fn apply_search_replace(before: &str, blocks_text: &str) -> Result<(String, usize)> {
    let mut pairs: Vec<(String, String)> = vec![];
    let mut lines = blocks_text.lines().peekable();
    while let Some(l) = lines.next() {
        if l.trim_start().starts_with("<<<<<<< SEARCH") {
            let mut search = vec![]; let mut replace = vec![]; let mut in_replace = false; let mut closed = false;
            for l2 in lines.by_ref() {
                if l2.trim_start().starts_with("=======") && !in_replace { in_replace = true; continue; }
                if l2.trim_start().starts_with(">>>>>>> REPLACE") { closed = true; break; }
                if in_replace { replace.push(l2.to_string()); } else { search.push(l2.to_string()); }
            }
            if closed { pairs.push((search.join("\n"), replace.join("\n"))); }
        }
    }
    if pairs.is_empty() { return Err(CaretError::invalid("no SEARCH/REPLACE blocks found")); }
    let mut text = before.to_string();
    let mut applied = 0usize;
    for (search, replace) in pairs {
        let hay: Vec<&str> = text.lines().collect();
        let needle: Vec<&str> = search.lines().collect();
        if needle.is_empty() {
            // empty search = append
            if !text.ends_with('\n') && !text.is_empty() { text.push('\n'); }
            text.push_str(&replace); applied += 1; continue;
        }
        let Some(pos) = find_block(&hay, &needle, 0) else { return Err(CaretError::invalid(format!("SEARCH block not found:\n{}", &search[..search.len().min(200)]))) };
        let mut out: Vec<String> = hay[..pos].iter().map(|s| s.to_string()).collect();
        // Re-indent replacement if the matched block was indented differently than the search text.
        let matched_indent = hay[pos].len() - hay[pos].trim_start().len();
        let search_indent = needle[0].len() - needle[0].trim_start().len();
        let rep_lines: Vec<&str> = replace.lines().collect();
        if matched_indent != search_indent && !rep_lines.is_empty() {
            let delta = matched_indent as isize - search_indent as isize;
            for rl in &rep_lines { if rl.trim().is_empty() { out.push(String::new()); continue; } let cur = rl.len() - rl.trim_start().len(); let new = (cur as isize + delta).max(0) as usize; out.push(format!("{}{}", " ".repeat(new), rl.trim_start())); }
        } else { out.extend(rep_lines.iter().map(|s| s.to_string())); }
        out.extend(hay[pos + needle.len()..].iter().map(|s| s.to_string()));
        let trailing = text.ends_with('\n');
        text = out.join("\n"); if trailing { text.push('\n'); }
        applied += 1;
    }
    Ok((text, applied))
}

/// Heuristic merge of a block containing ellipsis markers ("// ... existing code ...") into `before`.
pub fn merge_with_markers(before: &str, block: &str) -> Option<String> {
    let bl: Vec<&str> = block.lines().collect();
    if !bl.iter().any(|l| is_ellipsis_marker(l)) { return None; }
    // Split into segments separated by marker lines.
    let mut segments: Vec<Vec<&str>> = vec![vec![]];
    for l in &bl { if is_ellipsis_marker(l) { segments.push(vec![]); } else { segments.last_mut().unwrap().push(l); } }
    let segments: Vec<Vec<&str>> = segments.into_iter().map(|s| { let mut s = s; while s.first().map(|l| l.trim().is_empty()).unwrap_or(false) { s.remove(0); } while s.last().map(|l| l.trim().is_empty()).unwrap_or(false) { s.pop(); } s }).filter(|s| !s.is_empty()).collect();
    if segments.is_empty() { return None; }
    let hay: Vec<&str> = before.lines().collect();
    let mut out: Vec<String> = vec![];
    let mut cursor = 0usize;
    for seg in &segments {
        // Anchor on first and last lines of the segment; replace the span between them.
        let first = seg[0]; let last = *seg.last().unwrap();
        let start = find_block(&hay, &[first], cursor)?;
        let end_search_from = start + 1;
        let end = if seg.len() == 1 { start } else { find_block(&hay, &[last], end_search_from.min(hay.len()))? };
        if end < start { return None; }
        out.extend(hay[cursor..start].iter().map(|s| s.to_string()));
        out.extend(seg.iter().map(|s| s.to_string()));
        cursor = end + 1;
    }
    out.extend(hay[cursor.min(hay.len())..].iter().map(|s| s.to_string()));
    let mut text = out.join("\n"); if before.ends_with('\n') { text.push('\n'); }
    Some(text)
}

/// Heuristic replace of a contiguous region whose first and last lines match the block (unique first-line anchor).
pub fn replace_anchored_region(before: &str, block: &str) -> Option<String> {
    let bl: Vec<&str> = block.lines().filter(|l| !l.trim().is_empty() || true).collect();
    let non_empty: Vec<&str> = bl.iter().copied().filter(|l| !l.trim().is_empty()).collect();
    if non_empty.len() < 2 { return None; }
    let first = non_empty[0]; let last = *non_empty.last().unwrap();
    let hay: Vec<&str> = before.lines().collect();
    let firsts: Vec<usize> = hay.iter().enumerate().filter(|(_, l)| norm(l) == norm(first)).map(|(i, _)| i).collect();
    if firsts.len() != 1 { return None; }
    let start = firsts[0];
    let mut best: Option<(usize, f32)> = None;
    for (i, l) in hay.iter().enumerate().skip(start + 1) {
        if norm(l) == norm(last) {
            let window = hay[start..=i].join("\n");
            let r = TextDiff::from_lines(&window, block).ratio();
            if best.map(|(_, br)| r > br).unwrap_or(true) { best = Some((i, r)); }
            if i - start > bl.len() * 3 + 50 { break; }
        }
    }
    let (end, ratio) = best?;
    if ratio < 0.5 { return None; }
    let mut out: Vec<&str> = hay[..start].to_vec();
    out.extend(bl.iter());
    out.extend(hay[end + 1..].iter());
    let mut text = out.join("\n"); if before.ends_with('\n') { text.push('\n'); }
    Some(text)
}

const REWRITE_SYSTEM: &str = "You merge a proposed code snippet into an existing file. Output the COMPLETE updated file inside <file></file> — no explanations, no fences. Integrate the snippet where it belongs (replace the corresponding existing code, add new code in the right place), preserve everything unrelated exactly, and keep formatting consistent. Treat markers like '// ... existing code ...' as 'keep the existing code here'.";
const SR_SYSTEM: &str = "You merge a proposed code snippet into an existing file by emitting edit blocks. Output one or more blocks in exactly this format and nothing else:\n<<<<<<< SEARCH\n<exact existing lines to replace>\n=======\n<replacement lines>\n>>>>>>> REPLACE\nThe SEARCH text must match the file verbatim (copy it exactly, including indentation), be minimal but unique, and may be empty only to append at the end. Treat markers like '// ... existing code ...' as 'keep the existing code here'.";

pub async fn apply(app: &AppHandle, req: ApplyRequest) -> Result<ApplyResult> {
    let root = workspace_root(app);
    let abs = crate::paths::resolve(root.as_deref(), &req.path);
    if let Some(r) = &root { if !crate::paths::normalize(&abs).starts_with(crate::paths::normalize(r)) { return Err(CaretError::denied("apply target is outside the workspace")); } }
    let code = strip_fences(&req.code);
    let rel = root.as_deref().map(|r| crate::paths::rel_to(r, &abs)).unwrap_or_else(|| req.path.clone());
    if !abs.exists() {
        let after = if code.ends_with('\n') { code.clone() } else { format!("{code}\n") };
        return Ok(ApplyResult { path: abs.to_string_lossy().to_string(), before: String::new(), diff: unified_diff(&rel, "", &after), after, method: "create".into(), change_id: None });
    }
    let before = std::fs::read_to_string(&abs).map_err(|e| CaretError::io(format!("cannot read {}: {e}", abs.display())))?;
    let finish = |after: String, method: &str| ApplyResult { path: abs.to_string_lossy().to_string(), diff: unified_diff(&rel, &before, &after), before: before.clone(), after, method: method.into(), change_id: None };

    // 1. identical / contiguous region already present → no-op
    if norm(&before) == norm(&code) { return Ok(finish(before.clone(), "heuristic")); }
    { let hay: Vec<&str> = before.lines().collect(); let needle: Vec<&str> = code.lines().collect(); if !needle.is_empty() && find_block(&hay, &needle, 0).is_some() { return Ok(finish(before.clone(), "heuristic")); } }
    // 2. ellipsis markers
    if let Some(after) = merge_with_markers(&before, &code) { if after != before { return Ok(finish(after, "heuristic")); } }
    // 3. anchored region replacement (only when the block has no markers)
    if !code.lines().any(is_ellipsis_marker) { if let Some(after) = replace_anchored_region(&before, &code) { if after != before { return Ok(finish(after, "heuristic")); } } }
    // 4. whole-file replacement when the block looks like a complete file (shares first & last lines, similar size)
    {
        let bf = before.lines().next().map(norm).unwrap_or_default(); let cf = code.lines().next().map(norm).unwrap_or_default();
        let bl = before.lines().count(); let cl = code.lines().count();
        if !bf.is_empty() && bf == cf && cl >= bl * 7 / 10 && !code.lines().any(is_ellipsis_marker) && TextDiff::from_lines(&before, &code).ratio() > 0.5 {
            let after = if code.ends_with('\n') || !before.ends_with('\n') { code.clone() } else { format!("{code}\n") };
            return Ok(finish(after, "heuristic"));
        }
    }
    // 5. model-assisted
    let settings_state = app.state::<SettingsState>();
    let settings = settings_state.effective();
    let resolved = pick_model(&settings, &settings_state, req.model_id.as_deref(), true)?;
    let lang = req.language.clone().unwrap_or_else(|| crate::ai::util::language_for(&req.path).to_string());
    let line_count = before.lines().count();
    let instr = req.instructions.clone().map(|i| format!("\nInstructions from the chat: {i}\n")).unwrap_or_default();
    if line_count <= 400 {
        let mut r = LlmRequest::new(resolved.model.clone()).no_thinking().purpose("apply-rewrite");
        r.system = vec![SystemBlock { text: REWRITE_SYSTEM.into(), cache: true }];
        r.messages = vec![Message::user_text(format!("Language: {lang}\nFile: {rel}\n{instr}\n<file>\n{before}\n</file>\n\n<snippet>\n{code}\n</snippet>\n\nOutput the complete updated file inside <file></file>."))];
        r.max_tokens = r.max_tokens.min(32_000); r.temperature = Some(0.0);
        let (raw, _) = llm::complete_text(&resolved, r, CancellationToken::new()).await?;
        let mut after = extract_tag(&raw, "file").to_string();
        if after.starts_with('\n') { after.remove(0); }
        let after = strip_fences(&after);
        let after = if before.ends_with('\n') && !after.ends_with('\n') { format!("{after}\n") } else { after };
        let ok = !after.trim().is_empty() && after.lines().count() * 2 >= line_count.min(400);
        if ok { return Ok(finish(after, "rewrite")); }
    }
    let mut r = LlmRequest::new(resolved.model.clone()).no_thinking().purpose("apply-search-replace");
    r.system = vec![SystemBlock { text: SR_SYSTEM.into(), cache: true }];
    let (shown, _) = crate::tokens::truncate_middle(&before, 40_000);
    r.messages = vec![Message::user_text(format!("Language: {lang}\nFile: {rel}\n{instr}\n<file>\n{shown}\n</file>\n\n<snippet>\n{code}\n</snippet>"))];
    r.max_tokens = r.max_tokens.min(16_000); r.temperature = Some(0.0);
    let (raw, _) = llm::complete_text(&resolved, r, CancellationToken::new()).await?;
    let (after, n) = apply_search_replace(&before, &raw)?;
    if n == 0 { return Err(CaretError::provider("the model produced no applicable edits")); }
    Ok(finish(after, "search-replace"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_replace_exact_whitespace_and_fuzzy() {
        let before = "fn a() {\n    let x = 1;\n    let y = 2;\n}\n";
        let blocks = "<<<<<<< SEARCH\n    let x = 1;\n=======\n    let x = 10;\n>>>>>>> REPLACE\n";
        let (after, n) = apply_search_replace(before, blocks).unwrap();
        assert_eq!(n, 1); assert!(after.contains("let x = 10;") && after.ends_with('\n'));
        // whitespace-insensitive + re-indentation
        let blocks = "<<<<<<< SEARCH\nlet y = 2;\n=======\nlet y = 20;\nlet z = 3;\n>>>>>>> REPLACE\n";
        let (after, _) = apply_search_replace(before, blocks).unwrap();
        assert!(after.contains("    let y = 20;\n    let z = 3;"), "{after}");
        // fuzzy
        let blocks = "<<<<<<< SEARCH\n    let x = 1 ;\n    let y = 2;\n=======\n    done();\n>>>>>>> REPLACE\n";
        let (after, _) = apply_search_replace(before, blocks).unwrap();
        assert!(after.contains("done();"));
        assert!(apply_search_replace(before, "<<<<<<< SEARCH\nnope\n=======\nx\n>>>>>>> REPLACE\n").is_err());
        assert!(apply_search_replace(before, "no blocks").is_err());
    }
    #[test]
    fn marker_merge_and_anchored_region() {
        let before = "import a\n\ndef f():\n    return 1\n\ndef g():\n    return 2\n";
        let block = "import a\n# ... existing code ...\ndef g():\n    return 22\n";
        let after = merge_with_markers(before, block).unwrap();
        assert!(after.contains("return 1") && after.contains("return 22") && !after.contains("return 2\n"), "{after}");
        let block2 = "def f():\n    return 11\n";
        // single-line anchor segments → None for replace_anchored_region needs ≥2 lines; use it directly
        let after2 = replace_anchored_region(before, "def f():\n    print('x')\n    return 1").unwrap();
        assert!(after2.contains("print('x')") && after2.contains("def g()"));
        let _ = block2;
    }
    #[test]
    fn diff_is_unified() {
        let d = unified_diff("a.rs", "x\n", "y\n");
        assert!(d.contains("--- a/a.rs") && d.contains("+y"));
    }
}

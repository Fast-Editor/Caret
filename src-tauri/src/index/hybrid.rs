//! Reciprocal-rank fusion of BM25 and vector hits + snippet attachment.
use crate::index::embeddings::ChunkRef;
use crate::index::store::StoreHit;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodebaseSearchHit {
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub score: f32,
    pub snippet: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub name: Option<String>,
    pub source: String,
}

const K: f32 = 60.0;

fn overlaps(a: (u32, u32), b: (u32, u32)) -> bool {
    let (s1, e1) = a; let (s2, e2) = b;
    let inter = e1.min(e2) as i64 - s1.max(s2) as i64 + 1;
    if inter <= 0 { return false; }
    let shorter = (e1 - s1 + 1).min(e2 - s2 + 1) as i64;
    inter * 2 >= shorter
}

/// `alpha` weights the vector ranking (0 = BM25 only, 1 = vectors only).
pub fn rrf(bm25: &[StoreHit], vec: &[(ChunkRef, f32)], alpha: f32, limit: usize) -> Vec<CodebaseSearchHit> {
    let alpha = alpha.clamp(0.0, 1.0);
    let w_bm = if vec.is_empty() { 1.0 } else { 1.0 - alpha };
    let w_vec = if bm25.is_empty() { 1.0 } else { alpha };
    let mut fused: Vec<CodebaseSearchHit> = vec![];
    let bump = |path: &str, s: u32, e: u32, name: Option<String>, kind: Option<String>, add: f32, src: &str, fused: &mut Vec<CodebaseSearchHit>| {
        if let Some(h) = fused.iter_mut().find(|h| h.path == path && overlaps((h.start_line, h.end_line), (s, e))) {
            h.score += add;
            if h.source != src { h.source = "hybrid".into(); }
            if h.name.is_none() { h.name = name; }
            if h.kind.is_none() { h.kind = kind; }
        } else {
            fused.push(CodebaseSearchHit { path: path.to_string(), start_line: s, end_line: e, score: add, snippet: String::new(), kind, name, source: src.to_string() });
        }
    };
    for (rank, h) in bm25.iter().enumerate() { bump(&h.path, h.start_line, h.end_line, h.name.clone(), h.kind.clone(), w_bm / (K + rank as f32 + 1.0), "bm25", &mut fused); }
    for (rank, (r, _)) in vec.iter().enumerate() { bump(&r.path, r.start, r.end, None, None, w_vec / (K + rank as f32 + 1.0), "vector", &mut fused); }
    fused.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    fused.truncate(limit);
    fused
}

/// Read the referenced lines from disk (bounded) and fill `snippet`.
pub fn attach_snippets(root: &Path, hits: &mut [CodebaseSearchHit], max_lines: usize) {
    let mut cache: std::collections::HashMap<String, Vec<String>> = Default::default();
    for h in hits.iter_mut() {
        let lines = cache.entry(h.path.clone()).or_insert_with(|| {
            std::fs::read_to_string(root.join(&h.path)).map(|s| s.lines().map(|l| l.to_string()).collect()).unwrap_or_default()
        });
        if lines.is_empty() { continue; }
        let s = (h.start_line.max(1) as usize - 1).min(lines.len().saturating_sub(1));
        let e = (h.end_line as usize).min(lines.len()).max(s + 1);
        let e = e.min(s + max_lines);
        h.snippet = lines[s..e].join("\n");
        h.end_line = e as u32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sh(p: &str, s: u32, e: u32) -> StoreHit { StoreHit { path: p.into(), start_line: s, end_line: e, name: None, kind: None, score: 1.0 } }
    fn vr(p: &str, s: u32, e: u32) -> (ChunkRef, f32) { (ChunkRef { path: p.into(), start: s, end: e, hash: String::new() }, 0.9) }

    #[test]
    fn fuses_and_dedupes() {
        let bm = vec![sh("a.rs", 1, 20), sh("b.rs", 1, 20)];
        let v = vec![vr("b.rs", 5, 25), vr("c.rs", 1, 10)];
        let out = rrf(&bm, &v, 0.5, 10);
        assert_eq!(out[0].path, "b.rs"); // appears in both lists
        assert_eq!(out[0].source, "hybrid");
        assert_eq!(out.len(), 3);
        let only_bm = rrf(&bm, &[], 0.5, 10);
        assert!((only_bm[0].score - 1.0 / 61.0).abs() < 1e-6);
    }
}

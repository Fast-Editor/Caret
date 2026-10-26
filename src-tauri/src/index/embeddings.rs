//! Optional embeddings: provider detection and a flat, memory-bounded vector store.
use crate::ai::llm::{LlmProvider, ResolvedModel};
use crate::core::settings::{ModelConfig, ProviderConfig, ProviderKind, Settings, SettingsState};
use crate::error::{CaretError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct EmbeddingBackend {
    pub provider: Arc<dyn LlmProvider>,
    pub model: String,
    pub label: String,
    pub remote: bool,
}

impl EmbeddingBackend {
    fn build(p: &ProviderConfig, model: &str, key: Option<String>) -> Result<Self> {
        let mc = ModelConfig { id: format!("embed:{}", model), provider_id: p.id.clone(), model: model.to_string(), display_name: model.to_string(), ..Default::default() };
        let resolved = ResolvedModel { model: mc, provider: p.clone(), api_key: key };
        let provider = crate::ai::providers::build(&resolved)?;
        let remote = !p.base_url.as_deref().map(|u| u.contains("localhost") || u.contains("127.0.0.1")).unwrap_or(false);
        Ok(Self { provider, model: model.to_string(), label: format!("{} / {}", p.name, model), remote })
    }
    pub fn is_nomic(&self) -> bool { self.model.contains("nomic") }
    pub async fn embed(&self, inputs: &[String], for_query: bool) -> Result<Vec<Vec<f32>>> {
        let prepared: Vec<String> = if self.is_nomic() {
            let prefix = if for_query { "search_query: " } else { "search_document: " };
            inputs.iter().map(|s| format!("{prefix}{s}")).collect()
        } else { inputs.to_vec() };
        self.provider.embed(&self.model, &prepared).await
    }
}

async fn ollama_reachable(base: &str) -> bool {
    let root = base.trim_end_matches("/v1").trim_end_matches('/').to_string();
    let Ok(c) = reqwest::Client::builder().timeout(std::time::Duration::from_secs(2)).build() else { return false };
    match c.get(format!("{root}/api/tags")).send().await { Ok(r) if r.status().is_success() => {
        let body = r.text().await.unwrap_or_default();
        body.contains("nomic-embed-text") || body.contains("mxbai-embed") || body.contains("all-minilm") || body.contains("bge-")
    } _ => false }
}

/// Resolve the embeddings backend from settings, or auto-detect a local/remote one.
pub async fn detect(settings: &Settings, secrets: &SettingsState) -> Option<EmbeddingBackend> {
    let allow_remote = settings.privacy.allow_remote_embeddings && !settings.privacy.privacy_mode;
    if let (Some(pid), Some(model)) = (settings.indexing.embeddings_provider_id.as_deref(), settings.indexing.embeddings_model.as_deref()) {
        if let Some(p) = settings.provider(pid) {
            let key = p.api_key_ref.as_deref().and_then(|r| secrets.secret(r));
            match EmbeddingBackend::build(p, model, key) {
                Ok(b) if !b.remote || allow_remote => return Some(b),
                Ok(_) => tracing::info!("remote embeddings disabled by privacy settings"),
                Err(e) => tracing::warn!(error=%e, "embedding provider build failed"),
            }
        }
    }
    // auto-detect: Ollama
    if let Some(p) = settings.providers.iter().find(|p| p.kind == ProviderKind::Ollama && p.enabled) {
        let base = p.base_url.clone().unwrap_or_else(|| "http://localhost:11434/v1".into());
        if ollama_reachable(&base).await {
            if let Ok(b) = EmbeddingBackend::build(p, "nomic-embed-text", None) { return Some(b); }
        }
    }
    if !allow_remote { return None; }
    // auto-detect: OpenAI-compatible "local" provider with a key, or OpenAI
    for pid in ["local", "openai"] {
        if let Some(p) = settings.provider(pid).filter(|p| p.enabled) {
            let key = p.api_key_ref.as_deref().and_then(|r| secrets.secret(r));
            if key.is_none() { continue; }
            if let Ok(b) = EmbeddingBackend::build(p, "text-embedding-3-small", key) {
                match tokio::time::timeout(std::time::Duration::from_secs(8), b.embed(&["probe".to_string()], true)).await {
                    Ok(Ok(v)) if !v.is_empty() => return Some(b),
                    _ => {}
                }
            }
        }
    }
    None
}

// ───────────────────────── vector store ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkRef { pub path: String, pub start: u32, pub end: u32, pub hash: String }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct IdsFile { dim: usize, model: String, refs: Vec<ChunkRef> }

/// Flat f32 vectors in memory (`dim * count * 4` bytes), persisted to vectors.bin + ids.json.
pub struct VectorStore {
    dir: PathBuf,
    pub dim: usize,
    pub model: String,
    vectors: Vec<f32>,
    refs: Vec<ChunkRef>,
    alive: Vec<bool>,
    by_path: HashMap<String, Vec<usize>>,
    dead: usize,
    dirty: bool,
}

impl VectorStore {
    pub fn open(dir: &Path, model: &str) -> Self {
        let mut vs = Self { dir: dir.to_path_buf(), dim: 0, model: model.to_string(), vectors: vec![], refs: vec![], alive: vec![], by_path: HashMap::new(), dead: 0, dirty: false };
        let ids: Option<IdsFile> = std::fs::read(dir.join("ids.json")).ok().and_then(|b| serde_json::from_slice(&b).ok());
        if let Some(ids) = ids {
            if ids.model == model {
                if let Ok(bytes) = std::fs::read(dir.join("vectors.bin")) {
                    let expected = ids.refs.len() * ids.dim * 4;
                    if bytes.len() == expected && ids.dim > 0 {
                        let mut v = vec![0f32; ids.refs.len() * ids.dim];
                        for (i, ch) in bytes.chunks_exact(4).enumerate() { v[i] = f32::from_le_bytes([ch[0], ch[1], ch[2], ch[3]]); }
                        vs.dim = ids.dim; vs.vectors = v; vs.alive = vec![true; ids.refs.len()];
                        for (i, r) in ids.refs.iter().enumerate() { vs.by_path.entry(r.path.clone()).or_default().push(i); }
                        vs.refs = ids.refs;
                    }
                }
            }
        }
        vs
    }
    pub fn len(&self) -> usize { self.refs.len() - self.dead }
    pub fn is_empty(&self) -> bool { self.len() == 0 }
    pub fn has(&self, path: &str, hash: &str) -> bool {
        self.by_path.get(path).map(|ix| ix.iter().any(|&i| self.alive[i] && self.refs[i].hash == hash)).unwrap_or(false)
    }
    pub fn remove_path(&mut self, path: &str) {
        if let Some(ix) = self.by_path.remove(path) { for i in ix { if self.alive[i] { self.alive[i] = false; self.dead += 1; } } self.dirty = true; }
    }
    pub fn add(&mut self, r: ChunkRef, v: &[f32]) -> Result<()> {
        if self.dim == 0 { self.dim = v.len(); }
        if v.len() != self.dim { return Err(CaretError::internal(format!("embedding dim mismatch: {} vs {}", v.len(), self.dim))); }
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
        self.vectors.extend(v.iter().map(|x| x / norm));
        self.by_path.entry(r.path.clone()).or_default().push(self.refs.len());
        self.refs.push(r); self.alive.push(true); self.dirty = true;
        Ok(())
    }
    fn compact(&mut self) {
        if self.dead == 0 { return; }
        let dim = self.dim;
        let mut nv = Vec::with_capacity((self.refs.len() - self.dead) * dim);
        let mut nr = Vec::with_capacity(self.refs.len() - self.dead);
        for (i, r) in self.refs.iter().enumerate() { if self.alive[i] { nv.extend_from_slice(&self.vectors[i * dim..(i + 1) * dim]); nr.push(r.clone()); } }
        self.vectors = nv; self.refs = nr; self.alive = vec![true; self.refs.len()]; self.dead = 0;
        self.by_path.clear();
        for (i, r) in self.refs.iter().enumerate() { self.by_path.entry(r.path.clone()).or_default().push(i); }
    }
    pub fn save(&mut self) -> Result<()> {
        if !self.dirty { return Ok(()); }
        if self.dead * 5 > self.refs.len() { self.compact(); }
        std::fs::create_dir_all(&self.dir)?;
        let dim = self.dim;
        let mut bytes = Vec::with_capacity((self.refs.len() - self.dead) * dim * 4);
        let mut refs = Vec::with_capacity(self.refs.len() - self.dead);
        for (i, r) in self.refs.iter().enumerate() { if self.alive[i] { for x in &self.vectors[i * dim..(i + 1) * dim] { bytes.extend_from_slice(&x.to_le_bytes()); } refs.push(r.clone()); } }
        let tmp = self.dir.join("vectors.bin.tmp");
        std::fs::write(&tmp, &bytes)?; std::fs::rename(tmp, self.dir.join("vectors.bin"))?;
        let ids = IdsFile { dim, model: self.model.clone(), refs };
        let tmp = self.dir.join("ids.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(&ids)?)?; std::fs::rename(tmp, self.dir.join("ids.json"))?;
        self.dirty = false;
        Ok(())
    }
    pub fn clear(&mut self) { self.vectors.clear(); self.refs.clear(); self.alive.clear(); self.by_path.clear(); self.dead = 0; self.dirty = true; let _ = std::fs::remove_file(self.dir.join("vectors.bin")); let _ = std::fs::remove_file(self.dir.join("ids.json")); }
    /// Cosine similarity top-k over alive rows (vectors are L2-normalised on insert).
    pub fn search(&self, q: &[f32], limit: usize, path_prefix: Option<&str>) -> Vec<(ChunkRef, f32)> {
        if self.dim == 0 || q.len() != self.dim { return vec![]; }
        let qn = q.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
        let q: Vec<f32> = q.iter().map(|x| x / qn).collect();
        let mut scored: Vec<(usize, f32)> = Vec::with_capacity(self.refs.len());
        for i in 0..self.refs.len() {
            if !self.alive[i] { continue; }
            if let Some(p) = path_prefix { if !self.refs[i].path.starts_with(p) { continue; } }
            let row = &self.vectors[i * self.dim..(i + 1) * self.dim];
            let dot: f32 = row.iter().zip(q.iter()).map(|(a, b)| a * b).sum();
            scored.push((i, dot));
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored.into_iter().map(|(i, s)| (self.refs[i].clone(), s)).collect()
    }
    pub fn memory_bytes(&self) -> usize { self.vectors.len() * 4 }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vector_store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut vs = VectorStore::open(dir.path(), "m");
        vs.add(ChunkRef { path: "a".into(), start: 1, end: 2, hash: "h".into() }, &[1.0, 0.0]).unwrap();
        vs.add(ChunkRef { path: "b".into(), start: 1, end: 2, hash: "h".into() }, &[0.0, 1.0]).unwrap();
        vs.add(ChunkRef { path: "c".into(), start: 1, end: 2, hash: "h".into() }, &[0.7, 0.7]).unwrap();
        let r = vs.search(&[1.0, 0.1], 2, None);
        assert_eq!(r[0].0.path, "a"); assert_eq!(r[1].0.path, "c");
        vs.remove_path("a");
        vs.save().unwrap();
        let vs2 = VectorStore::open(dir.path(), "m");
        assert_eq!(vs2.len(), 2);
        assert!(vs2.has("b", "h")); assert!(!vs2.has("a", "h"));
        let r = vs2.search(&[1.0, 0.0], 1, Some("c"));
        assert_eq!(r[0].0.path, "c");
        // different model → ignored
        assert!(VectorStore::open(dir.path(), "other").is_empty());
    }
}

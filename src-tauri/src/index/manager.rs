//! IndexManager: per-workspace lifecycle (scan → chunk → BM25 → embeddings), incremental updates
//! from fs events, status broadcasting, and the `CodebaseSearchPort` implementation.
use crate::core::ports::{self, CodeHit, CodebaseSearchPort, DocHit};
use crate::core::settings::SettingsState;
use crate::error::{CaretError, Result};
use crate::events;
use crate::index::chunker::chunk_file;
use crate::index::embeddings::{self, ChunkRef, EmbeddingBackend, VectorStore};
use crate::index::hybrid::{self, CodebaseSearchHit};
use crate::index::store::Store;
use crate::index::walker::{self, FileEntry, Manifest, WalkOptions};
use crate::paths;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Listener, Manager};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    pub state: String,
    pub total_files: u64,
    pub indexed_files: u64,
    pub total_chunks: u64,
    pub embedded_chunks: u64,
    pub pending_files: u64,
    #[serde(skip_serializing_if = "Option::is_none")] pub last_indexed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub error: Option<String>,
    pub size_bytes: u64,
    pub embeddings_available: bool,
    pub root: Option<String>,
    pub progress: f32,
}
impl Default for IndexStatus {
    fn default() -> Self { Self { state: "idle".into(), total_files: 0, indexed_files: 0, total_chunks: 0, embedded_chunks: 0, pending_files: 0, last_indexed_at: None, error: None, size_bytes: 0, embeddings_available: false, root: None, progress: 0.0 } }
}

pub struct Inner {
    app: AppHandle,
    root: RwLock<Option<PathBuf>>,
    store: RwLock<Option<Arc<Store>>>,
    vectors: RwLock<Option<Arc<Mutex<VectorStore>>>>,
    manifest: Mutex<Manifest>,
    status: RwLock<IndexStatus>,
    paused: AtomicBool,
    cancel: Mutex<Option<CancellationToken>>,
    backend: tokio::sync::RwLock<Option<Arc<EmbeddingBackend>>>,
    backend_probed: AtomicBool,
    pending: Mutex<HashSet<PathBuf>>,
    debounce_running: AtomicBool,
    last_emit: Mutex<Instant>,
    run_lock: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
pub struct IndexManager { pub inner: Arc<Inner> }

fn now_ms() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) }

impl IndexManager {
    pub fn new(app: AppHandle) -> Self {
        let inner = Arc::new(Inner {
            app, root: RwLock::new(None), store: RwLock::new(None), vectors: RwLock::new(None), manifest: Mutex::new(Manifest::default()),
            status: RwLock::new(IndexStatus::default()), paused: AtomicBool::new(false), cancel: Mutex::new(None),
            backend: tokio::sync::RwLock::new(None), backend_probed: AtomicBool::new(false), pending: Mutex::new(HashSet::new()),
            debounce_running: AtomicBool::new(false), last_emit: Mutex::new(Instant::now() - Duration::from_secs(10)), run_lock: tokio::sync::Mutex::new(()),
        });
        Self { inner }
    }

    /// Subscribe to workspace / fs events and register the codebase search port.
    pub fn install(&self) {
        let me = self.inner.clone();
        self.inner.app.listen(events::WORKSPACE_CHANGED, move |ev| {
            let root = serde_json::from_str::<serde_json::Value>(ev.payload()).ok().and_then(|v| v.get("root").and_then(|r| r.as_str()).map(PathBuf::from));
            let me = me.clone();
            tauri::async_runtime::spawn(async move { me.set_workspace(root).await; });
        });
        let me = self.inner.clone();
        self.inner.app.listen(events::FS_CHANGED, move |ev| {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(ev.payload()) else { return };
            let paths: Vec<PathBuf> = v.get("paths").and_then(|p| p.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(PathBuf::from)).collect()).unwrap_or_default();
            if paths.is_empty() { return; }
            me.on_fs_changed(paths);
        });
        ports::set_codebase(Arc::new(IndexPort(self.inner.clone())));
    }
}

impl Inner {
    fn settings(&self) -> crate::core::settings::Settings { self.app.state::<SettingsState>().effective() }
    fn ws_dir(root: &Path) -> PathBuf { paths::sub_dir("index").join(paths::workspace_hash(root)) }
    fn walk_opts(&self, root: &Path) -> WalkOptions {
        let s = self.settings();
        WalkOptions { root: root.to_path_buf(), exclude_globs: s.indexing.exclude_globs.clone(), global_ignore: s.general.global_ignore.clone(), max_file_size: (s.indexing.max_file_size_kb as u64).max(1) * 1024 }
    }

    fn set_state(&self, state: &str, error: Option<String>) {
        { let mut st = self.status.write(); st.state = state.into(); st.error = error; }
        self.emit(true);
    }
    fn emit(&self, force: bool) {
        {
            let mut last = self.last_emit.lock();
            if !force && last.elapsed() < Duration::from_millis(250) { return; }
            *last = Instant::now();
        }
        let st = self.status.read().clone();
        events::emit(&self.app, events::INDEX_STATUS, st);
    }
    pub fn status(&self) -> IndexStatus {
        let mut st = self.status.read().clone();
        if let Some(s) = self.store.read().as_ref() { st.size_bytes = s.size_bytes(); st.total_chunks = s.doc_count(); }
        if let Some(v) = self.vectors.read().as_ref() { st.embedded_chunks = v.lock().len() as u64; st.size_bytes += v.lock().memory_bytes() as u64; }
        st
    }

    async fn set_workspace(self: &Arc<Self>, root: Option<PathBuf>) {
        if let Some(c) = self.cancel.lock().take() { c.cancel(); }
        let _guard = self.run_lock.lock().await;
        *self.store.write() = None;
        *self.vectors.write() = None;
        *self.manifest.lock() = Manifest::default();
        self.pending.lock().clear();
        self.backend_probed.store(false, Ordering::SeqCst);
        *self.backend.write().await = None;
        *self.root.write() = root.clone();
        *self.status.write() = IndexStatus { root: root.as_ref().map(|r| r.to_string_lossy().to_string()), ..Default::default() };
        drop(_guard);
        let Some(root) = root else { self.set_state("idle", None); return };
        let s = self.settings();
        if !s.indexing.enabled { self.set_state("disabled", None); return; }
        if let Err(e) = self.open_stores(&root).await { self.set_state("error", Some(e.message)); return; }
        self.emit(true);
        if s.indexing.auto_index_new_folders { let me = self.clone(); tauri::async_runtime::spawn(async move { me.run_full(false).await; }); }
    }

    async fn open_stores(&self, root: &Path) -> Result<()> {
        let dir = Self::ws_dir(root);
        let store = tokio::task::spawn_blocking({ let d = dir.join("tantivy"); move || Store::open(&d) }).await.map_err(|e| CaretError::internal(e.to_string()))??;
        let manifest = Manifest::load(&dir.join("manifest.json"));
        {
            let mut st = self.status.write();
            st.indexed_files = manifest.files.len() as u64;
            st.total_files = manifest.files.len() as u64;
            st.last_indexed_at = manifest.last_indexed_at;
            st.total_chunks = store.doc_count();
            if !manifest.files.is_empty() { st.state = "ready".into(); st.progress = 1.0; }
        }
        *self.store.write() = Some(Arc::new(store));
        *self.manifest.lock() = manifest;
        Ok(())
    }

    async fn ensure_backend(&self) -> Option<Arc<EmbeddingBackend>> {
        if !self.backend_probed.swap(true, Ordering::SeqCst) {
            let settings = self.settings();
            let secrets = self.app.state::<SettingsState>();
            let b = embeddings::detect(&settings, &secrets).await.map(Arc::new);
            if let Some(b) = &b { tracing::info!(backend = %b.label, "embeddings backend selected"); }
            *self.backend.write().await = b.clone();
            self.status.write().embeddings_available = b.is_some();
            let root = { let g = self.root.read(); g.clone() };
            if let (Some(b), Some(root)) = (b.as_ref(), root) {
                let dir = Self::ws_dir(&root).join("vectors");
                let vs = VectorStore::open(&dir, &b.model);
                *self.vectors.write() = Some(Arc::new(Mutex::new(vs)));
            }
        }
        self.backend.read().await.clone()
    }

    async fn wait_if_paused(&self, cancel: &CancellationToken) -> bool {
        while self.paused.load(Ordering::Relaxed) {
            if cancel.is_cancelled() { return false; }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        !cancel.is_cancelled()
    }

    /// Full (or forced) index run. Serialized via `run_lock`.
    pub async fn run_full(self: &Arc<Self>, force: bool) {
        if let Some(c) = self.cancel.lock().take() { c.cancel(); }
        let cancel = CancellationToken::new();
        *self.cancel.lock() = Some(cancel.clone());
        let _guard = self.run_lock.lock().await;
        let root = { let g = self.root.read(); g.clone() };
        let Some(root) = root else { return };
        let settings = self.settings();
        if !settings.indexing.enabled { self.set_state("disabled", None); return; }
        if self.store.read().is_none() { if let Err(e) = self.open_stores(&root).await { self.set_state("error", Some(e.message)); return; } }
        let store = self.store.read().clone().unwrap();
        let dir = Self::ws_dir(&root);
        if force {
            let _ = store.clear();
            *self.manifest.lock() = Manifest::default();
            if let Some(v) = self.vectors.read().as_ref() { v.lock().clear(); }
        }
        let started = Instant::now();
        // ── scan ──
        self.set_state("scanning", None);
        let opts = self.walk_opts(&root);
        let entries: Vec<FileEntry> = match tokio::task::spawn_blocking({ let o = opts.clone(); let c = cancel.clone(); move || { let mut v = Vec::new(); walker::walk(&o, |f| { v.push(f); !c.is_cancelled() }); v } }).await {
            Ok(v) => v, Err(e) => { self.set_state("error", Some(e.to_string())); return; }
        };
        if cancel.is_cancelled() { return; }
        let (changed, removed) = { let m = self.manifest.lock(); let (c, r) = m.diff(&entries); (c.into_iter().cloned().collect::<Vec<FileEntry>>(), r) };
        { let mut st = self.status.write(); st.total_files = entries.len() as u64; st.pending_files = changed.len() as u64; st.indexed_files = (entries.len() - changed.len()) as u64; st.progress = if changed.is_empty() { 1.0 } else { 0.0 }; }
        for p in &removed { store.remove_path(p); if let Some(v) = self.vectors.read().as_ref() { v.lock().remove_path(p); } self.manifest.lock().files.remove(p); }
        // ── index ──
        self.set_state("indexing", None);
        let chunk_lines = settings.indexing.chunk_lines as usize; let overlap = settings.indexing.chunk_overlap as usize;
        let mut done = 0usize; let total = changed.len().max(1);
        let mut last_commit = Instant::now();
        let mut since_commit = 0usize;
        let mut touched: Vec<(String, String)> = Vec::new(); // (rel_path, hash) for embedding
        for batch in changed.chunks(40) {
            if !self.wait_if_paused(&cancel).await { break; }
            let st = store.clone(); let batch: Vec<FileEntry> = batch.to_vec(); let prev: Vec<Option<String>> = { let m = self.manifest.lock(); batch.iter().map(|f| m.files.get(&f.rel_path).map(|r| r.hash.clone())).collect() };
            let res = tokio::task::spawn_blocking(move || {
                let mut out: Vec<(FileEntry, String, u32, bool)> = Vec::with_capacity(batch.len());
                for (f, prev_hash) in batch.into_iter().zip(prev) {
                    let Ok(bytes) = std::fs::read(&f.abs_path) else { continue };
                    let hash = walker::hash_bytes(&bytes);
                    if prev_hash.as_deref() == Some(hash.as_str()) { out.push((f, hash, 0, false)); continue; }
                    let text = String::from_utf8_lossy(&bytes);
                    let chunks = chunk_file(&f.rel_path, &text, chunk_lines, overlap);
                    let n = chunks.len() as u32;
                    if st.add_chunks(&f.rel_path, &hash, &chunks).is_ok() { out.push((f, hash, n, true)); }
                }
                out
            }).await.unwrap_or_default();
            {
                let mut m = self.manifest.lock();
                for (f, hash, n, reindexed) in &res {
                    let chunks = if *reindexed { *n } else { m.files.get(&f.rel_path).map(|r| r.chunks).unwrap_or(0) };
                    m.files.insert(f.rel_path.clone(), walker::FileRecord { hash: hash.clone(), mtime: f.mtime, size: f.size, chunks });
                    if *reindexed { touched.push((f.rel_path.clone(), hash.clone())); }
                }
            }
            done += res.len(); since_commit += res.len();
            { let mut s = self.status.write(); s.indexed_files = (entries.len() - changed.len() + done) as u64; s.pending_files = (changed.len() - done.min(changed.len())) as u64; s.progress = (done as f32 / total as f32) * 0.7; }
            self.emit(false);
            if since_commit >= 200 || last_commit.elapsed() > Duration::from_secs(2) { let _ = store.commit(); last_commit = Instant::now(); since_commit = 0; let _ = self.manifest.lock().save(&dir.join("manifest.json")); }
            tokio::task::yield_now().await;
            if cancel.is_cancelled() { break; }
        }
        let _ = store.commit();
        { let mut m = self.manifest.lock(); m.version = 1; m.root = root.to_string_lossy().to_string(); m.last_indexed_at = Some(now_ms()); let _ = m.save(&dir.join("manifest.json")); }
        if cancel.is_cancelled() { self.set_state("paused", None); return; }
        // ── embeddings ──
        if let Some(backend) = self.ensure_backend().await {
            self.set_state("embedding", None);
            let vectors = self.vectors.read().clone();
            if let Some(vs) = vectors {
                // files to embed: touched + any manifest entry missing vectors
                let mut targets: Vec<(String, String)> = { let m = self.manifest.lock(); let v = vs.lock(); m.files.iter().filter(|(p, r)| !v.has(p, &r.hash)).map(|(p, r)| (p.clone(), r.hash.clone())).collect() };
                targets.extend(touched.into_iter());
                targets.sort(); targets.dedup();
                let total = targets.len().max(1);
                let mut done = 0usize; let mut last_save = Instant::now();
                for file_batch in targets.chunks(8) {
                    if !self.wait_if_paused(&cancel).await { break; }
                    // chunk texts for this file batch
                    let items: Vec<(ChunkRef, String)> = {
                        let mut out = vec![];
                        for (rel, hash) in file_batch {
                            let Ok(text) = std::fs::read_to_string(root.join(rel)) else { continue };
                            for c in chunk_file(rel, &text, chunk_lines, overlap) { out.push((ChunkRef { path: rel.clone(), start: c.start_line, end: c.end_line, hash: hash.clone() }, c.text)); }
                        }
                        out
                    };
                    for rel in file_batch { vs.lock().remove_path(&rel.0); }
                    let mut it = items.chunks(32);
                    loop {
                        let a = it.next(); let b = it.next();
                        if a.is_none() { break; }
                        let fa = async { match a { Some(x) => Some((x, backend.embed(&x.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>(), false).await)), None => None } };
                        let fb = async { match b { Some(x) => Some((x, backend.embed(&x.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>(), false).await)), None => None } };
                        let (ra, rb) = tokio::join!(fa, fb);
                        for r in [ra, rb].into_iter().flatten() {
                            match r { (x, Ok(vecs)) => { let mut v = vs.lock(); for ((cr, _), e) in x.iter().zip(vecs) { let _ = v.add(cr.clone(), &e); } }
                                      (_, Err(e)) => { tracing::warn!(error=%e, "embedding batch failed"); self.status.write().error = Some(format!("embeddings: {}", e.message)); } }
                        }
                        if cancel.is_cancelled() { break; }
                    }
                    done += file_batch.len();
                    { let mut s = self.status.write(); s.progress = 0.7 + 0.3 * (done as f32 / total as f32); s.embedded_chunks = vs.lock().len() as u64; }
                    self.emit(false);
                    if last_save.elapsed() > Duration::from_secs(5) { let _ = vs.lock().save(); last_save = Instant::now(); }
                    tokio::task::yield_now().await;
                    if cancel.is_cancelled() { break; }
                }
                let _ = vs.lock().save();
            }
        }
        if cancel.is_cancelled() { self.set_state("paused", None); return; }
        { let mut s = self.status.write(); s.progress = 1.0; s.pending_files = 0; s.last_indexed_at = Some(now_ms()); s.total_chunks = store.doc_count(); }
        self.set_state("ready", None);
        tracing::info!(files = entries.len(), changed = changed.len(), removed = removed.len(), secs = started.elapsed().as_secs_f32(), "index run complete");
    }

    fn on_fs_changed(self: &Arc<Self>, paths: Vec<PathBuf>) {
        if self.root.read().is_none() { return; }
        { let mut p = self.pending.lock(); for x in paths { p.insert(x); } }
        if self.debounce_running.swap(true, Ordering::SeqCst) { return; }
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            me.debounce_running.store(false, Ordering::SeqCst);
            me.process_pending().await;
        });
    }

    async fn process_pending(self: &Arc<Self>) {
        let batch: Vec<PathBuf> = { let mut p = self.pending.lock(); p.drain().collect() };
        if batch.is_empty() { return; }
        let _guard = self.run_lock.lock().await;
        let root = { let g = self.root.read(); g.clone() };
        let Some(root) = root else { return };
        let settings = self.settings();
        if !settings.indexing.enabled { return; }
        let store = { let g = self.store.read(); g.clone() };
        let Some(store) = store else { return };
        let opts = self.walk_opts(&root);
        let dir = Self::ws_dir(&root);
        let mut touched: Vec<(String, String)> = vec![];
        let mut expanded: Vec<PathBuf> = vec![];
        for p in batch {
            if p.is_dir() {
                // new/renamed directory: enumerate its files (bounded)
                let mut n = 0; let mut w = ignore::WalkBuilder::new(&p); w.hidden(false).follow_links(false).max_depth(Some(6));
                for e in w.build().flatten() { if e.file_type().map(|t| t.is_file()).unwrap_or(false) { expanded.push(e.into_path()); n += 1; if n > 2000 { break; } } }
                // also drop manifest entries beneath a removed dir
            } else { expanded.push(p); }
        }
        for abs in expanded {
            let rel = match abs.strip_prefix(&root) { Ok(r) => r.to_string_lossy().replace('\\', "/"), Err(_) => continue };
            if rel.is_empty() || rel.starts_with(".git/") || rel.starts_with(".caret/") { continue; }
            if walker::is_indexable(&opts, &abs) {
                let Ok(bytes) = std::fs::read(&abs) else { continue };
                let hash = walker::hash_bytes(&bytes);
                let prev = self.manifest.lock().files.get(&rel).map(|r| r.hash.clone());
                if prev.as_deref() == Some(hash.as_str()) { continue; }
                let text = String::from_utf8_lossy(&bytes);
                let chunks = chunk_file(&rel, &text, settings.indexing.chunk_lines as usize, settings.indexing.chunk_overlap as usize);
                let md = std::fs::metadata(&abs).ok();
                if store.add_chunks(&rel, &hash, &chunks).is_ok() {
                    self.manifest.lock().files.insert(rel.clone(), walker::FileRecord { hash: hash.clone(), mtime: md.as_ref().map(walker::mtime_of).unwrap_or(0), size: md.map(|m| m.len()).unwrap_or(0), chunks: chunks.len() as u32 });
                    touched.push((rel, hash));
                }
            } else {
                // removed or no longer indexable: drop it and anything beneath it (dir removal)
                let mut m = self.manifest.lock();
                let gone: Vec<String> = m.files.keys().filter(|k| *k == &rel || k.starts_with(&format!("{rel}/"))).cloned().collect();
                for g in gone { store.remove_path(&g); if let Some(v) = self.vectors.read().as_ref() { v.lock().remove_path(&g); } m.files.remove(&g); }
            }
        }
        let _ = store.commit();
        { let mut m = self.manifest.lock(); m.last_indexed_at = Some(now_ms()); let n = m.files.len() as u64; let _ = m.save(&dir.join("manifest.json")); let mut s = self.status.write(); s.indexed_files = n; s.total_files = n; s.total_chunks = store.doc_count(); }
        if !touched.is_empty() {
            if let Some(backend) = self.ensure_backend().await {
                let vectors = { let g = self.vectors.read(); g.clone() };
                if let Some(vs) = vectors {
                    for (rel, hash) in &touched {
                        vs.lock().remove_path(rel);
                        let Ok(text) = std::fs::read_to_string(root.join(rel)) else { continue };
                        let items: Vec<(ChunkRef, String)> = chunk_file(rel, &text, settings.indexing.chunk_lines as usize, settings.indexing.chunk_overlap as usize).into_iter().map(|c| (ChunkRef { path: rel.clone(), start: c.start_line, end: c.end_line, hash: hash.clone() }, c.text)).collect();
                        for b in items.chunks(32) {
                            if let Ok(vecs) = backend.embed(&b.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>(), false).await { let mut v = vs.lock(); for ((cr, _), e) in b.iter().zip(vecs) { let _ = v.add(cr.clone(), &e); } }
                        }
                    }
                    let _ = vs.lock().save();
                }
            }
        }
        self.emit(true);
    }

    pub async fn search(&self, query: &str, limit: usize, path_prefix: Option<&str>) -> Result<Vec<CodebaseSearchHit>> {
        let root = { let g = self.root.read(); g.clone() };
        let Some(root) = root else { return Err(CaretError::no_workspace()) };
        let store = { let g = self.store.read(); g.clone() };
        let Some(store) = store else { return Ok(vec![]) };
        let limit = limit.clamp(1, 100);
        let bm25 = { let st = store.clone(); let q = query.to_string(); let p = path_prefix.map(String::from); tokio::task::spawn_blocking(move || st.search(&q, limit * 2, p.as_deref())).await.map_err(|e| CaretError::internal(e.to_string()))?? };
        let mut vec_hits = vec![];
        let backend = self.backend.read().await.clone();
        let vectors = { let g = self.vectors.read(); g.clone() };
        if let (Some(backend), Some(vs)) = (backend, vectors) {
            let empty = { let g = vs.lock(); g.is_empty() };
            if !empty {
                match tokio::time::timeout(Duration::from_secs(10), backend.embed(&[query.to_string()], true)).await {
                    Ok(Ok(mut q)) if !q.is_empty() => { let q = q.remove(0); vec_hits = vs.lock().search(&q, limit * 2, path_prefix); }
                    Ok(Err(e)) => tracing::debug!(error=%e, "query embedding failed"),
                    _ => {}
                }
            }
        }
        let alpha = self.settings().indexing.hybrid_alpha;
        let mut hits = hybrid::rrf(&bm25, &vec_hits, alpha, limit);
        hybrid::attach_snippets(&root, &mut hits, 40);
        Ok(hits)
    }

    pub fn pause(&self) { self.paused.store(true, Ordering::SeqCst); self.set_state("paused", None); }
    pub fn resume(self: &Arc<Self>) { self.paused.store(false, Ordering::SeqCst); if self.status.read().progress < 1.0 { let me = self.clone(); tauri::async_runtime::spawn(async move { me.run_full(false).await; }); } else { self.set_state("ready", None); } }
    pub async fn clear(&self) -> Result<()> {
        if let Some(c) = self.cancel.lock().take() { c.cancel(); }
        let _g = self.run_lock.lock().await;
        if let Some(s) = self.store.read().as_ref() { s.clear()?; }
        if let Some(v) = self.vectors.read().as_ref() { v.lock().clear(); }
        let root = self.root.read().clone();
        *self.manifest.lock() = Manifest::default();
        if let Some(r) = root { let _ = std::fs::remove_file(Self::ws_dir(&r).join("manifest.json")); }
        { let mut s = self.status.write(); s.indexed_files = 0; s.total_chunks = 0; s.embedded_chunks = 0; s.progress = 0.0; s.last_indexed_at = None; }
        self.set_state("idle", None);
        Ok(())
    }
}

pub struct IndexPort(pub Arc<Inner>);

#[async_trait::async_trait]
impl CodebaseSearchPort for IndexPort {
    async fn search(&self, query: &str, limit: usize, path_prefix: Option<&str>) -> Result<Vec<CodeHit>> {
        Ok(self.0.search(query, limit, path_prefix).await?.into_iter().map(|h| CodeHit { path: h.path, start_line: h.start_line, end_line: h.end_line, score: h.score, snippet: h.snippet, name: h.name }).collect())
    }
    fn is_ready(&self) -> bool { let s = self.0.status.read(); s.state == "ready" || (s.indexed_files > 0 && s.state != "error" && s.state != "disabled") }
    async fn search_docs(&self, query: &str, doc_ids: Option<&[String]>, limit: usize) -> Result<Vec<DocHit>> {
        let docs = self.0.app.state::<crate::index::docs::DocsManager>();
        let hits = docs.search(query, doc_ids, limit).await?;
        Ok(hits.into_iter().map(|h| DocHit { doc_id: h.doc_id, url: h.url, title: h.title, snippet: h.snippet, score: h.score }).collect())
    }
}

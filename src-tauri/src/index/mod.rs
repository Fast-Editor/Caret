//! Codebase indexing: ignore-aware walker → language-aware chunker → tantivy BM25 (+ optional
//! embeddings) → hybrid search. Also @Docs crawling/indexing. Memory-bounded by design:
//! streaming file reads, mmap'd tantivy segments, single writer thread, flat f32 vectors.
pub mod walker;
pub mod chunker;
pub mod store;
pub mod embeddings;
pub mod hybrid;
pub mod manager;
pub mod docs;
pub mod commands;

pub use manager::IndexManager;
pub use docs::DocsManager;

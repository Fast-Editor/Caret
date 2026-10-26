//! tantivy-backed BM25 chunk store with a code-aware tokenizer (camelCase / snake_case splitting).
use crate::error::{CaretError, Result};
use crate::index::chunker::Chunk;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::{BooleanQuery, BoostQuery, Occur, Query, TermQuery};
use tantivy::schema::{Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, STORED, STRING};
use tantivy::tokenizer::{LowerCaser, RemoveLongFilter, TextAnalyzer, Token, TokenStream, Tokenizer};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term};

// ───────────────────────── code tokenizer ─────────────────────────

/// Emits each identifier/word, then its camelCase / snake_case parts when it has several.
#[derive(Clone, Default)]
pub struct CodeTokenizer;

pub struct CodeTokenStream<'a> {
    text: &'a str,
    byte_pos: usize,
    position: usize,
    pending: Vec<Token>,
    token: Token,
}

fn is_word_char(c: char) -> bool { c.is_alphanumeric() || c == '_' }

/// Split an identifier into sub-words: `fooBarHTTPServer_v2` → [foo, Bar, HTTP, Server, v, 2].
pub fn split_identifier(word: &str) -> Vec<(usize, usize)> {
    let chars: Vec<(usize, char)> = word.char_indices().collect();
    let mut parts: Vec<(usize, usize)> = vec![];
    let mut start: Option<usize> = None;
    let n = chars.len();
    for i in 0..n {
        let (bi, c) = chars[i];
        if c == '_' || !c.is_alphanumeric() {
            if let Some(s) = start.take() { parts.push((s, bi)); }
            continue;
        }
        let prev = if i > 0 { Some(chars[i - 1].1) } else { None };
        let next = if i + 1 < n { Some(chars[i + 1].1) } else { None };
        let boundary = match prev {
            None => false,
            Some(p) => {
                (p.is_lowercase() && c.is_uppercase())
                    || (p.is_alphabetic() && c.is_ascii_digit())
                    || (p.is_ascii_digit() && c.is_alphabetic())
                    || (p.is_uppercase() && c.is_uppercase() && next.map(|n| n.is_lowercase()).unwrap_or(false))
            }
        };
        if boundary { if let Some(s) = start.take() { parts.push((s, bi)); } }
        if start.is_none() { start = Some(bi); }
    }
    if let Some(s) = start { parts.push((s, word.len())); }
    parts
}

impl Tokenizer for CodeTokenizer {
    type TokenStream<'a> = CodeTokenStream<'a>;
    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        CodeTokenStream { text, byte_pos: 0, position: usize::MAX, pending: Vec::new(), token: Token::default() }
    }
}

impl TokenStream for CodeTokenStream<'_> {
    fn advance(&mut self) -> bool {
        loop {
            if let Some(t) = self.pending.pop() {
                self.token = t;
                return true;
            }
            // scan next word
            let rest = &self.text[self.byte_pos..];
            let Some(start_rel) = rest.char_indices().find(|(_, c)| is_word_char(*c)).map(|(i, _)| i) else { return false };
            let start = self.byte_pos + start_rel;
            let end = self.text[start..].char_indices().find(|(_, c)| !is_word_char(*c)).map(|(i, _)| start + i).unwrap_or(self.text.len());
            self.byte_pos = end;
            let word = &self.text[start..end];
            let parts = split_identifier(word);
            let mut toks: Vec<Token> = Vec::with_capacity(parts.len() + 1);
            self.position = self.position.wrapping_add(1);
            toks.push(Token { offset_from: start, offset_to: end, position: self.position, text: word.to_string(), position_length: 1 });
            if parts.len() > 1 {
                for (s, e) in parts {
                    if e - s == 0 { continue; }
                    self.position = self.position.wrapping_add(1);
                    toks.push(Token { offset_from: start + s, offset_to: start + e, position: self.position, text: word[s..e].to_string(), position_length: 1 });
                }
            }
            toks.reverse();
            self.pending = toks;
        }
    }
    fn token(&self) -> &Token { &self.token }
    fn token_mut(&mut self) -> &mut Token { &mut self.token }
}

pub fn code_analyzer() -> TextAnalyzer {
    TextAnalyzer::builder(CodeTokenizer).filter(RemoveLongFilter::limit(64)).filter(LowerCaser).build()
}

/// Tokenize a query string into lowercase code terms (deduplicated, order preserved).
pub fn query_terms(q: &str) -> Vec<String> {
    let mut a = code_analyzer();
    let mut s = a.token_stream(q);
    let mut out: Vec<String> = vec![];
    while s.advance() {
        let t = &s.token().text;
        if t.len() < 2 && !t.chars().all(|c| c.is_ascii_digit()) { continue; }
        if !out.iter().any(|x| x == t) { out.push(t.clone()); }
    }
    out
}

// ───────────────────────── store ─────────────────────────

#[derive(Clone, Copy)]
struct Fields { path: Field, start: Field, end: Field, name: Field, kind: Field, body: Field, hash: Field }

pub struct Store {
    dir: PathBuf,
    _index: Index,
    /// Created on first mutation and dropped after each commit so the 32 MiB arena is not held while idle.
    writer: Mutex<Option<IndexWriter>>,
    reader: IndexReader,
    f: Fields,
    dirty: Mutex<bool>,
}

#[derive(Debug, Clone)]
pub struct StoreHit { pub path: String, pub start_line: u32, pub end_line: u32, pub name: Option<String>, pub kind: Option<String>, pub score: f32 }

fn schema() -> (Schema, Fields) {
    let mut sb = Schema::builder();
    let code = TextOptions::default().set_indexing_options(TextFieldIndexing::default().set_tokenizer("code").set_index_option(IndexRecordOption::WithFreqsAndPositions));
    let path = sb.add_text_field("path", STRING | STORED);
    let start = sb.add_u64_field("start_line", STORED);
    let end = sb.add_u64_field("end_line", STORED);
    let name = sb.add_text_field("name", code.clone().set_stored());
    let kind = sb.add_text_field("kind", STRING | STORED);
    let body = sb.add_text_field("body", code);
    let hash = sb.add_text_field("hash", STRING);
    (sb.build(), Fields { path, start, end, name, kind, body, hash })
}

const WRITER_HEAP: usize = 32 * 1024 * 1024;

impl Store {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let (sch, f) = schema();
        let open = |dir: &Path| -> std::result::Result<Index, tantivy::TantivyError> {
            let d = MmapDirectory::open(dir)?;
            Index::open_or_create(d, sch.clone())
        };
        let index = match open(dir) {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!(error=%e, dir=%dir.display(), "index incompatible; recreating");
                let _ = std::fs::remove_dir_all(dir);
                std::fs::create_dir_all(dir)?;
                open(dir).map_err(|e| CaretError::internal(format!("tantivy open: {e}")))?
            }
        };
        index.tokenizers().register("code", code_analyzer());
        let reader = index.reader_builder().reload_policy(ReloadPolicy::Manual).try_into().map_err(|e: tantivy::TantivyError| CaretError::internal(format!("tantivy reader: {e}")))?;
        Ok(Self { dir: dir.to_path_buf(), _index: index, writer: Mutex::new(None), reader, f, dirty: Mutex::new(false) })
    }

    /// Run `f` with the (lazily created) writer.
    fn with_writer<R>(&self, f: impl FnOnce(&mut IndexWriter) -> Result<R>) -> Result<R> {
        let mut g = self.writer.lock();
        if g.is_none() {
            *g = Some(self._index.writer_with_num_threads::<TantivyDocument>(1, WRITER_HEAP).map_err(|e| CaretError::internal(format!("tantivy writer: {e}")))?);
        }
        f(g.as_mut().expect("writer"))
    }
    /// Release the writer (and its memory arena); safe only after a commit.
    fn drop_writer(&self) { *self.writer.lock() = None; }

    pub fn dir(&self) -> &Path { &self.dir }

    pub fn remove_path(&self, path: &str) {
        let _ = self.with_writer(|w| { w.delete_term(Term::from_field_text(self.f.path, path)); Ok(()) });
        *self.dirty.lock() = true;
    }

    /// Replace all chunks for `path`.
    pub fn add_chunks(&self, path: &str, hash: &str, chunks: &[Chunk]) -> Result<()> {
        let f = self.f;
        self.with_writer(|w| {
            w.delete_term(Term::from_field_text(f.path, path));
            for c in chunks {
                let mut d = TantivyDocument::new();
                d.add_text(f.path, path);
                d.add_u64(f.start, c.start_line as u64);
                d.add_u64(f.end, c.end_line as u64);
                if let Some(n) = &c.name { d.add_text(f.name, n); }
                if let Some(k) = &c.kind { d.add_text(f.kind, k); }
                d.add_text(f.body, &c.text);
                d.add_text(f.hash, hash);
                w.add_document(d).map_err(|e| CaretError::internal(format!("tantivy add: {e}")))?;
            }
            Ok(())
        })?;
        *self.dirty.lock() = true;
        Ok(())
    }

    pub fn is_dirty(&self) -> bool { *self.dirty.lock() }

    pub fn commit(&self) -> Result<()> {
        if !*self.dirty.lock() { return Ok(()); }
        self.with_writer(|w| w.commit().map(|_| ()).map_err(|e| CaretError::internal(format!("tantivy commit: {e}"))))?;
        self.drop_writer();
        self.reader.reload().map_err(|e| CaretError::internal(format!("tantivy reload: {e}")))?;
        *self.dirty.lock() = false;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        self.with_writer(|w| { w.delete_all_documents().map_err(|e| CaretError::internal(e.to_string()))?; w.commit().map(|_| ()).map_err(|e| CaretError::internal(e.to_string())) })?;
        self.drop_writer();
        *self.dirty.lock() = false;
        self.reader.reload().map_err(|e| CaretError::internal(e.to_string()))?;
        Ok(())
    }

    pub fn doc_count(&self) -> u64 { self.reader.searcher().num_docs() }

    pub fn size_bytes(&self) -> u64 {
        std::fs::read_dir(&self.dir).map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()).unwrap_or(0)
    }

    pub fn search(&self, query: &str, limit: usize, path_prefix: Option<&str>) -> Result<Vec<StoreHit>> {
        let terms = query_terms(query);
        if terms.is_empty() { return Ok(vec![]); }
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::with_capacity(terms.len() * 2);
        for t in &terms {
            clauses.push((Occur::Should, Box::new(TermQuery::new(Term::from_field_text(self.f.body, t), IndexRecordOption::WithFreqs))));
            clauses.push((Occur::Should, Box::new(BoostQuery::new(Box::new(TermQuery::new(Term::from_field_text(self.f.name, t), IndexRecordOption::WithFreqs)), 2.0))));
        }
        let q = BooleanQuery::new(clauses);
        let searcher = self.reader.searcher();
        let fetch = if path_prefix.is_some() { limit.saturating_mul(4).max(50) } else { limit };
        let top = searcher.search(&q, &TopDocs::with_limit(fetch.max(1)).order_by_score()).map_err(|e| CaretError::internal(format!("tantivy search: {e}")))?;
        let mut out = Vec::with_capacity(limit);
        for (score, addr) in top {
            let doc: TantivyDocument = searcher.doc(addr).map_err(|e| CaretError::internal(e.to_string()))?;
            let path = doc.get_first(self.f.path).and_then(|v| v.as_str()).unwrap_or("").to_string();
            if let Some(p) = path_prefix { if !path.starts_with(p) { continue; } }
            out.push(StoreHit {
                path,
                start_line: doc.get_first(self.f.start).and_then(|v| v.as_u64()).unwrap_or(1) as u32,
                end_line: doc.get_first(self.f.end).and_then(|v| v.as_u64()).unwrap_or(1) as u32,
                name: doc.get_first(self.f.name).and_then(|v| v.as_str()).map(String::from),
                kind: doc.get_first(self.f.kind).and_then(|v| v.as_str()).map(String::from),
                score,
            });
            if out.len() >= limit { break; }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_identifiers() {
        let w = "fooBarHTTPServer_v2";
        let parts: Vec<&str> = split_identifier(w).into_iter().map(|(s, e)| &w[s..e]).collect();
        assert_eq!(parts, vec!["foo", "Bar", "HTTP", "Server", "v", "2"]);
        assert_eq!(query_terms("getUserName"), vec!["getusername", "get", "user", "name"]);
        assert_eq!(query_terms("snake_case_fn()"), vec!["snake_case_fn", "snake", "case", "fn"]);
    }

    #[test]
    fn store_roundtrip_and_ranking() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path()).unwrap();
        let mk = |n: &str, body: &str| Chunk { start_line: 1, end_line: 3, text: body.to_string(), name: Some(n.to_string()), kind: Some("function".into()) };
        s.add_chunks("src/auth.rs", "h1", &[mk("validateToken", "src/auth.rs › validateToken\nfn validate_token(t: &str) -> bool { jwt::decode(t) }")]).unwrap();
        s.add_chunks("src/db.rs", "h2", &[mk("connect", "src/db.rs › connect\nfn connect(url: &str) -> Pool { Pool::new(url) }")]).unwrap();
        s.add_chunks("src/user.rs", "h3", &[mk("getUserName", "src/user.rs › getUserName\nfn get_user_name(id: u64) -> String { token_lookup(id) }")]).unwrap();
        s.commit().unwrap();
        assert_eq!(s.doc_count(), 3);
        let hits = s.search("validate token", 10, None).unwrap();
        assert_eq!(hits[0].path, "src/auth.rs");
        let hits = s.search("user name", 10, None).unwrap();
        assert_eq!(hits[0].path, "src/user.rs");
        let hits = s.search("connect", 10, Some("src/d")).unwrap();
        assert_eq!(hits.len(), 1);
        // replace + remove
        s.add_chunks("src/db.rs", "h2b", &[mk("disconnect", "src/db.rs › disconnect\nfn disconnect() {}")]).unwrap();
        s.remove_path("src/auth.rs");
        s.commit().unwrap();
        assert_eq!(s.doc_count(), 2);
        assert!(s.search("validate", 10, None).unwrap().is_empty());
        assert_eq!(s.search("disconnect", 10, None).unwrap().len(), 1);
    }
}

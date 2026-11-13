//! Shared diagnostics store: LSP `publishDiagnostics` (relayed by core/lsp) and UI-reported
//! diagnostics (Monaco TS worker) land here so agent tools (`read_lints`) and @Lint mentions can read them.
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspPosition { pub line: u32, pub character: u32 }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspRange { pub start: LspPosition, pub end: LspPosition }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub range: LspRange,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub severity: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub code: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub source: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub tags: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub related_information: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub code_description: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiagnostics {
    pub uri: String,
    pub path: String,
    pub diagnostics: Vec<Diagnostic>,
    pub server_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub version: Option<i64>,
}

/// path → (server_id → diagnostics)
#[derive(Default)]
pub struct DiagnosticsStore { inner: DashMap<String, DashMap<String, Vec<Diagnostic>>> }

impl DiagnosticsStore {
    pub fn publish(&self, fd: FileDiagnostics) {
        let entry = self.inner.entry(fd.path.clone()).or_default();
        if fd.diagnostics.is_empty() { entry.remove(&fd.server_id); } else { entry.insert(fd.server_id, fd.diagnostics); }
    }
    pub fn clear_server(&self, server_id: &str) { for e in self.inner.iter() { e.value().remove(server_id); } }
    pub fn for_path(&self, path: &str) -> Vec<FileDiagnostics> {
        self.inner.get(path).map(|m| m.iter().map(|kv| FileDiagnostics { uri: format!("file://{path}"), path: path.to_string(), diagnostics: kv.value().clone(), server_id: kv.key().clone(), version: None }).collect()).unwrap_or_default()
    }
    pub fn all(&self) -> Vec<FileDiagnostics> {
        self.inner.iter().flat_map(|e| { let path = e.key().clone(); e.value().iter().map(move |kv| FileDiagnostics { uri: format!("file://{path}"), path: path.clone(), diagnostics: kv.value().clone(), server_id: kv.key().clone(), version: None }).collect::<Vec<_>>() }).collect()
    }
    /// Diagnostics of files under `root` (what a window showing that folder lists).
    pub fn under_root(&self, root: &std::path::Path) -> Vec<FileDiagnostics> {
        self.all().into_iter().filter(|fd| std::path::Path::new(&fd.path).starts_with(root)).collect()
    }
    /// Errors+warnings as compact text lines for prompts: `path:line:col: [severity] message`.
    pub fn summary(&self, paths: Option<&[String]>, max_lines: usize) -> String {
        let mut out = vec![];
        for fd in self.all() {
            if let Some(ps) = paths { if !ps.iter().any(|p| p == &fd.path) { continue; } }
            for d in fd.diagnostics { if d.severity.unwrap_or(1) > 2 { continue; }
                let sev = if d.severity == Some(2) { "warning" } else { "error" };
                out.push(format!("{}:{}:{}: [{}] {}", fd.path, d.range.start.line + 1, d.range.start.character + 1, sev, d.message.lines().next().unwrap_or("")));
                if out.len() >= max_lines { return out.join("\n"); }
            }
        }
        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fd(path: &str, server: &str) -> FileDiagnostics {
        let d = Diagnostic { range: LspRange { start: LspPosition { line: 0, character: 0 }, end: LspPosition { line: 0, character: 1 } }, severity: Some(1), code: None, source: None, message: "boom".into(), tags: None, related_information: None, code_description: None, data: None };
        FileDiagnostics { uri: format!("file://{path}"), path: path.into(), diagnostics: vec![d], server_id: server.into(), version: None }
    }

    #[test]
    fn under_root_keeps_only_files_inside_that_folder() {
        let st = DiagnosticsStore::default();
        st.publish(fd("/work/a/src/main.rs", "rust-1"));
        st.publish(fd("/work/a/lib.ts", "ts-1"));
        st.publish(fd("/work/ab/x.ts", "ts-2"));
        st.publish(fd("/work/b/y.ts", "ts-3"));
        let mut a: Vec<String> = st.under_root(std::path::Path::new("/work/a")).into_iter().map(|f| f.path).collect(); a.sort();
        assert_eq!(a, vec!["/work/a/lib.ts".to_string(), "/work/a/src/main.rs".to_string()], "/work/ab is a sibling, not a child");
        assert_eq!(st.under_root(std::path::Path::new("/work/b")).len(), 1);
        assert!(st.under_root(std::path::Path::new("/work/c")).is_empty());
        assert_eq!(st.all().len(), 4);
    }
}

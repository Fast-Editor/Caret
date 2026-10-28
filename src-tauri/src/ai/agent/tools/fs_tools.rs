//! File tools: read_file, list_dir, edit_file, write_file, delete_file, rename_file.
use super::{arg_bool, arg_str, arg_u64, cap_output, edit, s_arr, s_bool, s_int, s_str, schema, Tool, ToolContext, ToolOutput};
use crate::ai::agent::hooks::{self, HookEvent, HookPermission};
use crate::ai::agent::policy::ToolEffect;
use crate::ai::agent::session::{ChangeKind, ChangeStatus, PendingChange};
use crate::error::{CaretError, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

pub fn tools() -> Vec<Arc<dyn Tool>> { vec![Arc::new(ReadFile), Arc::new(ListDir), Arc::new(EditFile), Arc::new(WriteFile), Arc::new(DeleteFile), Arc::new(RenameFile)] }

const MAX_CONTENT_BYTES: u64 = 8 * 1024 * 1024;

pub fn read_text(p: &Path) -> Result<String> {
    let meta = std::fs::metadata(p).map_err(|_| CaretError::not_found(format!("File {}", p.display())))?;
    if meta.is_dir() { return Err(CaretError::invalid(format!("{} is a directory (use list_dir)", p.display()))); }
    if meta.len() > MAX_CONTENT_BYTES { return Err(CaretError::invalid(format!("{} is too large ({} MB)", p.display(), meta.len() / 1_048_576))); }
    let bytes = std::fs::read(p)?;
    if content_inspector::inspect(&bytes[..bytes.len().min(8192)]).is_binary() { return Err(CaretError::invalid(format!("{} is a binary file", p.display()))); }
    Ok(match String::from_utf8(bytes) { Ok(s) => s, Err(e) => { let (cow, _, _) = encoding_rs::WINDOWS_1252.decode(e.as_bytes()); cow.into_owned() } })
}

/// Record + apply a file change through checkpoints/pending-change registry. Returns the summary.
pub async fn apply_change(ctx: &ToolContext, abs: &Path, kind: ChangeKind, before: Option<String>, after: Option<String>, old_path: Option<String>) -> Result<crate::ai::agent::session::FileChangeSummary> {
    let rel = ctx.display(abs);
    ctx.manager.snapshot_before_write(&ctx.session_id, &ctx.turn_id, abs, before.as_deref());
    if let Some(op) = &old_path { let old_abs = ctx.resolve(op); ctx.manager.snapshot_before_write(&ctx.session_id, &ctx.turn_id, &old_abs, before.as_deref()); }
    let (diff, add, del) = edit::unified_diff(&rel, before.as_deref().unwrap_or(""), after.as_deref().unwrap_or(""));
    if ctx.settings.agent.auto_apply_edits {
        match kind {
            ChangeKind::Create | ChangeKind::Modify => { if let Some(parent) = abs.parent() { std::fs::create_dir_all(parent)?; } std::fs::write(abs, after.as_deref().unwrap_or(""))?; }
            ChangeKind::Delete => { if abs.is_dir() { std::fs::remove_dir_all(abs)?; } else if abs.exists() { std::fs::remove_file(abs)?; } }
            ChangeKind::Rename => { if let Some(op) = &old_path { let old_abs = ctx.resolve(op); if let Some(parent) = abs.parent() { std::fs::create_dir_all(parent)?; } std::fs::rename(&old_abs, abs)?; } }
        }
    }
    let change = PendingChange { change_id: crate::ai::agent::session::new_id(), session_id: ctx.session_id.clone(), path: rel.clone(), kind, before, after, diff: diff.clone(), status: ChangeStatus::Pending, turn_id: Some(ctx.turn_id.clone()), tool_call_id: Some(ctx.tool_call_id.clone()), additions: add, deletions: del, old_path };
    let summary = ctx.manager.add_change(&ctx.session_id, change);
    let _ = hooks::run(ctx.root.as_deref(), HookEvent::AfterFileEdit, json!({ "file_path": abs.to_string_lossy(), "kind": format!("{:?}", kind).to_lowercase(), "session_id": ctx.session_id, "diff": diff.chars().take(20000).collect::<String>() })).await;
    Ok(summary)
}

pub struct ReadFile;
#[async_trait::async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str { "read_file" }
    fn display_name(&self) -> &str { "Read file" }
    fn description(&self) -> String { "Read the contents of a file with line numbers. Prefer reading whole files when they are small; for large files pass start_line/end_line (1-based, inclusive) and read the sections you need. Output is capped at 2000 lines per call — call again with a later start_line to continue. Binary files cannot be read.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "path": s_str("Path to the file, relative to the workspace root or absolute"), "start_line": s_int("First line to read (1-based)"), "end_line": s_int("Last line to read (1-based, inclusive)") }), &["path"]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "files" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::ReadOnly }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let p = arg_str(&input, &["path", "target_file", "file_path"]).ok_or_else(|| CaretError::invalid("path is required"))?;
        let abs = ctx.resolve(p);
        let h = hooks::run(ctx.root.as_deref(), HookEvent::BeforeReadFile, json!({ "file_path": abs.to_string_lossy(), "session_id": ctx.session_id })).await;
        if h.permission == HookPermission::Deny { return Ok(ToolOutput::error(format!("Reading {} was blocked by a hook{}", p, h.user_message.map(|m| format!(": {m}")).unwrap_or_default()))); }
        if abs.is_dir() { return Ok(ToolOutput::error(format!("{} is a directory. Use list_dir to see its contents.", p))); }
        let content = match read_text(&abs) { Ok(c) => c, Err(e) => return Ok(ToolOutput::error(e.message)) };
        let lines: Vec<&str> = content.split('\n').collect();
        let total = if content.ends_with('\n') { lines.len() - 1 } else { lines.len() };
        let start = arg_u64(&input, &["start_line", "start_line_one_indexed"]).unwrap_or(1).max(1) as usize;
        let end = arg_u64(&input, &["end_line", "end_line_one_indexed_inclusive"]).map(|e| e as usize).unwrap_or(total).min(total);
        if start > total && total > 0 { return Ok(ToolOutput::error(format!("start_line {start} is past the end of the file ({total} lines)"))); }
        let hard_end = end.min(start + 1999);
        let mut out = String::new();
        let mut chars = 0usize;
        let mut last = start.saturating_sub(1);
        for (i, l) in lines.iter().enumerate().take(hard_end).skip(start.saturating_sub(1)) {
            let ln = format!("{:>5}| {}\n", i + 1, l);
            chars += ln.len(); if chars > 60_000 { break; }
            out.push_str(&ln); last = i + 1;
        }
        let header = if start == 1 && last >= total { format!("{} ({} lines)\n", ctx.display(&abs), total) } else { format!("{} (lines {}-{} of {})\n", ctx.display(&abs), start, last, total) };
        let mut text = header + &out;
        if last < end { text.push_str(&format!("\n[... {} more lines; call read_file with start_line={} to continue]", end - last, last + 1)); }
        if total == 0 { text.push_str("(empty file)"); }
        Ok(ToolOutput::text(text).with_summary(format!("Read {}", ctx.display(&abs))))
    }
}

pub struct ListDir;
#[async_trait::async_trait]
impl Tool for ListDir {
    fn name(&self) -> &str { "list_dir" }
    fn display_name(&self) -> &str { "List directory" }
    fn description(&self) -> String { "List files and folders in a directory (respects .gitignore). Use depth up to 3 to see nested structure. Good for discovering project layout before reading files.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "path": s_str("Directory path relative to the workspace root (default: workspace root)"), "depth": s_int("How many levels deep to list (1-3, default 1)") }), &[]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "files" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::ReadOnly }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let p = arg_str(&input, &["path", "relative_workspace_path", "target_directory"]).unwrap_or(".");
        let abs = ctx.resolve(p);
        if !abs.is_dir() { return Ok(ToolOutput::error(format!("{} is not a directory", p))); }
        let depth = arg_u64(&input, &["depth"]).unwrap_or(1).clamp(1, 3) as usize;
        let mut walker = ignore::WalkBuilder::new(&abs);
        let depth = if crate::paths::is_broad_root(&abs) { 1 } else { depth }; // never descend into ~ / protected folders
        walker.max_depth(Some(depth)).hidden(false).git_ignore(true).git_global(true).git_exclude(true).add_custom_ignore_filename(".caretignore").add_custom_ignore_filename(".cursorignore").sort_by_file_name(|a, b| a.cmp(b));
        let mut entries: Vec<(String, bool, u64)> = vec![];
        let mut limited = false;
        for e in walker.build().flatten() {
            if e.depth() == 0 { continue; }
            let rel = e.path().strip_prefix(&abs).unwrap_or(e.path()).to_string_lossy().to_string();
            if rel.starts_with(".git/") || rel == ".git" { continue; }
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            entries.push((rel, is_dir, size));
            if entries.len() >= 600 { limited = true; break; }
        }
        entries.sort_by(|a, b| { let da = a.0.matches('/').count(); let db = b.0.matches('/').count(); da.cmp(&db).then(b.1.cmp(&a.1)).then(a.0.cmp(&b.0)) });
        let mut out = format!("{}/\n", ctx.display(&abs));
        for (rel, is_dir, size) in &entries {
            if *is_dir { out.push_str(&format!("  {rel}/\n")); } else { out.push_str(&format!("  {rel} ({})\n", human_size(*size))); }
        }
        if entries.is_empty() { out.push_str("  (empty)\n"); }
        if limited { out.push_str("  … (listing truncated at 600 entries; list a subdirectory for more)\n"); }
        Ok(ToolOutput::text(out).with_summary(format!("Listed {}", ctx.display(&abs))))
    }
}

pub fn human_size(n: u64) -> String { if n < 1024 { format!("{n} B") } else if n < 1024 * 1024 { format!("{:.1} KB", n as f64 / 1024.0) } else { format!("{:.1} MB", n as f64 / 1048576.0) } }

pub struct EditFile;
#[async_trait::async_trait]
impl Tool for EditFile {
    fn name(&self) -> &str { "edit_file" }
    fn display_name(&self) -> &str { "Edit file" }
    fn description(&self) -> String { "Make precise edits to an existing file by replacing exact text. Each edit's `old_string` must match the file exactly (whitespace-insensitive matching is attempted as a fallback) and must be unique — include enough surrounding lines to disambiguate. `new_string` replaces it. Use multiple edits for several changes in one file; they apply in order. To create a new file, use write_file. Do not rewrite whole files with this tool.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "path": s_str("File to edit (relative to workspace root)"), "edits": s_arr("Ordered list of replacements", json!({ "type": "object", "properties": { "old_string": s_str("Exact existing text to replace (unique in the file)"), "new_string": s_str("Replacement text") }, "required": ["old_string", "new_string"], "additionalProperties": false })), "create_if_missing": s_bool("Create the file (with the edits' new_string content) when it does not exist") }), &["path", "edits"]) }
    fn readonly(&self) -> bool { false }
    fn group(&self) -> &str { "files" }
    fn effect(&self, i: &Value) -> ToolEffect { ToolEffect::FileEdit(arg_str(i, &["path", "target_file"]).map(|s| vec![s.to_string()]).unwrap_or_default()) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let p = arg_str(&input, &["path", "target_file"]).ok_or_else(|| CaretError::invalid("path is required"))?;
        let abs = ctx.resolve(p);
        let edits: Vec<(String, String)> = match input.get("edits").and_then(|v| v.as_array()) {
            Some(a) => a.iter().map(|e| (arg_str(e, &["old_string", "old_str", "old"]).unwrap_or("").to_string(), arg_str(e, &["new_string", "new_str", "new"]).unwrap_or("").to_string())).collect(),
            None => { let o = arg_str(&input, &["old_string", "old_str"]); let n = arg_str(&input, &["new_string", "new_str", "code_edit"]); match (o, n) { (Some(o), Some(n)) => vec![(o.to_string(), n.to_string())], _ => return Ok(ToolOutput::error("edits array is required: [{old_string, new_string}]")) } }
        };
        if edits.is_empty() { return Ok(ToolOutput::error("No edits provided")); }
        let exists = abs.exists();
        let before = if exists { match read_text(&abs) { Ok(c) => c, Err(e) => return Ok(ToolOutput::error(e.message)) } } else { String::new() };
        if !exists {
            let create = arg_bool(&input, &["create_if_missing"]).unwrap_or(false) || (edits.len() == 1 && edits[0].0.is_empty());
            if !create { return Ok(ToolOutput::error(format!("{} does not exist. Pass create_if_missing=true or use write_file.", p))); }
            let content = edits.iter().map(|e| e.1.clone()).collect::<Vec<_>>().join("\n");
            let summary = apply_change(ctx, &abs, ChangeKind::Create, None, Some(content.clone()), None).await?;
            return Ok(ToolOutput { text: format!("Created {} ({} lines)", ctx.display(&abs), content.lines().count()), file_changes: vec![summary], ..Default::default() }.with_summary(format!("Created {}", ctx.display(&abs))));
        }
        let mut content = before.clone();
        let mut methods = vec![];
        for (i, (old, new)) in edits.iter().enumerate() {
            if old.is_empty() { if content.is_empty() { content = new.clone(); methods.push("append"); continue; } return Ok(ToolOutput::error(format!("Edit {}: old_string is empty. Provide the exact text to replace (or use write_file for whole-file content).", i + 1))); }
            match edit::locate(&content, old) {
                edit::MatchResult::Found { start, end, method } => { content = edit::apply(&content, start, end, new); methods.push(method); }
                edit::MatchResult::Ambiguous(n) => return Ok(ToolOutput::error(format!("Edit {}: old_string matches {} places in {}. Include more surrounding context to make it unique.", i + 1, n, p))),
                edit::MatchResult::NotFound => {
                    let hint = old.lines().next().map(|l| l.trim()).filter(|l| !l.is_empty()).map(|l| format!(" The first line `{}` was {}found in the file.", l.chars().take(80).collect::<String>(), if content.contains(l) { "" } else { "not " })).unwrap_or_default();
                    return Ok(ToolOutput::error(format!("Edit {}: old_string was not found in {}.{} Re-read the file and copy the exact text.", i + 1, p, hint)));
                }
            }
        }
        if content == before { return Ok(ToolOutput::error("The edits produced no change (new_string equals old_string?)")); }
        let summary = apply_change(ctx, &abs, ChangeKind::Modify, Some(before), Some(content), None).await?;
        let (diff_text, _) = cap_output(&ctx.manager.change_diff(&summary.change_id).unwrap_or_default(), 3000);
        let applied = if ctx.settings.agent.auto_apply_edits { "Applied" } else { "Staged (pending user review)" };
        Ok(ToolOutput { text: format!("{} {} edit(s) to {} (+{} -{}; match: {}).\n{}", applied, edits.len(), ctx.display(&abs), summary.additions, summary.deletions, methods.join(","), diff_text), diff: Some(diff_text.clone()), file_changes: vec![summary.clone()], ..Default::default() }.with_summary(format!("Edited {} (+{} -{})", ctx.display(&abs), summary.additions, summary.deletions)))
    }
}

pub struct WriteFile;
#[async_trait::async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &str { "write_file" }
    fn display_name(&self) -> &str { "Write file" }
    fn description(&self) -> String { "Create a new file or completely overwrite an existing one with the given content. Parent directories are created. Prefer edit_file for changes to existing files.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "path": s_str("File path (relative to workspace root)"), "content": s_str("Full file content") }), &["path", "content"]) }
    fn readonly(&self) -> bool { false }
    fn group(&self) -> &str { "files" }
    fn effect(&self, i: &Value) -> ToolEffect { ToolEffect::FileEdit(arg_str(i, &["path"]).map(|s| vec![s.to_string()]).unwrap_or_default()) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let p = arg_str(&input, &["path", "target_file"]).ok_or_else(|| CaretError::invalid("path is required"))?;
        let content = arg_str(&input, &["content", "contents", "code"]).ok_or_else(|| CaretError::invalid("content is required"))?.to_string();
        let abs = ctx.resolve(p);
        if abs.is_dir() { return Ok(ToolOutput::error(format!("{} is a directory", p))); }
        let (kind, before) = if abs.exists() { (ChangeKind::Modify, read_text(&abs).ok()) } else { (ChangeKind::Create, None) };
        if before.as_deref() == Some(content.as_str()) { return Ok(ToolOutput::text(format!("{} already has this exact content; nothing to do.", ctx.display(&abs)))); }
        let summary = apply_change(ctx, &abs, kind, before, Some(content.clone()), None).await?;
        let verb = if kind == ChangeKind::Create { "Created" } else { "Overwrote" };
        Ok(ToolOutput { text: format!("{} {} ({} lines, +{} -{})", verb, ctx.display(&abs), content.lines().count(), summary.additions, summary.deletions), file_changes: vec![summary.clone()], ..Default::default() }.with_summary(format!("{} {}", verb, ctx.display(&abs))))
    }
}

pub struct DeleteFile;
#[async_trait::async_trait]
impl Tool for DeleteFile {
    fn name(&self) -> &str { "delete_file" }
    fn display_name(&self) -> &str { "Delete file" }
    fn description(&self) -> String { "Delete a file (or an empty directory; pass recursive=true for non-empty directories). Deletions are checkpointed and can be restored by the user.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "path": s_str("Path to delete"), "recursive": s_bool("Allow deleting a non-empty directory") }), &["path"]) }
    fn readonly(&self) -> bool { false }
    fn group(&self) -> &str { "files" }
    fn effect(&self, i: &Value) -> ToolEffect { ToolEffect::FileDelete(arg_str(i, &["path", "target_file"]).map(|s| vec![s.to_string()]).unwrap_or_default()) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let p = arg_str(&input, &["path", "target_file"]).ok_or_else(|| CaretError::invalid("path is required"))?;
        let abs = ctx.resolve(p);
        if !abs.exists() { return Ok(ToolOutput::error(format!("{} does not exist", p))); }
        if abs.is_dir() {
            let mut files = vec![]; for e in walkdir::WalkDir::new(&abs).into_iter().flatten() { if e.file_type().is_file() { files.push(e.path().to_path_buf()); if files.len() > 500 { break; } } }
            if !files.is_empty() && !arg_bool(&input, &["recursive"]).unwrap_or(false) { return Ok(ToolOutput::error(format!("{} is a non-empty directory ({} files). Pass recursive=true to delete it.", p, files.len()))); }
            if files.len() > 500 { return Ok(ToolOutput::error("Refusing to delete a directory with more than 500 files; use run_terminal_cmd deliberately instead.")); }
            let mut summaries = vec![];
            for f in files { let before = read_text(&f).ok(); summaries.push(apply_change(ctx, &f, ChangeKind::Delete, before, None, None).await?); }
            let _ = std::fs::remove_dir_all(&abs);
            return Ok(ToolOutput { text: format!("Deleted directory {} ({} files)", ctx.display(&abs), summaries.len()), file_changes: summaries, ..Default::default() }.with_summary(format!("Deleted {}", ctx.display(&abs))));
        }
        let before = read_text(&abs).ok();
        let summary = apply_change(ctx, &abs, ChangeKind::Delete, before, None, None).await?;
        Ok(ToolOutput { text: format!("Deleted {}", ctx.display(&abs)), file_changes: vec![summary], ..Default::default() }.with_summary(format!("Deleted {}", ctx.display(&abs))))
    }
}

pub struct RenameFile;
#[async_trait::async_trait]
impl Tool for RenameFile {
    fn name(&self) -> &str { "rename_file" }
    fn display_name(&self) -> &str { "Rename / move file" }
    fn description(&self) -> String { "Rename or move a file or directory to a new path (parent directories are created).".into() }
    fn input_schema(&self) -> Value { schema(json!({ "old_path": s_str("Current path"), "new_path": s_str("New path") }), &["old_path", "new_path"]) }
    fn readonly(&self) -> bool { false }
    fn group(&self) -> &str { "files" }
    fn effect(&self, i: &Value) -> ToolEffect { ToolEffect::FileDelete([arg_str(i, &["old_path"]), arg_str(i, &["new_path"])].into_iter().flatten().map(String::from).collect()) }
    fn label(&self, i: &Value) -> Option<String> { Some(format!("{} → {}", arg_str(i, &["old_path"]).unwrap_or("?"), arg_str(i, &["new_path"]).unwrap_or("?"))) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let o = arg_str(&input, &["old_path", "from"]).ok_or_else(|| CaretError::invalid("old_path is required"))?;
        let n = arg_str(&input, &["new_path", "to"]).ok_or_else(|| CaretError::invalid("new_path is required"))?;
        let (oa, na) = (ctx.resolve(o), ctx.resolve(n));
        if !oa.exists() { return Ok(ToolOutput::error(format!("{} does not exist", o))); }
        if na.exists() { return Ok(ToolOutput::error(format!("{} already exists", n))); }
        let before = if oa.is_file() { read_text(&oa).ok() } else { None };
        let summary = apply_change(ctx, &na, ChangeKind::Rename, before.clone(), before, Some(o.to_string())).await?;
        Ok(ToolOutput { text: format!("Renamed {} → {}", o, n), file_changes: vec![summary], ..Default::default() }.with_summary(format!("Renamed {} → {}", o, n)))
    }
}

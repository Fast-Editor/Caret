//! Meta tools: todo_write, update_memory, fetch_rules, skill, ask_question, create_plan, edit_plan, read_lints, task.
use super::{arg_bool, arg_str, cap_output, s_arr, s_bool, s_str, schema, Tool, ToolContext, ToolOutput};
use crate::ai::agent::policy::ToolEffect;
use crate::ai::agent::session::{AgentQuestion, TodoItem};
use crate::ai::agent::{memories, plans, rules, skills};
use crate::core::diagnostics::DiagnosticsStore;
use crate::error::{CaretError, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::Manager;

pub fn tools() -> Vec<Arc<dyn Tool>> { vec![Arc::new(TodoWrite), Arc::new(UpdateMemory), Arc::new(FetchRules), Arc::new(Skill), Arc::new(AskQuestion), Arc::new(CreatePlan), Arc::new(EditPlan), Arc::new(ReadLints), Arc::new(Task)] }

pub struct TodoWrite;
#[async_trait::async_trait]
impl Tool for TodoWrite {
    fn name(&self) -> &str { "todo_write" }
    fn display_name(&self) -> &str { "Update todos" }
    fn description(&self) -> String { "Create or update the task list for multi-step work so the user can follow progress. Keep items short and actionable; mark exactly one item in_progress at a time; mark items completed as soon as they are done. Use merge=true to update only the listed items (by id); merge=false replaces the whole list.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "todos": s_arr("Todo items", json!({ "type": "object", "properties": { "id": s_str("Stable id"), "content": s_str("What to do"), "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "cancelled"] } }, "required": ["id", "content", "status"], "additionalProperties": false })), "merge": s_bool("Merge into the existing list instead of replacing it") }), &["todos"]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "planning" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::Benign }
    fn label(&self, i: &Value) -> Option<String> { i.get("todos").and_then(|t| t.as_array()).map(|a| format!("{} items", a.len())) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let items: Vec<TodoItem> = serde_json::from_value(input.get("todos").cloned().unwrap_or(Value::Array(vec![]))).map_err(|e| CaretError::invalid(format!("todos: {e}")))?;
        let merge = arg_bool(&input, &["merge"]).unwrap_or(false);
        let todos = ctx.manager.update_todos(&ctx.session_id, items, merge);
        let done = todos.iter().filter(|t| t.status == "completed").count();
        Ok(ToolOutput::text(format!("Todo list updated: {} items ({} completed).\n{}", todos.len(), done, todos.iter().map(|t| format!("- [{}] {}", match t.status.as_str() { "completed" => "x", "in_progress" => ">", "cancelled" => "-", _ => " " }, t.content)).collect::<Vec<_>>().join("\n"))).with_summary(format!("{}/{} done", done, todos.len())))
    }
}

pub struct UpdateMemory;
#[async_trait::async_trait]
impl Tool for UpdateMemory {
    fn name(&self) -> &str { "update_memory" }
    fn display_name(&self) -> &str { "Update memory" }
    fn description(&self) -> String { "Save, update or delete a persistent memory about this project or the user's preferences (e.g. \"tests run with `pnpm vitest`\", \"user prefers tabs\"). Memories are shown in future conversations. Only store durable, non-sensitive facts the user confirmed or clearly implied.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "action": { "type": "string", "enum": ["add", "update", "delete"] }, "text": s_str("Memory text (for add/update)"), "title": s_str("Short title (for add)"), "id": s_str("Memory id (for update/delete)") }), &["action"]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "memory" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::Benign }
    fn label(&self, i: &Value) -> Option<String> { arg_str(i, &["title", "text"]).map(|s| s.chars().take(60).collect()) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let root = ctx.root.as_deref();
        match arg_str(&input, &["action"]).unwrap_or("add") {
            "add" => { let text = arg_str(&input, &["text", "content"]).ok_or_else(|| CaretError::invalid("text is required"))?; let m = memories::add(root, text, arg_str(&input, &["title"]), "agent", Some(&ctx.session_id))?; Ok(ToolOutput::text(format!("Saved memory {} — {}", m.id, text))) }
            "update" => { let id = arg_str(&input, &["id"]).ok_or_else(|| CaretError::invalid("id is required"))?; let text = arg_str(&input, &["text"]).ok_or_else(|| CaretError::invalid("text is required"))?; memories::update(root, id, text)?; Ok(ToolOutput::text(format!("Updated memory {id}"))) }
            "delete" => { let id = arg_str(&input, &["id"]).ok_or_else(|| CaretError::invalid("id is required"))?; memories::delete(root, id)?; Ok(ToolOutput::text(format!("Deleted memory {id}"))) }
            other => Ok(ToolOutput::error(format!("Unknown action {other}"))),
        }
    }
}

pub struct FetchRules;
#[async_trait::async_trait]
impl Tool for FetchRules {
    fn name(&self) -> &str { "fetch_rules" }
    fn display_name(&self) -> &str { "Fetch rules" }
    fn description(&self) -> String { "Load the full text of project rules by name. The system prompt lists available agent-requested rules with descriptions; fetch the ones relevant to the current task before acting.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "rule_names": s_arr("Rule names/ids to fetch", json!({"type": "string"})) }), &["rule_names"]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "knowledge" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::ReadOnly }
    fn label(&self, i: &Value) -> Option<String> { i.get("rule_names").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let names: Vec<String> = input.get("rule_names").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        if names.is_empty() { return Ok(ToolOutput::error("rule_names is required")); }
        let all = rules::list(ctx.root.as_deref(), &ctx.settings);
        let mut out = String::new();
        for n in &names {
            let found = all.iter().find(|r| r.id == *n || r.name == *n || r.path.ends_with(n) || r.name.eq_ignore_ascii_case(n));
            match found { Some(r) => match rules::read(ctx.root.as_deref(), &r.id) { Ok(t) => out.push_str(&format!("<rule name=\"{}\">\n{}\n</rule>\n\n", r.name, t.trim())), Err(e) => out.push_str(&format!("(could not read rule {n}: {})\n", e.message)) }, None => out.push_str(&format!("(no rule named {n}; available: {})\n", all.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", "))) }
        }
        let (text, _) = cap_output(&out, ctx.max_output());
        Ok(ToolOutput::text(text))
    }
}

pub struct Skill;
#[async_trait::async_trait]
impl Tool for Skill {
    fn name(&self) -> &str { "skill" }
    fn display_name(&self) -> &str { "Load skill" }
    fn description(&self) -> String { "Load a skill's full instructions (SKILL.md) by name. The system prompt lists available skills; load one when the task matches its description and follow it.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "name": s_str("Skill name") }), &["name"]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "knowledge" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::ReadOnly }
    fn label(&self, i: &Value) -> Option<String> { arg_str(i, &["name"]).map(String::from) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let name = arg_str(&input, &["name", "skill"]).ok_or_else(|| CaretError::invalid("name is required"))?;
        let all = skills::list(ctx.root.as_deref());
        let Some(s) = all.iter().find(|s| s.name == name || s.id == name || s.name.eq_ignore_ascii_case(name)) else { return Ok(ToolOutput::error(format!("No skill named {name}. Available: {}", all.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")))) };
        let text = skills::read(ctx.root.as_deref(), &s.id)?;
        let (text, _) = cap_output(&format!("<skill name=\"{}\" path=\"{}\">\n{}\n</skill>", s.name, s.path, text.trim()), ctx.max_output());
        Ok(ToolOutput::text(text).with_summary(format!("Loaded skill {}", s.name)))
    }
}

pub struct AskQuestion;
#[async_trait::async_trait]
impl Tool for AskQuestion {
    fn name(&self) -> &str { "ask_question" }
    fn display_name(&self) -> &str { "Ask the user" }
    fn description(&self) -> String { "Pause and ask the user a clarifying question when requirements are ambiguous or a decision is theirs to make (e.g. which of two valid approaches). Offer concise options when possible. Do not ask when you can reasonably decide yourself.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "question": s_str("The question to ask"), "options": s_arr("Optional short answer options", json!({"type": "string"})), "allow_free_text": s_bool("Allow a free-form answer (default true)") }), &["question"]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "planning" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::Benign }
    fn label(&self, i: &Value) -> Option<String> { arg_str(i, &["question"]).map(|q| q.chars().take(80).collect()) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let q = arg_str(&input, &["question"]).ok_or_else(|| CaretError::invalid("question is required"))?.to_string();
        let options: Option<Vec<String>> = input.get("options").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect());
        let question = AgentQuestion { question_id: crate::ai::agent::session::new_id(), question: q, options, allow_free_text: arg_bool(&input, &["allow_free_text"]).unwrap_or(true) };
        let answer = ctx.manager.ask_question(&ctx.session_id, &ctx.message_id, question, &ctx.cancel).await?;
        Ok(ToolOutput::text(format!("User answered: {answer}")).with_summary(answer.chars().take(60).collect::<String>()))
    }
}

pub struct CreatePlan;
#[async_trait::async_trait]
impl Tool for CreatePlan {
    fn name(&self) -> &str { "create_plan" }
    fn display_name(&self) -> &str { "Create plan" }
    fn description(&self) -> String { "Write an implementation plan as a markdown document (saved under .caret/plans) for the user to review and approve before building. Include: goal, context discovered, numbered steps with target files, risks/open questions, and how to verify.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "title": s_str("Short plan title"), "content": s_str("Plan body in markdown") }), &["title", "content"]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "planning" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::Benign }
    fn label(&self, i: &Value) -> Option<String> { arg_str(i, &["title"]).map(String::from) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let title = arg_str(&input, &["title"]).ok_or_else(|| CaretError::invalid("title is required"))?;
        let content = arg_str(&input, &["content", "plan"]).ok_or_else(|| CaretError::invalid("content is required"))?;
        let root = ctx.root.clone().unwrap_or_else(|| ctx.cwd.clone());
        let path = plans::create(&root, title, content, Some(&ctx.session_id))?;
        ctx.manager.emit_plan(&ctx.session_id, &ctx.message_id, &path.to_string_lossy(), title);
        Ok(ToolOutput::text(format!("Plan saved to {}. Tell the user to review it; they can click Build to execute it.", path.display())).with_summary(format!("Plan: {title}")))
    }
}

pub struct EditPlan;
#[async_trait::async_trait]
impl Tool for EditPlan {
    fn name(&self) -> &str { "edit_plan" }
    fn display_name(&self) -> &str { "Edit plan" }
    fn description(&self) -> String { "Replace the content of an existing plan document (after user feedback).".into() }
    fn input_schema(&self) -> Value { schema(json!({ "path": s_str("Plan file path (as returned by create_plan)"), "content": s_str("New markdown content") }), &["path", "content"]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "planning" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::Benign }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let path = arg_str(&input, &["path"]).ok_or_else(|| CaretError::invalid("path is required"))?;
        let content = arg_str(&input, &["content"]).ok_or_else(|| CaretError::invalid("content is required"))?;
        let abs = ctx.resolve(path);
        plans::update(&abs.to_string_lossy(), content)?;
        ctx.manager.emit_plan(&ctx.session_id, &ctx.message_id, &abs.to_string_lossy(), content.lines().find(|l| l.starts_with("# ")).map(|l| l.trim_start_matches("# ")).unwrap_or("Plan"));
        Ok(ToolOutput::text(format!("Plan updated: {}", abs.display())))
    }
}

pub struct ReadLints;
#[async_trait::async_trait]
impl Tool for ReadLints {
    fn name(&self) -> &str { "read_lints" }
    fn display_name(&self) -> &str { "Read lints" }
    fn description(&self) -> String { "Read current linter / language-server diagnostics (errors and warnings) for the given files, or for the whole workspace when no paths are given. Use after editing to verify your changes compile.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "paths": s_arr("Files to check (default: all files with diagnostics)", json!({"type": "string"})) }), &[]) }
    fn readonly(&self) -> bool { true }
    fn group(&self) -> &str { "files" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::ReadOnly }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let paths: Option<Vec<String>> = input.get("paths").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).map(|p| ctx.resolve(p).to_string_lossy().to_string()).collect());
        let summary = match &ctx.app { Some(app) => app.state::<DiagnosticsStore>().summary(paths.as_deref(), 200), None => String::new() };
        if summary.is_empty() { return Ok(ToolOutput::text("No errors or warnings reported for the requested files (note: diagnostics only exist for files the language server has analysed/opened).")); }
        let n = summary.lines().count();
        let (text, _) = cap_output(&summary, ctx.max_output());
        Ok(ToolOutput::text(text).with_summary(format!("{n} diagnostic{}", if n == 1 { "" } else { "s" })))
    }
}

pub struct Task;
#[async_trait::async_trait]
impl Tool for Task {
    fn name(&self) -> &str { "task" }
    fn display_name(&self) -> &str { "Run subagent" }
    fn description(&self) -> String { "Delegate a self-contained task to a subagent that runs with its own context window and returns a report. Use for parallelizable research (e.g. \"find every call site of X and summarize\") or large exploration that would flood your context. Name a configured subagent to use its specialised instructions. The subagent cannot ask the user questions.".into() }
    fn input_schema(&self) -> Value { schema(json!({ "description": s_str("3-6 word summary shown to the user"), "prompt": s_str("Complete, self-contained instructions and what to report back"), "subagent": s_str("Optional name of a configured subagent (see system prompt)"), "readonly": s_bool("Restrict the subagent to read-only tools (default true unless the subagent config says otherwise)") }), &["description", "prompt"]) }
    fn readonly(&self) -> bool { false }
    fn group(&self) -> &str { "agents" }
    fn effect(&self, _i: &Value) -> ToolEffect { ToolEffect::Subagent }
    fn label(&self, i: &Value) -> Option<String> { arg_str(i, &["description"]).map(String::from) }
    async fn run(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let description = arg_str(&input, &["description"]).unwrap_or("Subagent task").to_string();
        let prompt = arg_str(&input, &["prompt"]).ok_or_else(|| CaretError::invalid("prompt is required"))?.to_string();
        let name = arg_str(&input, &["subagent", "subagent_type"]).map(String::from);
        let readonly = arg_bool(&input, &["readonly"]);
        let (sid, text) = ctx.manager.clone().run_subagent(ctx, &description, &prompt, name.as_deref(), readonly).await?;
        let (text, _) = cap_output(&text, ctx.max_output());
        Ok(ToolOutput { text: format!("Subagent report:\n{text}"), subagent_session_id: Some(sid), ..Default::default() }.with_summary(description))
    }
}

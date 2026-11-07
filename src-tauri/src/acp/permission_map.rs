//! Maps Caret permission modes onto an external agent's session modes or approval config options (pure functions).
//!
//! Known mode ids: Claude Code ACP `default` / `acceptEdits` / `plan` / `bypassPermissions`; Gemini CLI `default` /
//! `autoEdit` / `yolo`. Codex exposes its approval policy as a config option (`untrusted` / `on-request` /
//! `on-failure` / `never`), matched by lowercase substrings so other agents with similar vocab work too.
use crate::acp::protocol::{ConfigOption, SessionModeState};
use crate::ai::agent::policy::PermissionMode;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum AcpPermissionAction {
    SetMode(String),
    SetConfigOption { config_id: String, value: Value },
    None,
}

fn norm(s: &str) -> String { s.trim().to_lowercase().replace(['_', ' '], "-") }

fn accept_edits_id(ids: &[(String, String)]) -> Option<String> {
    ids.iter().find(|(n, _)| n.contains("acceptedit") || n.contains("accept-edit") || n.contains("autoedit") || n.contains("auto-edit")).map(|(_, o)| o.clone())
}

/// Pick the agent mode id that best matches `mode`, if the agent advertises one.
pub fn pick_mode_id(mode: PermissionMode, modes: &SessionModeState) -> Option<String> {
    let ids: Vec<(String, String)> = modes.available_modes.iter().map(|m| (norm(&m.id), m.id.clone())).collect();
    let find = |f: &dyn Fn(&str) -> bool| ids.iter().find(|(n, _)| f(n)).map(|(_, o)| o.clone());
    match mode {
        PermissionMode::Supervised => find(&|n| n == "default" || n == "supervised" || n == "ask" || n == "normal" || n == "manual"),
        PermissionMode::AutoAcceptEdits => accept_edits_id(&ids),
        PermissionMode::Auto => find(&|n| n == "auto" || (n.contains("auto") && !n.contains("edit"))).or_else(|| accept_edits_id(&ids)),
        PermissionMode::FullAccess => find(&|n| n.contains("bypass") || n.contains("yolo") || n.contains("full-access") || n.contains("fullaccess") || n.contains("danger")),
    }
}

/// Pick a value for the agent's approval/permission config option, if it has one.
pub fn pick_config_value(mode: PermissionMode, options: &[ConfigOption]) -> Option<(String, Value)> {
    let opt = options.iter().find(|o| { let k = norm(&format!("{} {}", o.id, o.name)); k.contains("approval") || k.contains("permission") })?;
    let keywords: &[&str] = match mode {
        PermissionMode::Supervised => &["untrusted", "default", "always", "ask", "supervised", "manual"],
        PermissionMode::AutoAcceptEdits => &["on-request", "accept", "edit", "request"],
        PermissionMode::Auto => &["on-failure", "auto", "failure", "on-request"],
        PermissionMode::FullAccess => &["never", "bypass", "yolo", "full", "danger"],
    };
    for kw in keywords {
        if let Some(c) = opt.options.iter().find(|c| norm(&c.value).contains(kw) || norm(&c.name).contains(kw)) {
            return Some((opt.id.clone(), Value::String(c.value.clone())));
        }
    }
    None
}

/// Session modes win over config options; `None` when the agent exposes nothing comparable.
pub fn plan(mode: PermissionMode, modes: Option<&SessionModeState>, options: Option<&[ConfigOption]>) -> AcpPermissionAction {
    if let Some(ms) = modes { if let Some(id) = pick_mode_id(mode, ms) { return AcpPermissionAction::SetMode(id); } }
    if let Some(o) = options { if let Some((config_id, value)) = pick_config_value(mode, o) { return AcpPermissionAction::SetConfigOption { config_id, value }; } }
    AcpPermissionAction::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::protocol::{ConfigOptionChoice, SessionMode};

    fn modes(ids: &[&str]) -> SessionModeState { SessionModeState { current_mode_id: ids[0].into(), available_modes: ids.iter().map(|i| SessionMode { id: i.to_string(), name: i.to_string(), description: None }).collect() } }
    fn opt(id: &str, name: &str, values: &[&str]) -> ConfigOption { ConfigOption { id: id.into(), name: name.into(), description: None, category: Some("mode".into()), kind: "select".into(), current_value: Value::String(values[0].into()), options: values.iter().map(|v| ConfigOptionChoice { value: v.to_string(), name: v.to_string(), description: None }).collect() } }

    #[test]
    fn claude_code_modes() {
        let m = modes(&["default", "acceptEdits", "plan", "bypassPermissions"]);
        assert_eq!(pick_mode_id(PermissionMode::Supervised, &m).as_deref(), Some("default"));
        assert_eq!(pick_mode_id(PermissionMode::AutoAcceptEdits, &m).as_deref(), Some("acceptEdits"));
        assert_eq!(pick_mode_id(PermissionMode::Auto, &m).as_deref(), Some("acceptEdits"), "no auto id: falls back to acceptEdits");
        assert_eq!(pick_mode_id(PermissionMode::FullAccess, &m).as_deref(), Some("bypassPermissions"));
    }

    #[test]
    fn gemini_modes() {
        let m = modes(&["default", "autoEdit", "yolo"]);
        assert_eq!(pick_mode_id(PermissionMode::Supervised, &m).as_deref(), Some("default"));
        assert_eq!(pick_mode_id(PermissionMode::AutoAcceptEdits, &m).as_deref(), Some("autoEdit"));
        assert_eq!(pick_mode_id(PermissionMode::Auto, &m).as_deref(), Some("autoEdit"));
        assert_eq!(pick_mode_id(PermissionMode::FullAccess, &m).as_deref(), Some("yolo"));
    }

    #[test]
    fn explicit_auto_id_wins_and_unknown_vocab_maps_to_nothing() {
        let m = modes(&["default", "acceptEdits", "auto", "bypassPermissions"]);
        assert_eq!(pick_mode_id(PermissionMode::Auto, &m).as_deref(), Some("auto"));
        let m = modes(&["ask", "code"]);
        assert_eq!(pick_mode_id(PermissionMode::Supervised, &m).as_deref(), Some("ask"));
        assert_eq!(pick_mode_id(PermissionMode::AutoAcceptEdits, &m), None);
        assert_eq!(pick_mode_id(PermissionMode::FullAccess, &m), None);
        assert_eq!(plan(PermissionMode::FullAccess, Some(&m), None), AcpPermissionAction::None);
    }

    #[test]
    fn codex_approval_policy_config_option() {
        let opts = vec![opt("model", "Model", &["gpt-5"]), opt("approval_policy", "Approval policy", &["untrusted", "on-failure", "on-request", "never"])];
        assert_eq!(pick_config_value(PermissionMode::Supervised, &opts), Some(("approval_policy".into(), Value::String("untrusted".into()))));
        assert_eq!(pick_config_value(PermissionMode::AutoAcceptEdits, &opts), Some(("approval_policy".into(), Value::String("on-request".into()))));
        assert_eq!(pick_config_value(PermissionMode::Auto, &opts), Some(("approval_policy".into(), Value::String("on-failure".into()))));
        assert_eq!(pick_config_value(PermissionMode::FullAccess, &opts), Some(("approval_policy".into(), Value::String("never".into()))));
        // no approval-ish option → nothing
        assert_eq!(pick_config_value(PermissionMode::Auto, &[opt("model", "Model", &["a"])]), None);
    }

    #[test]
    fn plan_prefers_modes_over_config_options() {
        let m = modes(&["default", "acceptEdits"]);
        let opts = vec![opt("permissions", "Permissions", &["ask", "never"])];
        assert_eq!(plan(PermissionMode::Supervised, Some(&m), Some(&opts)), AcpPermissionAction::SetMode("default".into()));
        // full access has no mode id here, so the config option is used
        assert_eq!(plan(PermissionMode::FullAccess, Some(&m), Some(&opts)), AcpPermissionAction::SetConfigOption { config_id: "permissions".into(), value: Value::String("never".into()) });
        assert_eq!(plan(PermissionMode::Auto, None, None), AcpPermissionAction::None);
    }
}

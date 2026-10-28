//! Deterministic tier assignment from the user's enabled models. Pure (settings in, `Assignment` out) so it is
//! unit-testable and cheap to recompute on every settings change.
use super::{Assignment, Tier, TierSlot};
use crate::core::settings::{ModelConfig, ProviderConfig, ProviderKind, Settings};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone)]
struct Cand<'a> { m: &'a ModelConfig, price: f64, price_known: bool, rank: u8, order: usize }
impl Cand<'_> {
    fn fast(&self) -> bool { self.m.is_fast == Some(true) || self.m.tags.as_ref().map(|t| t.iter().any(|x| x == "fast")).unwrap_or(false) }
}

/// Models that are themselves routers (OpenRouter Auto, Lynkr's `lynkr-auto`) must not be routed to again; pinned
/// virtual tiers such as `lynkr-complex` are fixed targets and stay eligible.
fn is_meta_router(m: &ModelConfig) -> bool {
    let id = m.model.to_ascii_lowercase();
    id.ends_with("/auto") || id == "lynkr-auto" || m.tags.as_ref().map(|t| t.iter().any(|x| x == "router")).unwrap_or(false)
}
fn cmp_f(a: f64, b: f64) -> std::cmp::Ordering { a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal) }

/// Blended $/M used for ordering: input + 3×output (agent turns are output-heavy). Local (Ollama) models cost 0.
fn blended_price(m: &ModelConfig, local: bool) -> (f64, bool) {
    if local { return (0.0, true); }
    let known = m.input_price_per_m.is_some() || m.output_price_per_m.is_some();
    (m.input_price_per_m.unwrap_or(0.0) + 3.0 * m.output_price_per_m.unwrap_or(0.0), known)
}

/// Assign SIMPLE / MEDIUM / COMPLEX / REASONING among the enabled, tool-capable models whose provider is enabled and
/// `usable` (has a key when it needs one). Overrides from `settings.router.overrides` apply last.
///
/// Rules (ties broken by larger context, larger max output, then catalog order):
/// 1. REASONING — best capability rank (effort > thinking > none), then highest price.
/// 2. SIMPLE    — cheapest model other than the REASONING pick, preferring fast-tagged models.
/// 3. COMPLEX   — strongest of the rest by price (preferring models with thinking); none left → REASONING.
/// 4. MEDIUM    — remaining model whose price is nearest sqrt(price_simple × price_complex); none left → COMPLEX.
/// Fewer than two candidates → no slots (the sidecar is not started) and a warning.
pub fn assign_tiers(settings: &Settings, usable: &dyn Fn(&ProviderConfig) -> bool) -> Assignment {
    let r = &settings.router;
    let excluded: HashSet<&str> = r.excluded_models.iter().map(|s| s.as_str()).collect();
    let providers: BTreeMap<&str, &ProviderConfig> = settings.providers.iter().map(|p| (p.id.as_str(), p)).collect();
    let mut warnings: Vec<String> = vec![];
    let mut cands: Vec<Cand> = vec![];
    for (order, m) in settings.models.iter().enumerate() {
        if !m.enabled || !m.supports_tools || excluded.contains(m.id.as_str()) || is_meta_router(m) { continue; }
        let Some(p) = providers.get(m.provider_id.as_str()) else { continue };
        if !p.enabled || !usable(p) { continue; }
        let local = matches!(p.kind, ProviderKind::Ollama);
        let (price, price_known) = blended_price(m, local);
        let rank = if m.supports_effort { 2 } else if m.supports_thinking { 1 } else { 0 };
        cands.push(Cand { m, price, price_known, rank, order });
    }
    // Unknown cloud prices → median of the known ones (flagged), so a missing price never dominates the ordering.
    let mut known: Vec<f64> = cands.iter().filter(|c| c.price_known).map(|c| c.price).collect();
    known.sort_by(|a, b| cmp_f(*a, *b));
    let median = if known.is_empty() { 0.0 } else { known[known.len() / 2] };
    for c in cands.iter_mut() { if !c.price_known { c.price = median; warnings.push(format!("{}: no price configured, assumed ${median:.2}/M for tier ordering", c.m.display_name)); } }

    let mut slots: BTreeMap<String, TierSlot> = BTreeMap::new();
    let n = cands.len();
    if n == 0 { warnings.push("Auto routing: no enabled model with a usable provider".into()); }
    else if n == 1 { warnings.push(format!("Auto routing needs at least 2 enabled models (only {} is usable)", cands[0].m.display_name)); }
    else {
        let by_max = |a: &Cand, b: &Cand, price_first: bool| {
            let cap = a.rank.cmp(&b.rank); let price = cmp_f(a.price, b.price);
            let head = if price_first { price.then(cap) } else { cap.then(price) };
            head.then(a.m.context_window.cmp(&b.m.context_window)).then(a.m.max_output.cmp(&b.m.max_output)).then(b.order.cmp(&a.order))
        };
        // 1. REASONING
        let reasoning = cands.iter().max_by(|a, b| by_max(a, b, false)).unwrap().clone();
        // 2. SIMPLE — cheapest model other than the reasoning pick (fast-tagged first among equals).
        let mut cheap: Vec<&Cand> = cands.iter().collect();
        cheap.sort_by(|a, b| cmp_f(a.price, b.price).then(b.fast().cmp(&a.fast())).then(b.m.context_window.cmp(&a.m.context_window)).then(a.order.cmp(&b.order)));
        let simple = cheap.iter().copied().find(|c| c.m.id != reasoning.m.id).or(cheap.first().copied()).unwrap().clone();
        // 3. COMPLEX — strongest of what is left (prefer models with thinking); nothing left → the reasoning pick.
        let rest: Vec<&Cand> = cands.iter().filter(|c| c.m.id != reasoning.m.id && c.m.id != simple.m.id).collect();
        let complex = if rest.is_empty() { reasoning.clone() } else {
            let thinking: Vec<&Cand> = rest.iter().copied().filter(|c| c.rank >= 1).collect();
            let pool = if thinking.is_empty() { rest.clone() } else { thinking };
            (*pool.iter().max_by(|a, b| by_max(a, b, true)).unwrap()).clone()
        };
        // 4. MEDIUM — remaining model nearest the geometric mean of the simple and complex prices; none → complex.
        let target = (simple.price.max(0.0) * complex.price.max(0.0)).sqrt();
        let remaining: Vec<&Cand> = cands.iter().filter(|c| c.m.id != simple.m.id && c.m.id != complex.m.id && c.m.id != reasoning.m.id).collect();
        let medium = remaining.iter().copied().min_by(|a, b| cmp_f((a.price - target).abs(), (b.price - target).abs()).then(b.m.context_window.cmp(&a.m.context_window)).then(a.order.cmp(&b.order))).cloned().unwrap_or_else(|| complex.clone());
        for (t, c, why) in [
            (Tier::Simple, &simple, "cheapest usable model"),
            (Tier::Medium, &medium, "between the simple and complex picks by price"),
            (Tier::Complex, &complex, "strongest non-reasoning pick by price and capability"),
            (Tier::Reasoning, &reasoning, "best thinking/effort support, then price"),
        ] { slots.insert(t.key().into(), TierSlot { model_id: c.m.id.clone(), source: "auto".into(), reason: Some(why.into()) }); }
    }

    // Overrides apply last; invalid ones are reported but kept in settings so they revive when the model returns.
    for t in Tier::ALL {
        let Some(id) = r.overrides.get(t.key()) else { continue };
        let ok = settings.model(id).and_then(|m| providers.get(m.provider_id.as_str()).map(|p| (m, *p)));
        match ok {
            Some((m, p)) if m.enabled && p.enabled && usable(p) => { slots.insert(t.key().into(), TierSlot { model_id: m.id.clone(), source: "override".into(), reason: None }); }
            Some((m, p)) => warnings.push(format!("{} override '{}' ignored: {}", t.upper(), id, if !m.enabled { "model is disabled" } else if !p.enabled { "provider is disabled" } else { "provider has no usable key" })),
            None => warnings.push(format!("{} override '{}' ignored: model not found", t.upper(), id)),
        }
    }
    if !slots.is_empty() && !Tier::ALL.iter().all(|t| slots.contains_key(t.key())) { slots.clear(); warnings.push("Auto routing: incomplete tier assignment".into()); }

    let mut hasher = blake3::Hasher::new();
    for t in Tier::ALL { if let Some(s) = slots.get(t.key()) { hasher.update(t.key().as_bytes()); hasher.update(b"="); hasher.update(s.model_id.as_bytes()); hasher.update(b";"); } }
    Assignment { slots, candidates: n, warnings, hash: hasher.finalize().to_hex()[..16].to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::settings::{ModelConfig, ProviderConfig, ProviderKind, Settings};

    fn p(id: &str, kind: ProviderKind) -> ProviderConfig { ProviderConfig { id: id.into(), kind, name: id.into(), enabled: true, api_key_ref: Some(id.into()), ..Default::default() } }
    fn m(id: &str, provider: &str, inp: f64, out: f64, thinking: bool, effort: bool, ctx: u32) -> ModelConfig {
        ModelConfig { id: id.into(), provider_id: provider.into(), model: id.into(), display_name: id.into(), context_window: ctx, max_output: 16_000, supports_tools: true, supports_vision: false, supports_thinking: thinking, supports_effort: effort, input_price_per_m: Some(inp), output_price_per_m: Some(out), enabled: true, ..Default::default() }
    }
    fn settings(models: Vec<ModelConfig>) -> Settings { let mut s = Settings::default(); s.providers = vec![p("anthropic", ProviderKind::Anthropic), p("openai", ProviderKind::Openai), p("ollama", ProviderKind::Ollama)]; s.models = models; s }
    fn all_usable(_: &ProviderConfig) -> bool { true }
    fn slot(a: &Assignment, t: Tier) -> &str { a.get(t).map(|s| s.model_id.as_str()).unwrap_or("") }

    #[test]
    fn four_distinct_models_fill_four_tiers() {
        let s = settings(vec![m("opus", "anthropic", 4.0, 20.0, true, true, 1_000_000), m("sonnet", "anthropic", 2.0, 10.0, true, true, 1_000_000), m("haiku", "anthropic", 1.0, 5.0, true, false, 200_000), m("mini", "openai", 0.25, 2.0, false, false, 400_000)]);
        let a = assign_tiers(&s, &all_usable);
        assert_eq!(a.candidates, 4);
        assert_eq!(slot(&a, Tier::Reasoning), "opus");
        assert_eq!(slot(&a, Tier::Complex), "sonnet");
        assert_eq!(slot(&a, Tier::Simple), "mini");
        assert_eq!(slot(&a, Tier::Medium), "haiku");
        assert!(a.complete()); assert!(a.warnings.is_empty()); assert_eq!(a.hash.len(), 16);
    }

    #[test]
    fn two_models_split_cheap_and_strong() {
        let s = settings(vec![m("opus", "anthropic", 4.0, 20.0, true, true, 1_000_000), m("haiku", "anthropic", 1.0, 5.0, true, false, 200_000)]);
        let a = assign_tiers(&s, &all_usable);
        assert_eq!(slot(&a, Tier::Simple), "haiku"); assert_eq!(slot(&a, Tier::Medium), "opus"); assert_eq!(slot(&a, Tier::Complex), "opus"); assert_eq!(slot(&a, Tier::Reasoning), "opus");
    }

    #[test]
    fn three_models_medium_falls_to_complex() {
        let s = settings(vec![m("opus", "anthropic", 4.0, 20.0, true, true, 1_000_000), m("sonnet", "anthropic", 2.0, 10.0, true, true, 1_000_000), m("haiku", "anthropic", 1.0, 5.0, true, false, 200_000)]);
        let a = assign_tiers(&s, &all_usable);
        assert_eq!((slot(&a, Tier::Simple), slot(&a, Tier::Medium), slot(&a, Tier::Complex), slot(&a, Tier::Reasoning)), ("haiku", "sonnet", "sonnet", "opus"));
    }

    #[test]
    fn one_or_zero_models_yield_no_slots() {
        let a = assign_tiers(&settings(vec![m("opus", "anthropic", 4.0, 20.0, true, true, 1_000_000)]), &all_usable);
        assert!(a.slots.is_empty()); assert_eq!(a.candidates, 1); assert!(a.warnings[0].contains("at least 2"));
        let a0 = assign_tiers(&settings(vec![]), &all_usable);
        assert!(a0.slots.is_empty()); assert!(a0.warnings[0].contains("no enabled model"));
    }

    #[test]
    fn unusable_disabled_excluded_and_meta_models_are_skipped() {
        let mut s = settings(vec![m("opus", "anthropic", 4.0, 20.0, true, true, 1_000_000), m("gpt", "openai", 1.25, 10.0, true, false, 400_000), m("haiku", "anthropic", 1.0, 5.0, true, false, 200_000), m("auto", "openai", 0.0, 0.0, false, false, 1), m("off", "anthropic", 9.0, 9.0, true, true, 1)]);
        s.models[3].model = "openrouter/auto".into(); s.models[4].enabled = false; s.router.excluded_models = vec!["haiku".into()];
        let a = assign_tiers(&s, &|p| p.id != "openai");
        assert_eq!(a.candidates, 1, "only opus survives: gpt unusable, haiku excluded, auto meta, off disabled");
        assert!(a.slots.is_empty());
    }

    #[test]
    fn unknown_price_uses_median_and_warns() {
        let mut s = settings(vec![m("opus", "anthropic", 4.0, 20.0, true, true, 1_000_000), m("mystery", "openai", 0.0, 0.0, false, false, 100_000), m("haiku", "anthropic", 1.0, 5.0, true, false, 200_000)]);
        s.models[1].input_price_per_m = None; s.models[1].output_price_per_m = None;
        let a = assign_tiers(&s, &all_usable);
        assert!(a.warnings.iter().any(|w| w.contains("mystery") && w.contains("no price")));
        assert_eq!(slot(&a, Tier::Simple), "haiku", "unknown price must not be treated as free");
    }

    #[test]
    fn local_models_are_free_and_become_simple() {
        let mut s = settings(vec![m("opus", "anthropic", 4.0, 20.0, true, true, 1_000_000), m("qwen", "ollama", 0.0, 0.0, false, false, 32_000)]);
        s.models[1].input_price_per_m = None; s.models[1].output_price_per_m = None;
        let a = assign_tiers(&s, &all_usable);
        assert_eq!(slot(&a, Tier::Simple), "qwen"); assert!(a.warnings.is_empty());
    }

    #[test]
    fn overrides_apply_last_and_invalid_ones_are_reported() {
        let mut s = settings(vec![m("opus", "anthropic", 4.0, 20.0, true, true, 1_000_000), m("sonnet", "anthropic", 2.0, 10.0, true, true, 1_000_000), m("haiku", "anthropic", 1.0, 5.0, true, false, 200_000), m("mini", "openai", 0.25, 2.0, false, false, 400_000), m("off", "anthropic", 9.0, 9.0, true, true, 1)]);
        s.models[4].enabled = false;
        s.router.overrides = [("simple".to_string(), "sonnet".to_string()), ("complex".to_string(), "off".to_string()), ("medium".to_string(), "nope".to_string())].into_iter().collect();
        let a = assign_tiers(&s, &all_usable);
        assert_eq!(slot(&a, Tier::Simple), "sonnet"); assert_eq!(a.get(Tier::Simple).unwrap().source, "override");
        assert_eq!(slot(&a, Tier::Complex), "sonnet", "disabled override ignored → auto pick kept");
        assert!(a.warnings.iter().any(|w| w.contains("COMPLEX override 'off'") && w.contains("disabled")));
        assert!(a.warnings.iter().any(|w| w.contains("MEDIUM override 'nope'") && w.contains("not found")));
    }

    #[test]
    fn hash_changes_only_with_the_mapping() {
        let s = settings(vec![m("opus", "anthropic", 4.0, 20.0, true, true, 1_000_000), m("haiku", "anthropic", 1.0, 5.0, true, false, 200_000)]);
        let a = assign_tiers(&s, &all_usable); let b = assign_tiers(&s, &all_usable);
        assert_eq!(a.hash, b.hash);
        let mut s2 = s.clone(); s2.router.overrides.insert("simple".into(), "opus".into());
        assert_ne!(assign_tiers(&s2, &all_usable).hash, a.hash);
    }
}

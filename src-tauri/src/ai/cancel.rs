//! Request-id → CancellationToken registry shared by Tab, Cmd+K, review, etc.
use dashmap::DashMap;
use once_cell::sync::Lazy;
use tokio_util::sync::CancellationToken;

static TOKENS: Lazy<DashMap<String, CancellationToken>> = Lazy::new(DashMap::new);

/// Register (or replace) a token for `id`. Replacing cancels the previous one.
pub fn register(id: &str) -> CancellationToken {
    let t = CancellationToken::new();
    if let Some((_, old)) = TOKENS.remove(id) { old.cancel(); }
    TOKENS.insert(id.to_string(), t.clone());
    t
}
pub fn cancel(id: &str) -> bool { if let Some((_, t)) = TOKENS.remove(id) { t.cancel(); true } else { false } }
pub fn unregister(id: &str) { TOKENS.remove(id); }
pub fn is_active(id: &str) -> bool { TOKENS.contains_key(id) }

/// RAII guard: unregisters on drop.
pub struct Guard(pub String, pub CancellationToken);
impl Guard { pub fn new(id: &str) -> Self { Self(id.to_string(), register(id)) } pub fn token(&self) -> CancellationToken { self.1.clone() } }
impl Drop for Guard { fn drop(&mut self) { TOKENS.remove(&self.0); } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn register_cancel_replace() {
        let t1 = register("a");
        let t2 = register("a");
        assert!(t1.is_cancelled());
        assert!(!t2.is_cancelled());
        assert!(cancel("a"));
        assert!(t2.is_cancelled());
        assert!(!cancel("a"));
        { let g = Guard::new("b"); assert!(is_active("b")); let _ = g.token(); }
        assert!(!is_active("b"));
    }
}

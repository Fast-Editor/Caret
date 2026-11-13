//! Monotonic generation counter for work started by a workspace open.
//!
//! Opening a folder fans out to background tasks (file watcher, quick-open index, codebase index). When the user
//! switches folders again before those finish, each task compares the ticket it was started with against the
//! current generation and stops instead of installing results for a root that is no longer open.
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct Generation(AtomicU64);

impl Generation {
    pub const fn new() -> Self { Self(AtomicU64::new(0)) }
    /// Start a new generation and return its ticket. Every ticket handed out earlier is stale from now on.
    pub fn begin(&self) -> u64 { self.0.fetch_add(1, Ordering::AcqRel) + 1 }
    /// `true` while no newer generation has begun since `ticket` was issued.
    pub fn is_current(&self, ticket: u64) -> bool { self.0.load(Ordering::Acquire) == ticket }
    pub fn current(&self) -> u64 { self.0.load(Ordering::Acquire) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tickets_are_monotonic_and_only_the_latest_is_current() {
        let g = Generation::new();
        assert_eq!(g.current(), 0);
        let a = g.begin();
        assert!(g.is_current(a));
        let b = g.begin();
        assert!(b > a);
        assert!(!g.is_current(a), "an older open is superseded");
        assert!(g.is_current(b));
        assert_eq!(g.current(), b);
    }

    #[test]
    fn a_stale_ticket_never_becomes_current_again() {
        let g = Generation::new();
        let a = g.begin();
        for _ in 0..5 { g.begin(); }
        assert!(!g.is_current(a));
        assert!(!g.is_current(0), "the pre-open generation is not a valid ticket once anything began");
    }

    #[test]
    fn is_shared_across_threads() {
        let g = std::sync::Arc::new(Generation::new());
        let handles: Vec<_> = (0..8).map(|_| { let g = g.clone(); std::thread::spawn(move || g.begin()) }).collect();
        let mut tickets: Vec<u64> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        tickets.sort_unstable(); tickets.dedup();
        assert_eq!(tickets.len(), 8, "every thread got a distinct ticket");
        assert_eq!(g.current(), 8);
    }
}

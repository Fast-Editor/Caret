//! Per-root table behind the codebase index: which roots are open, the generation ticket that guards delayed work
//! started for a root (the 1.2 s first-run delay, a queued run), and which open root owns a path.
//!
//! Windows showing the same folder share one entry; `IndexManager` opens an entry on `first_for_root` and closes it
//! on `last_for_prev_root` (see docs/MULTI_WINDOW.md). The table is generic so its lifecycle rules can be tested
//! without a tantivy store or an `AppHandle`.
use crate::core::generation::Generation;
use dashmap::DashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What the table needs from an entry: its generation counter (bumped when the root closes) and its root.
pub trait RootResource: Send + Sync + 'static {
    fn generation(&self) -> &Generation;
    fn root(&self) -> &Path;
}

pub struct RootTable<T: RootResource> { entries: DashMap<PathBuf, Arc<T>> }

impl<T: RootResource> Default for RootTable<T> { fn default() -> Self { Self::new() } }

impl<T: RootResource> RootTable<T> {
    pub fn new() -> Self { Self { entries: DashMap::new() } }

    /// Open `root`: `Some((entry, ticket))` when it was not open (the entry is built with `make` and its first
    /// generation begins), `None` when it already is (a second window joined the folder; nothing to start).
    pub fn open(&self, root: &Path, make: impl FnOnce() -> T) -> Option<(Arc<T>, u64)> {
        use dashmap::mapref::entry::Entry;
        match self.entries.entry(root.to_path_buf()) {
            Entry::Occupied(_) => None,
            Entry::Vacant(v) => { let t = Arc::new(make()); let ticket = t.generation().begin(); v.insert(t.clone()); Some((t, ticket)) }
        }
    }

    /// Close `root`: removes it and bumps its generation so delayed work holding an older ticket stops. Returns the
    /// entry so the caller can cancel its running work; `None` when the root was not open.
    pub fn close(&self, root: &Path) -> Option<Arc<T>> {
        let (_, t) = self.entries.remove(root)?;
        t.generation().begin();
        Some(t)
    }

    pub fn get(&self, root: &Path) -> Option<Arc<T>> { self.entries.get(root).map(|e| e.value().clone()) }
    pub fn contains(&self, root: &Path) -> bool { self.entries.contains_key(root) }

    /// `true` while `entry` is still the open entry for its root (not closed, not replaced by a reopen) and no newer
    /// generation began since `ticket` was issued. Delayed work checks this before touching the stores.
    pub fn is_current(&self, entry: &Arc<T>, ticket: u64) -> bool {
        self.get(entry.root()).map(|cur| Arc::ptr_eq(&cur, entry)).unwrap_or(false) && entry.generation().is_current(ticket)
    }

    /// The open root that is `path` or its deepest ancestor (a nested checkout open in another window wins over its
    /// parent for files inside it).
    pub fn owner_of(&self, path: &Path) -> Option<Arc<T>> {
        self.entries.iter().filter(|e| path.starts_with(e.key())).max_by_key(|e| e.key().components().count()).map(|e| e.value().clone())
    }

    /// Group `paths` by the open root that owns each; paths under no open root are dropped.
    pub fn group_by_owner(&self, paths: Vec<PathBuf>) -> Vec<(Arc<T>, Vec<PathBuf>)> {
        let mut out: Vec<(Arc<T>, Vec<PathBuf>)> = Vec::new();
        for p in paths {
            let Some(owner) = self.owner_of(&p) else { continue };
            match out.iter_mut().find(|(o, _)| Arc::ptr_eq(o, &owner)) { Some((_, v)) => v.push(p), None => out.push((owner, vec![p])) }
        }
        out
    }

    pub fn roots(&self) -> Vec<PathBuf> { let mut v: Vec<PathBuf> = self.entries.iter().map(|e| e.key().clone()).collect(); v.sort(); v }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Probe { root: PathBuf, generation: Generation }
    impl RootResource for Probe { fn generation(&self) -> &Generation { &self.generation } fn root(&self) -> &Path { &self.root } }
    fn probe(root: &str) -> impl FnOnce() -> Probe { let r = PathBuf::from(root); move || Probe { root: r, generation: Generation::new() } }

    #[test]
    fn two_roots_open_independently_and_closing_one_keeps_the_other() {
        let t: RootTable<Probe> = RootTable::new();
        let (a, ta) = t.open(Path::new("/work/a"), probe("/work/a")).expect("a opens");
        let (b, tb) = t.open(Path::new("/work/b"), probe("/work/b")).expect("b opens");
        assert_eq!(t.roots(), vec![PathBuf::from("/work/a"), PathBuf::from("/work/b")]);
        assert!(t.open(Path::new("/work/a"), probe("/work/a")).is_none(), "a second window on /work/a starts nothing");
        assert_eq!(t.len(), 2);
        assert!(t.is_current(&a, ta) && t.is_current(&b, tb));

        let closed = t.close(Path::new("/work/a")).expect("a was open");
        assert!(Arc::ptr_eq(&closed, &a));
        assert!(!t.contains(Path::new("/work/a")));
        assert!(t.contains(Path::new("/work/b")), "closing a leaves b alone");
        assert!(t.get(Path::new("/work/b")).map(|e| Arc::ptr_eq(&e, &b)).unwrap_or(false));
        assert!(t.close(Path::new("/work/a")).is_none(), "closing twice is a no-op");
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn generation_tickets_are_per_root() {
        let t: RootTable<Probe> = RootTable::new();
        let (a, ta) = t.open(Path::new("/work/a"), probe("/work/a")).unwrap();
        let (b, tb) = t.open(Path::new("/work/b"), probe("/work/b")).unwrap();
        // closing a invalidates a's delayed work (its ticket is stale) but not b's
        t.close(Path::new("/work/a"));
        assert!(!a.generation().is_current(ta), "the entry's own counter was bumped on close");
        assert!(!t.is_current(&a, ta));
        assert!(t.is_current(&b, tb), "b's ticket is untouched");
        // reopening a gives a fresh entry: the old entry's ticket never becomes current again even though the new
        // entry hands out the same ticket number
        let (a2, ta2) = t.open(Path::new("/work/a"), probe("/work/a")).unwrap();
        assert!(!Arc::ptr_eq(&a, &a2));
        assert_eq!(ta, ta2, "ticket numbers restart per entry");
        assert!(!t.is_current(&a, ta), "the stale entry is not the open one");
        assert!(t.is_current(&a2, ta2));
        // a newer generation on the same entry supersedes the older ticket
        let newer = a2.generation().begin();
        assert!(!t.is_current(&a2, ta2));
        assert!(t.is_current(&a2, newer));
    }

    #[test]
    fn paths_are_owned_by_the_deepest_open_root() {
        let t: RootTable<Probe> = RootTable::new();
        t.open(Path::new("/work/a"), probe("/work/a")).unwrap();
        t.open(Path::new("/work/a/vendor/lib"), probe("/work/a/vendor/lib")).unwrap();
        t.open(Path::new("/work/b"), probe("/work/b")).unwrap();
        let owner = |p: &str| t.owner_of(Path::new(p)).map(|e| e.root().to_path_buf());
        assert_eq!(owner("/work/a/src/main.rs"), Some(PathBuf::from("/work/a")));
        assert_eq!(owner("/work/a/vendor/lib/x.c"), Some(PathBuf::from("/work/a/vendor/lib")), "the nested root wins for its files");
        assert_eq!(owner("/work/a"), Some(PathBuf::from("/work/a")), "the root itself");
        assert_eq!(owner("/work/ab/x.ts"), None, "a sibling with a common prefix is not inside");
        assert_eq!(owner("/elsewhere/x"), None);
        let groups = t.group_by_owner(vec![PathBuf::from("/work/b/1"), PathBuf::from("/work/a/2"), PathBuf::from("/work/b/3"), PathBuf::from("/nope/4")]);
        let mut summary: Vec<(PathBuf, usize)> = groups.iter().map(|(o, ps)| (o.root().to_path_buf(), ps.len())).collect(); summary.sort();
        assert_eq!(summary, vec![(PathBuf::from("/work/a"), 1), (PathBuf::from("/work/b"), 2)]);
    }
}

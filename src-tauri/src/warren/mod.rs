//! Warren sandboxes (docs/WARREN.md): every agent thread can run its commands in a persistent Linux instance on a
//! synced copy of the workspace. This module owns the HTTP client (`client`), the Caret-facing and wire types
//! (`types`), the workspace sync (`sync`) and the `warren_*` commands (`commands`). The executor that routes
//! `run_terminal_cmd` through an instance and the thread binding commands build on these.
//!
//! [`WarrenState`] caches one [`WarrenClient`] per `(apiUrl, connectVia, key)` fingerprint; a settings change drops
//! the cache (`init`), and a new key misses it on its own because the key is part of the fingerprint.
pub mod client;
pub mod commands;
pub mod sync;
pub mod types;

pub use client::WarrenClient;
pub use commands::create_instance_for;
pub use types::{SandboxBinding, SandboxStatus, SyncReport, WarrenExecResult, WarrenInstance, WarrenStatus};

use crate::core::settings::{SettingsState, WarrenSettings};
use crate::error::Result;
use parking_lot::Mutex;
use tauri::{AppHandle, Listener, Manager};

/// Managed state: the cached client and the fingerprint it was built for.
#[derive(Default)]
pub struct WarrenState { cached: Mutex<Option<(String, WarrenClient)>> }

impl WarrenState {
    /// The client for the current settings and key, reusing the cached one when nothing relevant changed.
    pub fn client(&self, settings: &SettingsState) -> Result<WarrenClient> {
        let (warren, key) = resolve(settings);
        let fp = fingerprint(&warren, key.as_deref());
        if let Some((cached_fp, c)) = self.cached.lock().as_ref() { if *cached_fp == fp { return Ok(c.clone()); } }
        let c = build(&warren, key.as_deref())?;
        *self.cached.lock() = Some((fp, c.clone()));
        Ok(c)
    }
    /// Forget the cached client (settings changed).
    pub fn invalidate(&self) { *self.cached.lock() = None; }
}

/// The Warren settings and the key stored under `warren.apiKeyRef` (`None` when no key is set).
pub fn resolve(settings: &SettingsState) -> (WarrenSettings, Option<String>) {
    let warren = settings.effective().warren;
    let key = settings.secret(&warren.api_key_ref).map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
    (warren, key)
}

fn fingerprint(w: &WarrenSettings, key: Option<&str>) -> String {
    let mut h = blake3::Hasher::new();
    h.update(w.api_url.as_bytes()); h.update(b"\0");
    h.update(w.connect_via.as_deref().unwrap_or("").as_bytes()); h.update(b"\0");
    h.update(key.unwrap_or("").as_bytes());
    h.finalize().to_hex()[..32].to_string()
}

fn build(w: &WarrenSettings, key: Option<&str>) -> Result<WarrenClient> { WarrenClient::new(&w.api_url, key.unwrap_or(""), w.connect_via.as_deref()) }

/// A fresh client for the current settings: the URL and `connectVia` from `warren`, the key from
/// `settings.secret(warren.apiKeyRef)` (an empty key still yields a client, good for `health`). Not cached; the
/// commands and the executor prefer [`cached_client`].
pub fn client_for(settings: &SettingsState) -> Result<WarrenClient> { let (w, key) = resolve(settings); build(&w, key.as_deref()) }

/// The cached client of the running app (see [`WarrenState::client`]).
pub fn cached_client(app: &AppHandle) -> Result<WarrenClient> {
    let settings = app.state::<SettingsState>();
    match app.try_state::<WarrenState>() { Some(st) => st.client(&settings), None => client_for(&settings) }
}

/// Drop the cached client whenever settings change.
pub fn init(app: &AppHandle) {
    let handle = app.clone();
    app.listen(crate::events::SETTINGS_CHANGED, move |_| { if let Some(st) = handle.try_state::<WarrenState>() { st.invalidate(); } });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_changes_with_url_tunnel_or_key() {
        let w = WarrenSettings::default();
        let a = fingerprint(&w, Some("sk_live_a"));
        assert_eq!(a, fingerprint(&w, Some("sk_live_a")));
        assert_ne!(a, fingerprint(&w, Some("sk_live_b")));
        assert_ne!(a, fingerprint(&w, None));
        let mut via = w.clone(); via.connect_via = Some("127.0.0.1:7337".into());
        assert_ne!(a, fingerprint(&via, Some("sk_live_a")));
        let mut url = w.clone(); url.api_url = "http://other:1/v1".into();
        assert_ne!(a, fingerprint(&url, Some("sk_live_a")));
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn build_uses_the_settings() {
        let mut w = WarrenSettings::default();
        w.connect_via = Some("127.0.0.1:7337".into());
        let c = build(&w, Some("sk_live_x")).unwrap();
        assert_eq!(c.api_url(), "http://api.warren.localhost:7337/v1");
        assert!(c.has_key());
        assert!(!build(&w, None).unwrap().has_key());
    }
}

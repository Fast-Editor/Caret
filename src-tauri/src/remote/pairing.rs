//! Pairing codes and the paired-device store.
//!
//! * A pairing code is six digits, valid for five minutes, one-time use. Redeeming it is rate limited to five
//!   attempts per minute per client IP so the code cannot be brute-forced over the LAN in its lifetime.
//! * A successful pairing mints a random 256-bit token for the browser; only its SHA-256 is stored, in
//!   `~/.caret/remote/devices.json` (mode 0600) as `{ id, name, token_hash, created_at, last_seen, user_agent }`.
use crate::error::{CaretError, Result};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const CODE_TTL: Duration = Duration::from_secs(5 * 60);
pub const MAX_ATTEMPTS_PER_MINUTE: usize = 5;
const ATTEMPT_WINDOW: Duration = Duration::from_secs(60);

pub fn now_ms() -> i64 { chrono::Utc::now().timestamp_millis() }

fn random_u32() -> u32 { let b = *uuid::Uuid::new_v4().as_bytes(); u32::from_le_bytes([b[0], b[1], b[2], b[3]]) }
/// Six decimal digits, zero padded.
pub fn random_code() -> String { format!("{:06}", random_u32() % 1_000_000) }
/// 64 hex characters (two v4 UUIDs, 244 random bits).
pub fn random_token() -> String { format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple()) }

pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut s = String::with_capacity(64);
    for b in digest { s.push_str(&format!("{b:02x}")); }
    s
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ───────────────────────── pairing codes ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairingCode {
    pub code: String,
    /// Unix milliseconds.
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairError { Invalid, RateLimited }

struct ActiveCode { code: String, issued: Instant, expires_at_ms: i64 }

#[derive(Default)]
pub struct Pairing {
    active: Mutex<Option<ActiveCode>>,
    attempts: Mutex<HashMap<IpAddr, Vec<Instant>>>,
}

impl Pairing {
    /// Issue a new code (replacing any outstanding one).
    pub fn generate(&self) -> PairingCode { self.generate_at(Instant::now(), now_ms()) }
    pub fn generate_at(&self, now: Instant, now_ms: i64) -> PairingCode {
        let code = random_code();
        let expires_at = now_ms + CODE_TTL.as_millis() as i64;
        *self.active.lock() = Some(ActiveCode { code: code.clone(), issued: now, expires_at_ms: expires_at });
        PairingCode { code, expires_at }
    }

    /// The outstanding code when it has not expired.
    pub fn current(&self) -> Option<PairingCode> { self.current_at(Instant::now()) }
    pub fn current_at(&self, now: Instant) -> Option<PairingCode> {
        let g = self.active.lock();
        g.as_ref().filter(|a| now.duration_since(a.issued) < CODE_TTL).map(|a| PairingCode { code: a.code.clone(), expires_at: a.expires_at_ms })
    }

    /// Consume the code. Every call counts as an attempt for `ip`, including rejected ones.
    pub fn redeem(&self, code: &str, ip: IpAddr) -> std::result::Result<(), PairError> { self.redeem_at(code, ip, Instant::now()) }
    pub fn redeem_at(&self, code: &str, ip: IpAddr, now: Instant) -> std::result::Result<(), PairError> {
        {
            let mut all = self.attempts.lock();
            all.retain(|_, v| v.iter().any(|t| now.duration_since(*t) < ATTEMPT_WINDOW));
            let v = all.entry(ip).or_default();
            v.retain(|t| now.duration_since(*t) < ATTEMPT_WINDOW);
            if v.len() >= MAX_ATTEMPTS_PER_MINUTE { return Err(PairError::RateLimited); }
            v.push(now);
        }
        let mut g = self.active.lock();
        let ok = g.as_ref().map(|a| now.duration_since(a.issued) < CODE_TTL && constant_time_eq(a.code.as_bytes(), code.trim().as_bytes())).unwrap_or(false);
        if ok { *g = None; Ok(()) } else { Err(PairError::Invalid) }
    }
}

// ───────────────────────── devices ─────────────────────────

/// On-disk record (snake_case, token hash only).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    pub created_at: i64,
    pub last_seen: i64,
    #[serde(default)] pub user_agent: String,
}

/// What the UI sees (camelCase, no hash).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo { pub id: String, pub name: String, pub created_at: i64, pub last_seen: i64, pub user_agent: String }

impl From<&Device> for DeviceInfo {
    fn from(d: &Device) -> Self { Self { id: d.id.clone(), name: d.name.clone(), created_at: d.created_at, last_seen: d.last_seen, user_agent: d.user_agent.clone() } }
}

pub struct DeviceStore {
    path: PathBuf,
    devices: RwLock<Vec<Device>>,
    /// `last_seen` is persisted at most once a minute per store (every asset request authenticates).
    last_seen_flush: Mutex<Option<Instant>>,
}

impl DeviceStore {
    pub fn default_path() -> PathBuf { crate::paths::sub_dir("remote").join("devices.json") }

    pub fn load(path: PathBuf) -> Self {
        let devices: Vec<Device> = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { path, devices: RwLock::new(devices), last_seen_flush: Mutex::new(None) }
    }

    pub fn path(&self) -> &Path { &self.path }
    pub fn len(&self) -> usize { self.devices.read().len() }
    pub fn is_empty(&self) -> bool { self.len() == 0 }
    pub fn list(&self) -> Vec<DeviceInfo> { self.devices.read().iter().map(DeviceInfo::from).collect() }

    fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() { std::fs::create_dir_all(dir)?; }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&*self.devices.read())?)?;
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600)); }
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// Register a device and return its record plus the plaintext token (shown to the browser once).
    pub fn add(&self, name: &str, user_agent: &str) -> Result<(DeviceInfo, String)> {
        let name = name.trim();
        let name = if name.is_empty() { "Browser" } else { name };
        let token = random_token();
        let now = now_ms();
        let d = Device { id: uuid::Uuid::new_v4().simple().to_string(), name: name.chars().take(80).collect(), token_hash: hash_token(&token), created_at: now, last_seen: now, user_agent: user_agent.chars().take(200).collect() };
        let info = DeviceInfo::from(&d);
        self.devices.write().push(d);
        self.save()?;
        Ok((info, token))
    }

    /// Look a token up; touches `last_seen` (flushed to disk at most once a minute).
    pub fn authenticate(&self, token: &str) -> Option<DeviceInfo> {
        if token.is_empty() { return None; }
        let hash = hash_token(token);
        let info = {
            let mut list = self.devices.write();
            let d = list.iter_mut().find(|d| constant_time_eq(d.token_hash.as_bytes(), hash.as_bytes()))?;
            d.last_seen = now_ms();
            DeviceInfo::from(&*d)
        };
        let flush = { let mut g = self.last_seen_flush.lock(); let due = g.map(|t| t.elapsed() > Duration::from_secs(60)).unwrap_or(true); if due { *g = Some(Instant::now()); } due };
        if flush { if let Err(e) = self.save() { tracing::debug!(error = %e, "remote: could not persist last_seen"); } }
        Some(info)
    }

    /// Remove a device; `Ok(false)` when the id is unknown.
    pub fn revoke(&self, id: &str) -> Result<bool> {
        let removed = { let mut list = self.devices.write(); let before = list.len(); list.retain(|d| d.id != id); before != list.len() };
        if removed { self.save()?; }
        Ok(removed)
    }
}

impl std::fmt::Debug for DeviceStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_struct("DeviceStore").field("path", &self.path).field("devices", &self.len()).finish() }
}

#[allow(dead_code)]
fn _assert_error_conv(e: std::io::Error) -> CaretError { e.into() }

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip(n: u8) -> IpAddr { IpAddr::V4(Ipv4Addr::new(10, 0, 0, n)) }

    #[test]
    fn codes_are_six_digits_and_expire() {
        let p = Pairing::default();
        let t0 = Instant::now();
        let c = p.generate_at(t0, 1_000);
        assert_eq!(c.code.len(), 6);
        assert!(c.code.bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(c.expires_at, 1_000 + 300_000);
        assert_eq!(p.current_at(t0 + Duration::from_secs(299)).as_ref().map(|c| c.code.as_str()), Some(c.code.as_str()));
        assert!(p.current_at(t0 + CODE_TTL).is_none(), "expired code is not current");
        assert_eq!(p.redeem_at(&c.code, ip(1), t0 + CODE_TTL), Err(PairError::Invalid), "expired code cannot be redeemed");
    }

    #[test]
    fn codes_are_one_time_and_whitespace_tolerant() {
        let p = Pairing::default();
        let t0 = Instant::now();
        let c = p.generate_at(t0, 0);
        assert_eq!(p.redeem_at("000000", ip(1), t0), Err(PairError::Invalid));
        assert_eq!(p.redeem_at(&format!(" {} ", c.code), ip(1), t0 + Duration::from_secs(1)), Ok(()));
        assert_eq!(p.redeem_at(&c.code, ip(1), t0 + Duration::from_secs(2)), Err(PairError::Invalid), "second use fails");
        assert!(p.current_at(t0 + Duration::from_secs(2)).is_none(), "redeemed code is gone");
        // a new code replaces the old one
        let c2 = p.generate_at(t0, 0);
        assert_ne!(p.current_at(t0).map(|c| c.code), None);
        assert_eq!(p.redeem_at(&c2.code, ip(2), t0), Ok(()));
    }

    #[test]
    fn attempts_are_limited_per_ip_per_minute() {
        let p = Pairing::default();
        let t0 = Instant::now();
        let c = p.generate_at(t0, 0);
        for i in 0..MAX_ATTEMPTS_PER_MINUTE { assert_eq!(p.redeem_at("111111", ip(1), t0 + Duration::from_millis(i as u64)), Err(PairError::Invalid)); }
        // sixth attempt within the minute is throttled, even with the right code
        assert_eq!(p.redeem_at(&c.code, ip(1), t0 + Duration::from_secs(1)), Err(PairError::RateLimited));
        // another IP is unaffected
        assert_eq!(p.redeem_at("222222", ip(2), t0 + Duration::from_secs(1)), Err(PairError::Invalid));
        // the window slides: a minute after the first attempts the IP may try again
        assert_eq!(p.redeem_at(&c.code, ip(1), t0 + Duration::from_secs(61)), Ok(()));
    }

    #[test]
    fn device_store_round_trips_and_authenticates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remote").join("devices.json");
        let store = DeviceStore::load(path.clone());
        assert!(store.is_empty());
        let (info, token) = store.add("  iPad  ", "Mozilla/5.0 Safari").unwrap();
        assert_eq!(info.name, "iPad");
        assert_eq!(token.len(), 64);
        assert!(path.exists());
        #[cfg(unix)]
        { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600); }
        // only the hash is on disk
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains(&token));
        assert!(raw.contains(&hash_token(&token)));
        assert!(raw.contains("\"token_hash\"") && raw.contains("\"created_at\"") && raw.contains("\"user_agent\""));
        // reload
        let again = DeviceStore::load(path.clone());
        assert_eq!(again.len(), 1);
        assert_eq!(again.authenticate(&token).map(|d| d.id), Some(info.id.clone()));
        assert!(again.authenticate("nope").is_none());
        assert!(again.authenticate("").is_none());
        // revoke
        assert!(again.revoke(&info.id).unwrap());
        assert!(!again.revoke(&info.id).unwrap());
        assert!(again.authenticate(&token).is_none());
        assert_eq!(DeviceStore::load(path).len(), 0);
    }

    #[test]
    fn hash_is_sha256_hex() {
        assert_eq!(hash_token("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_ne!(random_token(), random_token());
        assert_ne!(random_code(), "");
    }
}

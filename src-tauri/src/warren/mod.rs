//! Warren sandboxes (docs/WARREN.md).
//!
//! PART B STAND-IN. Part A owns this file together with `types.rs`, `client.rs`, `sync.rs` and `commands.rs`; the
//! versions in this tree only carry the documented signatures (`unimplemented!()` bodies) so part B compiles. The
//! integrator replaces them with part A's files and keeps the block below.
pub mod types;
pub mod client;
pub mod sync;
pub mod commands;

// REGISTER:modules:warren-exec (part B — paste into part A's `warren/mod.rs`)
pub mod adapter;
pub mod executor;
pub mod binding;

/// Stand-in for part A's `client_for`: a client for the API URL / key / connect-via in `settings.warren`.
pub fn client_for(_settings: &crate::core::settings::Settings) -> crate::error::Result<client::WarrenClient> {
    unimplemented!("part A: warren::client_for")
}

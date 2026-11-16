//! PART B STAND-IN for part A's `warren/commands.rs`: only `create_instance_for`, which `binding.rs` calls through
//! `adapter.rs`. The integrator replaces this file with part A's.
use super::types::WarrenInstance;
use crate::core::settings::Settings;
use crate::error::Result;

/// Create an instance named `name` with the defaults in `settings.warren` (template, shape, auto-sleep, idle timeout).
pub async fn create_instance_for(_settings: &Settings, _name: &str) -> Result<WarrenInstance> { unimplemented!("part A: commands::create_instance_for") }

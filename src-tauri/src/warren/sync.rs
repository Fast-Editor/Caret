//! PART B STAND-IN for part A's `warren/sync.rs`: the types and functions part B calls (through `adapter.rs`), with
//! the shapes docs/WARREN.md lists and `unimplemented!()` bodies. The integrator replaces this file with part A's.
use super::client::WarrenClient;
use super::types::SandboxBinding;
use crate::error::Result;
use std::collections::BTreeMap;
use std::path::Path;

/// Relative path → (size, mtime ms) of the files last pushed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Manifest(pub BTreeMap<String, (u64, u64)>);

#[derive(Debug, Clone, Default)]
pub struct PushPlan { pub upload: Vec<String>, pub delete: Vec<String> }

#[derive(Debug, Clone, Default)]
pub struct PushReport { pub files: usize, pub bytes: u64, pub deleted: usize }

#[derive(Debug, Clone, Default)]
pub struct PullReport { pub changed: Vec<(String, Vec<u8>)>, pub deleted: Vec<String> }

pub fn scan(_local_root: &Path, _excludes: &[String]) -> Result<Manifest> { unimplemented!("part A: sync::scan") }
pub fn plan_push(_prev: Option<&Manifest>, _now: &Manifest) -> PushPlan { unimplemented!("part A: sync::plan_push") }
pub async fn push(_client: &WarrenClient, _binding: &SandboxBinding, _local_root: &Path, _plan: &PushPlan) -> Result<PushReport> { unimplemented!("part A: sync::push") }
pub async fn pull(_client: &WarrenClient, _binding: &SandboxBinding, _local_root: &Path, _pushed: &Manifest) -> Result<PullReport> { unimplemented!("part A: sync::pull") }

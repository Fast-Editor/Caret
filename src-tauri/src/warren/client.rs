//! PART B STAND-IN for part A's `warren/client.rs`: the methods part B calls, bodies `unimplemented!()`.
//! The integrator replaces this file with part A's.
use super::types::{ExecRequest, ExecResult, WarrenInstance};
use crate::error::Result;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct WarrenClient { _private: () }

impl WarrenClient {
    pub fn new(_api_url: &str, _api_key: &str, _connect_via: Option<&str>) -> Result<Self> { unimplemented!("part A: WarrenClient::new") }
    pub async fn instance(&self, _id: &str) -> Result<WarrenInstance> { unimplemented!("part A: WarrenClient::instance") }
    pub async fn delete_instance(&self, _id: &str) -> Result<()> { unimplemented!("part A: WarrenClient::delete_instance") }
    pub async fn exec(&self, _id: &str, _req: &ExecRequest) -> Result<ExecResult> { unimplemented!("part A: WarrenClient::exec") }
    pub async fn wait_running(&self, _id: &str, _timeout: Duration) -> Result<WarrenInstance> { unimplemented!("part A: WarrenClient::wait_running") }
}

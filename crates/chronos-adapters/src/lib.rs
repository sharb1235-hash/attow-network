//! Owned execution adapters and a separate durable-ticket effect gateway.
mod contract;
mod storage;
pub use contract::*;
pub use storage::AdapterStore;
#[cfg(feature = "browser")]
pub mod browser;
pub mod evidence;
pub mod gateway;
pub mod provider_simulator;
pub mod sql;

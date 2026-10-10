//! Scheduler composed with local durable result retention.
pub use chronos_scheduler::*;
#[cfg(feature = "local")]
mod storage;
#[cfg(feature = "local")]
pub use storage::SqliteResultRepository;

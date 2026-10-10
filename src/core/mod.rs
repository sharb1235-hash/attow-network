//! Core state and local-authority modules.
#[cfg(feature = "local")]
pub mod persistence;
#[cfg(feature = "local")]
pub use chronos_ledger as ledger;
pub use chronos_state as state;
pub use chronos_state::{
    Branch, BranchId, Dependency, DomainId, FrozenBranch, Mutation, StateError, StateRoot, Version,
    Worldline, fork,
};

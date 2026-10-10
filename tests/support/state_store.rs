//! Exercise the same local persistence composition exposed to SDK callers.

pub use attow_network::core::persistence::{
    LocalStateStorage as StoreAdapter, StoredStateView as StoreView,
};

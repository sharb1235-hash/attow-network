//! Shared local composition for the facade and installed CLI.
//! This crate does not introduce authority into portable state or transport.
pub mod core;
pub mod owned_loopback;
#[cfg(feature = "peer")]
pub mod peer;
#[cfg(feature = "sandbox")]
pub mod sandbox;
#[cfg(feature = "scheduler")]
pub mod scheduler;

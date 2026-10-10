//! Private authenticated QUIC object exchange. Authority is an injected application port.
mod authority;
mod endpoint;
mod objects;
mod session;
pub use authority::{DenyProposals, ProposalContext, ProposalHandler};
use chronos_codec::ObjectId;
pub use endpoint::PeerNode;
pub use iroh::SecretKey;
pub use objects::{ObjectRepository, SqliteObjectRepository, TransferObject};
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("invalid private peer configuration")]
    Configuration,
    #[error("peer or scope is not authorized")]
    Unauthorized,
    #[error("peer transport is unavailable")]
    Unavailable,
    #[error("peer operation exceeded its deadline")]
    Timeout,
    #[error("peer quota is exhausted")]
    Busy,
    #[error("peer transfer exceeds its bounds")]
    Limit,
    #[error("invalid peer protocol state")]
    Protocol,
    #[error("object graph is invalid or incomplete")]
    InvalidGraph,
    #[error("peer object persistence failed")]
    Storage,
    #[error("application rejected the peer proposal")]
    Application,
}
impl From<chronos_store::StoreError> for TransportError {
    fn from(_: chronos_store::StoreError) -> Self {
        Self::Storage
    }
}
impl From<chronos_protocol::ProtocolError> for TransportError {
    fn from(_: chronos_protocol::ProtocolError) -> Self {
        Self::Protocol
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Scope {
    pub tenant: ObjectId,
    pub domain: ObjectId,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct PeerPermissions {
    pub objects: bool,
    pub proposals: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct PeerAddress {
    pub peer: [u8; 32],
    pub address: SocketAddr,
}
#[derive(Clone, Debug)]
pub struct TransportLimits {
    pub protocol: chronos_protocol::ProtocolLimits,
    pub max_batch_bytes: usize,
    pub bulk_bytes: usize,
    pub control_bytes: usize,
    pub bulk_streams: usize,
    pub control_streams: usize,
    pub deadline_ms: u64,
}
impl Default for TransportLimits {
    fn default() -> Self {
        Self {
            protocol: Default::default(),
            max_batch_bytes: 8 * 1024 * 1024,
            bulk_bytes: 16 * 1024 * 1024,
            control_bytes: 2 * 1024 * 1024,
            bulk_streams: 2,
            control_streams: 8,
            deadline_ms: 10_000,
        }
    }
}
pub struct PeerConfig {
    pub secret_key: SecretKey,
    pub bind_addr: SocketAddr,
    pub tenant: ObjectId,
    pub domain: ObjectId,
    pub permissions: BTreeMap<[u8; 32], PeerPermissions>,
    pub limits: TransportLimits,
    pub objects: Arc<dyn ObjectRepository>,
    pub authority: Arc<dyn ProposalHandler>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncReport {
    pub roots: Vec<ObjectId>,
    pub objects: usize,
    pub bytes: u64,
    pub durable: bool,
}

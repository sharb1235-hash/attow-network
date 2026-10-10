//! Bounded versioned peer data, independent of sockets, execution and authority policy.
mod checkpoint;
mod frame;
mod limits;
mod messages;
mod version;

pub use checkpoint::{Checkpoint, CheckpointTrust};
pub use ed25519_dalek::SigningKey;
pub use frame::{FrameHeader, decode_frame, decode_header, encode_frame};
pub use limits::{MAX_CHUNK_BYTES, MAX_CONTROL_BYTES, MAX_IDS, MAX_OBJECT_BYTES, ProtocolLimits};
pub use messages::{
    Ack, Chunk, ErrorMessage, Hello, Message, PromotionReply, PromotionStatus, Proposal,
    ProposalEnvelope, ResultMessage,
};
pub use version::{ALPN, PROTOCOL_VERSION};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("invalid protocol limits")]
    InvalidLimits,
    #[error("protocol resource limit exceeded")]
    Limit,
    #[error("truncated frame")]
    Truncated,
    #[error("trailing frame bytes")]
    TrailingBytes,
    #[error("unsupported protocol version")]
    Version,
    #[error("unknown message type")]
    UnknownMessage,
    #[error("invalid protocol message")]
    InvalidMessage,
    #[error("invalid checkpoint signature")]
    Signature,
    #[error("checkpoint trust context mismatch")]
    Trust,
    #[error("stale checkpoint")]
    StaleCheckpoint,
    #[error(transparent)]
    Codec(#[from] chronos_codec::CodecError),
}

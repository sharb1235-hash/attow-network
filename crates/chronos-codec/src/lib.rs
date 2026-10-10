//! Bounded, deterministic object encoding and content identity for Attow Network.

use std::collections::{BTreeMap, BTreeSet};

mod decode;
mod encode;
pub mod execution;
mod ids;
mod limits;

pub use ids::{ObjectId, object_id};
pub use limits::{HARD_MAX_BYTES, HARD_MAX_DEPTH, HARD_MAX_ITEMS, Limits};

/// Stable error categories; errors intentionally do not contain object contents.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodecError {
    #[error("invalid codec limits")]
    InvalidLimits,
    #[error("object byte limit exceeded")]
    ByteLimit,
    #[error("nesting depth limit exceeded")]
    DepthLimit,
    #[error("item count limit exceeded")]
    ItemLimit,
    #[error("truncated CBOR input")]
    Truncated,
    #[error("noncanonical CBOR encoding")]
    NonCanonical,
    #[error("duplicate map key or object reference")]
    Duplicate,
    #[error("trailing bytes after canonical object")]
    TrailingBytes,
    #[error("unsupported CBOR type")]
    UnsupportedType,
    #[error("invalid UTF-8 text")]
    InvalidText,
    #[error("invalid object envelope")]
    InvalidObject,
    #[error("unknown object kind or schema")]
    UnknownSchema,
    #[error("invalid object identifier")]
    InvalidId,
    #[error("encoding failed")]
    Encode,
}

/// The deliberately restricted, unambiguous CBOR value model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    Unsigned(u64),
    /// Encodes the integer `-1 - n`, allowing CBOR's complete negative range.
    Negative(u64),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Value>),
    Map(BTreeMap<String, Value>),
}

/// The registered generic immutable object kind.
pub const OBJECT_KIND: u16 = 1;
/// Version one of the generic object envelope.
pub const OBJECT_SCHEMA: u16 = 1;
pub(crate) const OBJECT_TAG: u64 = 0x4154_544f;

/// A versioned object whose references can be traversed safely by the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Object {
    pub refs: BTreeSet<ObjectId>,
    pub payload: Value,
}

pub trait Canonical: Sized {
    fn encode(&self) -> Result<Vec<u8>, CodecError>;
    fn decode(bytes: &[u8], limits: &Limits) -> Result<Self, CodecError>;
}

impl Canonical for Value {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.encode_with_limits(&Limits::default())
    }
    fn decode(bytes: &[u8], limits: &Limits) -> Result<Self, CodecError> {
        decode::value(bytes, limits)
    }
}

impl Canonical for Object {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.encode_with_limits(&Limits::default())
    }
    fn decode(bytes: &[u8], limits: &Limits) -> Result<Self, CodecError> {
        decode::object(bytes, limits)
    }
}

impl Value {
    /// Encode after checking the entire value against explicit resource limits.
    pub fn encode_with_limits(&self, limits: &Limits) -> Result<Vec<u8>, CodecError> {
        encode::value(self, limits)
    }
}

impl Object {
    /// Encode after checking the envelope and payload against resource limits.
    pub fn encode_with_limits(&self, limits: &Limits) -> Result<Vec<u8>, CodecError> {
        encode::object(self, limits)
    }
}

/// Decode references only for an explicitly registered object schema.
pub fn references(
    kind: u16,
    schema: u16,
    bytes: &[u8],
    limits: &Limits,
) -> Result<Vec<ObjectId>, CodecError> {
    if (kind, schema) != (OBJECT_KIND, OBJECT_SCHEMA) {
        return Err(CodecError::UnknownSchema);
    }
    Ok(Object::decode(bytes, limits)?.refs.into_iter().collect())
}

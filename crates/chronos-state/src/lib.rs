//! Immutable state, tracked speculative proposals, and durable admission ports.
//! Branches carry no permission to spend rights or dispatch external effects.

mod branch;
mod dependency;
mod diff;
pub mod execution;
mod root;
mod worldline;

pub use branch::{Branch, FrozenBranch, fork};
pub use dependency::{Dependency, Mutation, Version};
pub use diff::{Conflict, Difference, JoinOutcome, JoinedProposal, diff, join};
pub use root::{Cell, StateRoot};
pub use worldline::verify_graph as verify_object_graph;
pub use worldline::{ObjectData, StateReader, StateStorage, StateTransaction, Worldline};

use chronos_codec::{CodecError, Value};

pub const MAX_KEYS: usize = 4096;
pub const MAX_KEY_BYTES: usize = 1024;
pub const MAX_VALUE_BYTES: usize = 64 * 1024;
pub const MAX_STATE_BYTES: usize = 512 * 1024;
pub const MAX_DEPENDENCIES: usize = 4096;
pub const MAX_ANCESTRY: usize = 64;
pub const MAX_GRAPH_DEPTH: usize = 64;
pub const MAX_GRAPH_OBJECTS: usize = 4096;
pub const MAX_GRAPH_BYTES: usize = 16 * 1024 * 1024;
pub type ScanRows = Vec<(Vec<u8>, Vec<u8>)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DomainId(pub [u8; 32]);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BranchId(pub [u8; 32]);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StateError {
    #[error("invalid state encoding")]
    InvalidEncoding,
    #[error("authority domain mismatch")]
    DomainMismatch,
    #[error("unsupported state schema")]
    UnsupportedSchema,
    #[error("state resource limit exceeded: {0}")]
    Limit(&'static str),
    #[error("state sequence exhausted")]
    VersionExhausted,
    #[error("storage operation failed")]
    Storage,
    #[error("object identity or reference set is corrupt")]
    CorruptObject,
    #[error(transparent)]
    Codec(#[from] CodecError),
}

pub(crate) fn check_key(key: &[u8]) -> Result<(), StateError> {
    if key.len() > MAX_KEY_BYTES {
        return Err(StateError::Limit("key bytes"));
    }
    Ok(())
}
pub(crate) fn check_value(value: &[u8]) -> Result<(), StateError> {
    if value.len() > MAX_VALUE_BYTES {
        return Err(StateError::Limit("value bytes"));
    }
    Ok(())
}
pub(crate) fn array(value: Value, length: usize) -> Result<Vec<Value>, StateError> {
    match value {
        Value::Array(values) if values.len() == length => Ok(values),
        _ => Err(StateError::InvalidEncoding),
    }
}
pub(crate) fn bytes(value: Value) -> Result<Vec<u8>, StateError> {
    match value {
        Value::Bytes(bytes) => Ok(bytes),
        _ => Err(StateError::InvalidEncoding),
    }
}
pub(crate) fn number(value: Value) -> Result<u64, StateError> {
    match value {
        Value::Unsigned(number) => Ok(number),
        _ => Err(StateError::InvalidEncoding),
    }
}
pub(crate) fn id_bytes(value: Value) -> Result<[u8; 32], StateError> {
    bytes(value)?
        .try_into()
        .map_err(|_| StateError::InvalidEncoding)
}
pub(crate) fn next(fields: &mut impl Iterator<Item = Value>) -> Result<Value, StateError> {
    fields.next().ok_or(StateError::InvalidEncoding)
}
pub(crate) fn check_tag(value: Value, expected: &str) -> Result<(), StateError> {
    if value != Value::Text(expected.into()) {
        return Err(StateError::InvalidEncoding);
    }
    Ok(())
}
pub(crate) fn check_schema(value: Value) -> Result<(), StateError> {
    if number(value)? != 1 {
        return Err(StateError::UnsupportedSchema);
    }
    Ok(())
}

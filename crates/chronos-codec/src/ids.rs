use crate::CodecError;
use std::{fmt, str::FromStr};

/// Content identity, independent of authority or execution history.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(pub [u8; 32]);

impl fmt::Display for ObjectId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for ObjectId {
    type Err = CodecError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(CodecError::InvalidId);
        }
        let mut bytes = [0; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
                .map_err(|_| CodecError::InvalidId)?;
        }
        Ok(Self(bytes))
    }
}

/// Hash a domain-separated kind, schema, length, and payload. This function does
/// not assert that the bytes are canonical; callers validate their own schema.
pub fn object_id(kind: u16, schema: u16, bytes: &[u8]) -> ObjectId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"attow-network/object/v1\0");
    hasher.update(&kind.to_be_bytes());
    hasher.update(&schema.to_be_bytes());
    hasher.update(&(bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
    ObjectId(*hasher.finalize().as_bytes())
}

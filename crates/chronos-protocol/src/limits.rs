use crate::ProtocolError;

pub const MAX_CONTROL_BYTES: usize = 1024 * 1024;
pub const MAX_CHUNK_BYTES: usize = 64 * 1024;
pub const MAX_OBJECT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_IDS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtocolLimits {
    pub control_bytes: usize,
    pub chunk_bytes: usize,
    pub object_bytes: u64,
    pub max_ids: usize,
}
impl Default for ProtocolLimits {
    fn default() -> Self {
        Self {
            control_bytes: MAX_CONTROL_BYTES,
            chunk_bytes: MAX_CHUNK_BYTES,
            object_bytes: MAX_OBJECT_BYTES,
            max_ids: MAX_IDS,
        }
    }
}
impl ProtocolLimits {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.control_bytes == 0
            || self.control_bytes > MAX_CONTROL_BYTES
            || self.chunk_bytes == 0
            || self.chunk_bytes > MAX_CHUNK_BYTES
            || self.object_bytes == 0
            || self.object_bytes > MAX_OBJECT_BYTES
            || self.max_ids == 0
            || self.max_ids > MAX_IDS
        {
            return Err(ProtocolError::InvalidLimits);
        }
        Ok(())
    }
}

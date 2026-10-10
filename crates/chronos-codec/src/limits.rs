use crate::CodecError;

pub const HARD_MAX_BYTES: usize = 64 * 1024 * 1024;
pub const HARD_MAX_DEPTH: usize = 128;
pub const HARD_MAX_ITEMS: usize = 1_000_000;

/// Caller limits, additionally constrained by implementation hard caps. Depth
/// counts the root as one; items count every container, scalar, and map key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub object_bytes: usize,
    pub depth: usize,
    pub items: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            object_bytes: 1024 * 1024,
            depth: 64,
            items: 100_000,
        }
    }
}

impl Limits {
    pub fn validate(&self) -> Result<(), CodecError> {
        if self.object_bytes == 0
            || self.object_bytes > HARD_MAX_BYTES
            || self.depth == 0
            || self.depth > HARD_MAX_DEPTH
            || self.items == 0
            || self.items > HARD_MAX_ITEMS
        {
            return Err(CodecError::InvalidLimits);
        }
        Ok(())
    }
}

pub(crate) struct Budget<'a> {
    pub limits: &'a Limits,
    pub items: usize,
    pub bytes: usize,
}

impl<'a> Budget<'a> {
    pub fn new(limits: &'a Limits) -> Result<Self, CodecError> {
        limits.validate()?;
        Ok(Self {
            limits,
            items: 0,
            bytes: 0,
        })
    }

    pub fn ensure_items(&self, count: usize) -> Result<(), CodecError> {
        if count > self.limits.items.saturating_sub(self.items) {
            return Err(CodecError::ItemLimit);
        }
        Ok(())
    }

    pub fn node(&mut self, depth: usize) -> Result<(), CodecError> {
        if depth > self.limits.depth {
            return Err(CodecError::DepthLimit);
        }
        self.ensure_items(1)?;
        self.items += 1;
        Ok(())
    }

    pub fn add_bytes(&mut self, count: usize) -> Result<(), CodecError> {
        self.bytes = self.bytes.checked_add(count).ok_or(CodecError::ByteLimit)?;
        if self.bytes > self.limits.object_bytes {
            return Err(CodecError::ByteLimit);
        }
        Ok(())
    }
}

use crate::{ScheduleError, ScheduleLimits};

pub(crate) const MAX_RUNS: usize = 256;
pub(crate) const MAX_TRACE_EVENTS: usize = 16_384;
pub(crate) const EVENT_BYTES: u64 = 1024;
pub(crate) const CACHE_METADATA_BYTES: u64 = 1024;

pub(crate) fn validate(limits: ScheduleLimits) -> Result<(), ScheduleError> {
    if limits.resident_workers == 0
        || limits.resident_workers > 256
        || limits.queued_branches == 0
        || limits.queued_branches > 16_384
        || limits.retained_bytes == 0
        || limits.retained_bytes > 4 * 1024 * 1024 * 1024
    {
        Err(ScheduleError::InvalidLimits)
    } else {
        Ok(())
    }
}

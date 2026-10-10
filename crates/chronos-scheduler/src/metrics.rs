/// Telemetry is not replay authority and is excluded from semantic traces.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchedulerMetrics {
    pub submitted_nodes: usize,
    pub executed: usize,
    pub reused: usize,
    pub cache_hits: usize,
    pub shared_hits: usize,
    pub cancelled: usize,
    pub peak_resident: usize,
    pub retained_bytes: u64,
}

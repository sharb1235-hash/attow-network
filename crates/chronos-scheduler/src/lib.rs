//! Bounded graph scheduling with exact, admission-checked pure reuse.
mod budget;
pub mod cache;
mod engine;
pub mod graph;
pub mod invalidate;
mod metrics;
mod queue;
pub mod replay;
mod trace_codec;

pub use cache::EvaluationKey;
use chronos_codec::ObjectId;
pub use chronos_state::execution::{
    EffectClass, ExecutionError, ResourceClass, StepOutcome, StepRequest, WorkerBackend,
};
pub use engine::Scheduler;
pub use graph::{GraphNode, WorldlineGraph};
pub use metrics::SchedulerMetrics;
pub use replay::{RecordedInputs, ReplayError, ReplayReport, replay};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    #[error("invalid scheduling limits")]
    InvalidLimits,
    #[error("invalid bounded dependency graph")]
    InvalidGraph,
    #[error("invalid scheduling request")]
    InvalidRequest,
    #[error("scheduler resource limit exceeded: {0}")]
    Limit(&'static str),
    #[error("run is unknown or forgotten")]
    UnknownRun,
    #[error("graph node is unknown")]
    UnknownNode,
    #[error("operation requires a quiescent scheduler or run")]
    Busy,
    #[error("scheduler synchronization failed")]
    Poisoned,
    #[error("worker admission failed: {0}")]
    Backend(ExecutionError),
    #[error("result retention failed: {0}")]
    Repository(ExecutionError),
    #[error("asynchronous runtime is unavailable")]
    RuntimeUnavailable,
    #[error("scheduler identity generation failed")]
    Random,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduleLimits {
    pub resident_workers: usize,
    pub queued_branches: usize,
    pub retained_bytes: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId(pub u64);

/// Trusted persistence port. Bound the reachable graph before publishing pins.
/// Returning success means the rebound outcome and its complete graph are durable.
pub trait ResultRepository: Send + Sync {
    fn retain(
        &self,
        owner: &str,
        request: &StepRequest,
        outcome: &StepOutcome,
        max_bytes: u64,
    ) -> Result<u64, ExecutionError>;
    fn release(&self, owner: &str) -> Result<(), ExecutionError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvaluationSource {
    Worker,
    Shared,
    Cache,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticEvent {
    Scheduled {
        node: ObjectId,
        request: StepRequest,
        source: EvaluationSource,
    },
    Completed {
        node: ObjectId,
        outcome: StepOutcome,
    },
    Failed {
        node: ObjectId,
        error: ExecutionError,
    },
    Cancelled {
        node: ObjectId,
    },
    ObservationReplaced {
        node: ObjectId,
        observation_root: ObjectId,
        revision: u64,
        invalidated: Vec<ObjectId>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionTrace {
    pub graph: WorldlineGraph,
    pub events: Vec<SemanticEvent>,
}
#[derive(Clone, Debug)]
pub struct RunReport {
    pub outcomes: BTreeMap<ObjectId, StepOutcome>,
    pub failures: BTreeMap<ObjectId, ExecutionError>,
    pub cancelled: BTreeSet<ObjectId>,
    /// Nodes parked behind a suspended dependency hold no resident worker.
    pub pending: BTreeSet<ObjectId>,
    pub trace: ExecutionTrace,
    pub metrics: SchedulerMetrics,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancelReport {
    pub cancelled: usize,
    pub in_flight: usize,
    pub effects_revoked: usize,
}

pub type BackendRegistry = BTreeMap<ResourceClass, Arc<dyn WorkerBackend>>;

pub(crate) const MAX_REPLACEMENT_WORK: usize = 1_000_000;

//! Bounded component steps with durable explicit continuations and no ambient IO.
mod admission;
pub mod continuation;
mod host;
pub mod profile;
mod worker;

use chronos_codec::{CodecError, ObjectId};
use chronos_state::{ObjectData, execution::EffectClass};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, atomic::AtomicUsize},
};
use wasmtime::{Engine, component::Component};

pub use continuation::{Continuation, MAX_DATA_BYTES};
pub use profile::ExecutionProfile;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SandboxError {
    #[error("invalid or unsupported execution profile")]
    InvalidProfile,
    #[error("component is outside the admitted subset")]
    Admission,
    #[error("sandbox resource limit exceeded: {0}")]
    Limit(&'static str),
    #[error(transparent)]
    Codec(#[from] CodecError),
    #[error("sandbox repository operation failed")]
    Storage,
    #[error("required sandbox object is missing")]
    MissingObject(ObjectId),
    #[error("invalid immutable sandbox object")]
    InvalidObject,
    #[error("continuation does not match the code and profile")]
    InvalidContinuation,
    #[error("invalid explicit guest input")]
    InvalidInput,
    #[error("guest execution trapped")]
    Trap,
    #[error("guest fuel exhausted")]
    Fuel,
    #[error("guest host-call limit exceeded")]
    HostCall,
    #[error("guest output limit exceeded")]
    Output,
    #[error("worker request is outside its registered admission")]
    UnsupportedRequest,
    #[error("sandbox synchronization failed")]
    Poisoned,
}

/// A trusted storage port. `commit` must atomically insert the hash-checked
/// objects, verify the complete graph reachable from roots, and pin those roots.
/// It must return an error without changing visibility when any step fails.
pub trait SandboxRepository: Send + Sync {
    fn get(&self, id: ObjectId) -> Result<ObjectData, SandboxError>;
    fn commit(
        &self,
        owner: &str,
        objects: &[ObjectData],
        roots: &[ObjectId],
    ) -> Result<(), SandboxError>;
}

#[derive(Clone)]
pub struct AdmittedCode {
    pub(crate) component: Component,
    pub(crate) id: ObjectId,
}
impl AdmittedCode {
    pub fn id(&self) -> ObjectId {
        self.id
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepInput {
    pub code: ObjectId,
    pub state: ObjectId,
    pub continuation: ObjectId,
    pub observation: Option<ObjectId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepExit {
    Done {
        result: ObjectId,
        delta: ObjectId,
    },
    Suspended {
        continuation: ObjectId,
        request: ObjectId,
        delta: ObjectId,
    },
}

/// Host-installed exact admission tuple. It is not supplied by guest code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerAdmission {
    pub tenant: ObjectId,
    pub security_scope: ObjectId,
    pub policy: ObjectId,
    pub fidelity: ObjectId,
    pub code: ObjectId,
    pub profile: ObjectId,
    pub effect_class: EffectClass,
}

pub(crate) struct CodeCache {
    pub codes: BTreeMap<ObjectId, AdmittedCode>,
    pub bytes: usize,
}
pub struct Executor {
    pub(crate) engine: Engine,
    pub(crate) repository: Arc<dyn SandboxRepository>,
    pub(crate) cache: Mutex<CodeCache>,
    pub(crate) admissions: Mutex<Vec<WorkerAdmission>>,
    pub(crate) active: AtomicUsize,
}

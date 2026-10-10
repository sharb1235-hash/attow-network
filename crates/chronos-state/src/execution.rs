//! Object-safe worker ports with no runtime, transport or store dependency.
//!
//! Run blocking methods on bounded worker threads, never on an async reactor.
//! The composing application resolves immutable objects and injects backends.
//! This port cannot authorize an external effect; returned requests are proposals.
//! A future resident backend can add a genuine opaque-handle lifecycle when its
//! implementation needs one. Handles must never be treated as portable checkpoints.

use std::{error::Error, fmt};

use chronos_codec::ObjectId;
pub use chronos_codec::execution::{
    EffectClass, ResourceBudget, ResourceClass, StepExit, StepOutcome, StepRequest,
};

/// Incomplete attempts are errors, not semantic results suitable for reuse.
/// Categories intentionally contain no provider replies, credentials or log data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionError {
    UnsupportedResource,
    UnsupportedProfile,
    InvalidRequest,
    MissingObject(ObjectId),
    ResourceExhausted,
    Trapped,
    Interrupted,
    BackendUnavailable,
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedResource => formatter.write_str("unsupported worker resource class"),
            Self::UnsupportedProfile => {
                formatter.write_str("unsupported execution profile or fidelity")
            }
            Self::InvalidRequest => formatter.write_str("invalid or unadmitted execution request"),
            Self::MissingObject(_) => {
                formatter.write_str("required execution object is unavailable")
            }
            Self::ResourceExhausted => formatter.write_str("execution resource budget exhausted"),
            Self::Trapped => formatter.write_str("execution attempt trapped"),
            Self::Interrupted => formatter.write_str("execution attempt interrupted"),
            Self::BackendUnavailable => formatter.write_str("execution backend unavailable"),
        }
    }
}

impl Error for ExecutionError {}

/// Trusted implementation of one admitted execution resource class.
///
/// Implementors must validate effect classification against admitted code/profile,
/// policy, security scope and declared adapter fidelity. A caller-provided `Pure`
/// label is not evidence that browser/SQL requests or fresh observations are pure.
pub trait WorkerBackend: Send + Sync {
    fn resource_class(&self) -> ResourceClass;

    /// Validate the request against actual admitted capabilities. This must check
    /// budgets, class, code/profile, scope/policy and fidelity before execution or
    /// any cache lookup. Canonical decoding alone does not perform admission.
    fn validate_request(&self, request: &StepRequest) -> Result<(), ExecutionError>;

    /// Default-deny reuse. An override may return true only for an admitted pure
    /// computation under the exact profile/fidelity/security/sample contract.
    /// The scheduler must first call validate_request and also require Pure.
    fn permits_pure_reuse(&self, _request: &StepRequest) -> bool {
        false
    }

    /// Run a bounded step after revalidating admission; do not trust that the caller
    /// already validated it. The compositor checks attempt/profile binding and
    /// validates/pins all output objects before caching or recording a result.
    /// No implementation may dispatch effects or acquire provider credentials here.
    fn run_step(&self, request: StepRequest) -> Result<StepOutcome, ExecutionError>;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    struct UnavailableBackend;

    impl WorkerBackend for UnavailableBackend {
        fn resource_class(&self) -> ResourceClass {
            ResourceClass::Browser
        }
        fn validate_request(&self, _request: &StepRequest) -> Result<(), ExecutionError> {
            Err(ExecutionError::BackendUnavailable)
        }
        fn run_step(&self, _request: StepRequest) -> Result<StepOutcome, ExecutionError> {
            Err(ExecutionError::BackendUnavailable)
        }
    }

    fn request() -> StepRequest {
        let id = ObjectId([1; 32]);
        StepRequest {
            attempt: id,
            code: id,
            profile: id,
            tenant: id,
            security_scope: id,
            input_root: id,
            continuation: id,
            observation_root: id,
            policy: id,
            fidelity: id,
            sample_id: None,
            resource_class: ResourceClass::Browser,
            effect_class: EffectClass::Pure,
            budget: ResourceBudget {
                fuel: 1,
                memory_bytes: 1,
                output_bytes: 1,
                host_calls: 0,
            },
        }
    }

    #[test]
    fn object_safe_registry_does_not_trust_a_pure_label() {
        let backend: Arc<dyn WorkerBackend> = Arc::new(UnavailableBackend);
        assert_eq!(backend.resource_class(), ResourceClass::Browser);
        assert!(!backend.permits_pure_reuse(&request()));
        assert_eq!(
            backend.validate_request(&request()),
            Err(ExecutionError::BackendUnavailable)
        );
        assert_eq!(
            backend.run_step(request()),
            Err(ExecutionError::BackendUnavailable)
        );
    }

    #[test]
    fn infrastructure_failure_is_a_typed_error() {
        let error: Box<dyn Error + Send + Sync> = Box::new(ExecutionError::Interrupted);
        assert_eq!(error.to_string(), "execution attempt interrupted");
    }
}

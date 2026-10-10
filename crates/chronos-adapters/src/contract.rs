use chronos_codec::{CodecError, ObjectId};
use chronos_state::execution::{
    EffectClass, ExecutionError, ResourceBudget, ResourceClass, StepRequest,
};

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("adapter capability is unsupported")]
    Unsupported,
    #[error("invalid adapter request")]
    InvalidRequest,
    #[error("invalid adapter snapshot")]
    InvalidSnapshot,
    #[error("adapter exceeds its {0} limit")]
    Limit(&'static str),
    #[error("adapter storage failed")]
    Storage,
    #[error("adapter backend unavailable")]
    BackendUnavailable,
    #[error("invalid adapter bridge protocol")]
    Protocol,
    #[error("adapter deadline exceeded")]
    Timeout,
    #[error("snapshot belongs to a different owner")]
    OwnerMismatch,
    #[error(transparent)]
    Codec(#[from] CodecError),
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Fidelity {
    #[default]
    RecordedReplay,
    OwnedEnvironment,
    Model,
    Live,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AdapterCapabilities {
    pub fidelity: Fidelity,
    pub snapshot: bool,
    pub restore: bool,
    pub isolated_remote_state: bool,
    pub idempotent_effects: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StorageSupport {
    pub cookies: bool,
    pub local_storage: bool,
    pub indexed_db: bool,
    pub session_storage: bool,
    pub hidden_stack: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdapterLimits {
    pub snapshot_bytes: usize,
    pub output_bytes: usize,
    pub deadline_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterManifest {
    pub version: u16,
    pub name: String,
    pub fidelity: Fidelity,
    pub storage: StorageSupport,
    pub deterministic_subset: Vec<String>,
    pub effect_semantics: Vec<String>,
    pub limits: AdapterLimits,
}
impl AdapterManifest {
    pub fn validate(&self) -> Result<(), AdapterError> {
        fn label(value: &str) -> bool {
            !value.is_empty()
                && value.len() <= 128
                && value.bytes().all(|b| b.is_ascii_graphic() || b == b' ')
        }
        if self.version != 1 || !label(&self.name) || self.storage.hidden_stack {
            return Err(AdapterError::Unsupported);
        }
        if !(1..=512 * 1024).contains(&self.limits.snapshot_bytes)
            || !(1..=512 * 1024).contains(&self.limits.output_bytes)
            || !(1..=30_000).contains(&self.limits.deadline_ms)
            || self.deterministic_subset.len() > 32
            || self.effect_semantics.len() > 32
            || !self
                .deterministic_subset
                .iter()
                .chain(&self.effect_semantics)
                .all(|s| label(s))
        {
            return Err(AdapterError::Limit("manifest"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationRequest {
    pub method: String,
    pub input: Option<ObjectId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub root: ObjectId,
    pub provenance: ObjectId,
    pub fidelity: Fidelity,
    pub captured_at_unix_ms: u64,
}
pub trait Adapter {
    fn capabilities(&self) -> AdapterCapabilities;
    fn snapshot(&mut self) -> Result<ObjectId, AdapterError>;
    fn restore(&mut self, root: ObjectId) -> Result<(), AdapterError>;
    fn observe(&mut self, request: ObservationRequest) -> Result<Observation, AdapterError>;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerBinding {
    pub tenant: ObjectId,
    pub security_scope: ObjectId,
    pub policy: ObjectId,
    pub fidelity: ObjectId,
    pub code: ObjectId,
    pub profile: ObjectId,
    pub effect_class: EffectClass,
    pub budget: ResourceBudget,
}
impl WorkerBinding {
    pub fn validate(
        &self,
        request: &StepRequest,
        class: ResourceClass,
    ) -> Result<(), ExecutionError> {
        request
            .budget
            .validate()
            .map_err(|_| ExecutionError::InvalidRequest)?;
        if request.resource_class != class
            || request.tenant != self.tenant
            || request.security_scope != self.security_scope
            || request.code != self.code
            || request.profile != self.profile
            || request.policy != self.policy
            || request.fidelity != self.fidelity
            || request.effect_class != self.effect_class
            || request.budget != self.budget
            || request.sample_id.is_some()
            || request.effect_class == EffectClass::IrreversibleExternal
        {
            return Err(ExecutionError::InvalidRequest);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_capabilities_default_closed_and_manifest_limits_are_explicit() {
        let caps = AdapterCapabilities::default();
        assert!(
            !caps.snapshot
                && !caps.restore
                && !caps.isolated_remote_state
                && !caps.idempotent_effects
        );
        let mut manifest = AdapterManifest {
            version: 1,
            name: "owned-test".into(),
            fidelity: Fidelity::OwnedEnvironment,
            storage: StorageSupport::default(),
            deterministic_subset: vec!["read".into()],
            effect_semantics: vec!["none".into()],
            limits: AdapterLimits {
                snapshot_bytes: 512 * 1024,
                output_bytes: 64 * 1024,
                deadline_ms: 5_000,
            },
        };
        manifest.validate().expect("bounded version one manifest");
        manifest.version = 2;
        assert!(manifest.validate().is_err());
        manifest.version = 1;
        manifest.storage.hidden_stack = true;
        assert!(manifest.validate().is_err());
        manifest.storage.hidden_stack = false;
        manifest.limits.output_bytes = usize::MAX;
        assert!(manifest.validate().is_err());
    }
    #[test]
    fn admitted_worker_binding_cannot_be_relabelled_or_resampled() {
        let id = ObjectId([1; 32]);
        let binding = WorkerBinding {
            tenant: id,
            security_scope: id,
            policy: id,
            fidelity: id,
            code: id,
            profile: id,
            effect_class: EffectClass::ManagedRead,
            budget: ResourceBudget {
                fuel: 100,
                memory_bytes: 1024,
                output_bytes: 100,
                host_calls: 1,
            },
        };
        let request = StepRequest {
            attempt: id,
            tenant: id,
            security_scope: id,
            policy: id,
            fidelity: id,
            code: id,
            profile: id,
            input_root: id,
            continuation: id,
            observation_root: id,
            sample_id: None,
            resource_class: ResourceClass::Sql,
            effect_class: binding.effect_class,
            budget: binding.budget,
        };
        binding
            .validate(&request, ResourceClass::Sql)
            .expect("exact binding");
        for field in 0..10 {
            let mut changed = request.clone();
            let different = ObjectId([2; 32]);
            match field {
                0 => changed.tenant = different,
                1 => changed.security_scope = different,
                2 => changed.policy = different,
                3 => changed.fidelity = different,
                4 => changed.code = different,
                5 => changed.profile = different,
                6 => changed.effect_class = EffectClass::Pure,
                7 => changed.budget.fuel += 1,
                8 => changed.sample_id = Some(different),
                _ => changed.resource_class = ResourceClass::Browser,
            }
            assert!(
                binding.validate(&changed, ResourceClass::Sql).is_err(),
                "field {field}"
            );
        }
    }
}

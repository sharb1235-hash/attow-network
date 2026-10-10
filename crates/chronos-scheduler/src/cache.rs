//! Exact pure-evaluation identity; cache storage and single flight are engine-owned.
use crate::ScheduleError;
use chronos_codec::{
    ObjectId,
    execution::{EffectClass, ResourceClass, StepRequest},
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvaluationKey {
    pub tenant: ObjectId,
    pub security_scope: ObjectId,
    pub code: ObjectId,
    pub profile: ObjectId,
    pub input_root: ObjectId,
    pub continuation: ObjectId,
    pub observation_root: ObjectId,
    pub policy: ObjectId,
    pub fidelity: ObjectId,
    pub sample_id: Option<ObjectId>,
    pub resource_class: ResourceClass,
    pub effect_class: EffectClass,
    pub budget: (u64, u64, u64, u32),
}

impl EvaluationKey {
    /// A well-formed Pure label can form a key, but actual backend validation
    /// and explicit reuse admission are still required before any cache lookup.
    pub fn from_request(request: &StepRequest) -> Result<Self, ScheduleError> {
        if request.effect_class != EffectClass::Pure {
            return Err(ScheduleError::InvalidRequest);
        }
        request
            .budget
            .validate()
            .map_err(|_| ScheduleError::InvalidRequest)?;
        Ok(Self {
            tenant: request.tenant,
            security_scope: request.security_scope,
            code: request.code,
            profile: request.profile,
            input_root: request.input_root,
            continuation: request.continuation,
            observation_root: request.observation_root,
            policy: request.policy,
            fidelity: request.fidelity,
            sample_id: request.sample_id,
            resource_class: request.resource_class,
            effect_class: request.effect_class,
            budget: (
                request.budget.fuel,
                request.budget.memory_bytes,
                request.budget.output_bytes,
                request.budget.host_calls,
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{node, test_id};

    #[test]
    fn exact_key_ignores_attempt_but_binds_security_inputs_samples_and_budgets() {
        let original = node(1, &[]).request;
        let key = EvaluationKey::from_request(&original).expect("pure key");
        let mut same = original.clone();
        same.attempt = test_id(999);
        assert_eq!(
            key,
            EvaluationKey::from_request(&same).expect("same semantics")
        );
        for field in 0..15 {
            let mut changed = original.clone();
            match field {
                0 => changed.tenant = test_id(999),
                1 => changed.security_scope = test_id(999),
                2 => changed.code = test_id(999),
                3 => changed.profile = test_id(999),
                4 => changed.input_root = test_id(999),
                5 => changed.continuation = test_id(999),
                6 => changed.observation_root = test_id(999),
                7 => changed.policy = test_id(999),
                8 => changed.fidelity = test_id(999),
                9 => changed.sample_id = Some(test_id(999)),
                10 => changed.resource_class = ResourceClass::Browser,
                11 => changed.budget.fuel += 1,
                12 => changed.budget.memory_bytes += 1,
                13 => changed.budget.output_bytes += 1,
                _ => changed.budget.host_calls += 1,
            }
            assert_ne!(
                key,
                EvaluationKey::from_request(&changed).expect("different key"),
                "unbound field {field}"
            );
        }
    }

    #[test]
    fn fresh_observations_and_invalid_budgets_cannot_form_pure_keys() {
        let mut request = node(1, &[]).request;
        request.effect_class = EffectClass::FreshObservation;
        assert!(EvaluationKey::from_request(&request).is_err());
        request.effect_class = EffectClass::Pure;
        request.budget.fuel = 0;
        assert!(EvaluationKey::from_request(&request).is_err());
    }
}

//! Version-one worker envelopes. These contain immutable references and claimed
//! execution metadata, never credentials or permission to dispatch an effect.
//!
//! Requests are generic canonical Objects with payload
//! `[0x41545251, 1, attempt, code, profile, tenant, scope, input, continuation,
//! observations, policy, fidelity, sample_or_null, resource, effect, budget]`.
//! Replies use `[0x41545250, 1, attempt, profile, exit]`. IDs are 32-byte strings.
//! Resource tags are Wasm=0, Browser=1, Sql=2; effect tags follow enum order 0..6.
//! A budget is `[fuel, memory_bytes, output_bytes, host_calls]`. Exit is
//! `[0, result, delta, observations]` or `[1, continuation, request, delta,
//! observations]`. Unknown versions, tags, fields and mismatched reference sets
//! fail closed. Attempts, tenants, scopes and samples are opaque identities;
//! all other ID fields name immutable objects and appear in envelope references.

use std::collections::BTreeSet;

use crate::{Canonical, CodecError, Limits, Object, ObjectId, Value};

const REQUEST_TAG: u64 = 0x4154_5251;
const OUTCOME_TAG: u64 = 0x4154_5250;
const SCHEMA: u64 = 1;

/// Engineering envelope caps; a backend must enforce its stricter admitted profile.
pub const MAX_STEP_FUEL: u64 = 1_000_000_000_000;
pub const MAX_STEP_MEMORY_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_STEP_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_STEP_HOST_CALLS: u32 = 1_000_000;

/// Resource scheduling class, not a declaration that a backend is installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResourceClass {
    Wasm,
    Browser,
    Sql,
}

/// Claimed effects must be validated against admitted code and backend fidelity.
/// A caller marking a request Pure does not grant it cache eligibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EffectClass {
    Pure,
    ManagedRead,
    ManagedWrite,
    RecordedObservation,
    FreshObservation,
    PreparedExternal,
    IrreversibleExternal,
}

/// Fuel is an instruction/profile budget, not a wall-clock guarantee. Zero host
/// calls is valid; fuel, memory and output budgets must be positive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceBudget {
    pub fuel: u64,
    pub memory_bytes: u64,
    pub output_bytes: u64,
    pub host_calls: u32,
}

impl ResourceBudget {
    pub fn validate(&self) -> Result<(), CodecError> {
        if !(1..=MAX_STEP_FUEL).contains(&self.fuel)
            || !(1..=MAX_STEP_MEMORY_BYTES).contains(&self.memory_bytes)
            || !(1..=MAX_STEP_OUTPUT_BYTES).contains(&self.output_bytes)
            || self.host_calls > MAX_STEP_HOST_CALLS
        {
            return Err(CodecError::InvalidLimits);
        }
        Ok(())
    }
}

/// Portable inputs for one attempt. Identities remain distinct from object refs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepRequest {
    pub attempt: ObjectId,
    pub code: ObjectId,
    pub profile: ObjectId,
    pub tenant: ObjectId,
    pub security_scope: ObjectId,
    pub input_root: ObjectId,
    pub continuation: ObjectId,
    pub observation_root: ObjectId,
    pub policy: ObjectId,
    pub fidelity: ObjectId,
    pub sample_id: Option<ObjectId>,
    pub resource_class: ResourceClass,
    pub effect_class: EffectClass,
    pub budget: ResourceBudget,
}

impl StepRequest {
    pub fn references(&self) -> BTreeSet<ObjectId> {
        [
            self.code,
            self.profile,
            self.input_root,
            self.continuation,
            self.observation_root,
            self.policy,
            self.fidelity,
        ]
        .into_iter()
        .collect()
    }
}

/// A completed step or a portable suspension. The suspension request is a typed
/// proposal/observation object; it has no authority to send an external request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepExit {
    Completed {
        result: ObjectId,
        delta: ObjectId,
        observation_root: ObjectId,
    },
    Suspended {
        continuation: ObjectId,
        request: ObjectId,
        delta: ObjectId,
        observation_root: ObjectId,
    },
}

/// Successful semantic output only. Infrastructure interruption and traps are
/// errors in the worker port, never a cacheable variant of this envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepOutcome {
    pub attempt: ObjectId,
    pub profile: ObjectId,
    pub exit: StepExit,
}

impl StepOutcome {
    pub fn references(&self) -> BTreeSet<ObjectId> {
        let mut refs = BTreeSet::from([self.profile]);
        match self.exit {
            StepExit::Completed {
                result,
                delta,
                observation_root,
            } => {
                refs.extend([result, delta, observation_root]);
            }
            StepExit::Suspended {
                continuation,
                request,
                delta,
                observation_root,
            } => {
                refs.extend([continuation, request, delta, observation_root]);
            }
        }
        refs
    }

    /// The compositor must additionally validate/pin returned object contents.
    pub fn validate_for(&self, request: &StepRequest) -> Result<(), CodecError> {
        if self.attempt != request.attempt || self.profile != request.profile {
            return Err(CodecError::InvalidObject);
        }
        Ok(())
    }
}

fn id(value: ObjectId) -> Value {
    Value::Bytes(value.0.to_vec())
}
fn uint(value: u64) -> Value {
    Value::Unsigned(value)
}

fn fields<const N: usize>(value: Value) -> Result<[Value; N], CodecError> {
    let Value::Array(values) = value else {
        return Err(CodecError::InvalidObject);
    };
    values.try_into().map_err(|_| CodecError::InvalidObject)
}

fn read_uint(value: Value) -> Result<u64, CodecError> {
    let Value::Unsigned(value) = value else {
        return Err(CodecError::InvalidObject);
    };
    Ok(value)
}

fn read_id(value: Value) -> Result<ObjectId, CodecError> {
    let Value::Bytes(value) = value else {
        return Err(CodecError::InvalidId);
    };
    Ok(ObjectId(
        value.try_into().map_err(|_| CodecError::InvalidId)?,
    ))
}

fn version(tag: Value, schema: Value, expected: u64) -> Result<(), CodecError> {
    if read_uint(tag)? != expected {
        return Err(CodecError::InvalidObject);
    }
    if read_uint(schema)? != SCHEMA {
        return Err(CodecError::UnknownSchema);
    }
    Ok(())
}

impl Canonical for StepRequest {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.budget.validate()?;
        let resource = match self.resource_class {
            ResourceClass::Wasm => 0,
            ResourceClass::Browser => 1,
            ResourceClass::Sql => 2,
        };
        let effect = match self.effect_class {
            EffectClass::Pure => 0,
            EffectClass::ManagedRead => 1,
            EffectClass::ManagedWrite => 2,
            EffectClass::RecordedObservation => 3,
            EffectClass::FreshObservation => 4,
            EffectClass::PreparedExternal => 5,
            EffectClass::IrreversibleExternal => 6,
        };
        Object {
            refs: self.references(),
            payload: Value::Array(vec![
                uint(REQUEST_TAG),
                uint(SCHEMA),
                id(self.attempt),
                id(self.code),
                id(self.profile),
                id(self.tenant),
                id(self.security_scope),
                id(self.input_root),
                id(self.continuation),
                id(self.observation_root),
                id(self.policy),
                id(self.fidelity),
                self.sample_id.map(id).unwrap_or(Value::Null),
                uint(resource),
                uint(effect),
                Value::Array(vec![
                    uint(self.budget.fuel),
                    uint(self.budget.memory_bytes),
                    uint(self.budget.output_bytes),
                    uint(u64::from(self.budget.host_calls)),
                ]),
            ]),
        }
        .encode()
    }

    fn decode(bytes: &[u8], limits: &Limits) -> Result<Self, CodecError> {
        let object = Object::decode(bytes, limits)?;
        let [
            tag,
            schema,
            attempt,
            code,
            profile,
            tenant,
            security_scope,
            input_root,
            continuation,
            observation_root,
            policy,
            fidelity,
            sample,
            resource,
            effect,
            budget,
        ] = fields(object.payload)?;
        version(tag, schema, REQUEST_TAG)?;
        let [fuel, memory, output, calls] = fields(budget)?;
        let resource_class = match read_uint(resource)? {
            0 => ResourceClass::Wasm,
            1 => ResourceClass::Browser,
            2 => ResourceClass::Sql,
            _ => return Err(CodecError::UnsupportedType),
        };
        let effect_class = match read_uint(effect)? {
            0 => EffectClass::Pure,
            1 => EffectClass::ManagedRead,
            2 => EffectClass::ManagedWrite,
            3 => EffectClass::RecordedObservation,
            4 => EffectClass::FreshObservation,
            5 => EffectClass::PreparedExternal,
            6 => EffectClass::IrreversibleExternal,
            _ => return Err(CodecError::UnsupportedType),
        };
        let request = Self {
            attempt: read_id(attempt)?,
            code: read_id(code)?,
            profile: read_id(profile)?,
            tenant: read_id(tenant)?,
            security_scope: read_id(security_scope)?,
            input_root: read_id(input_root)?,
            continuation: read_id(continuation)?,
            observation_root: read_id(observation_root)?,
            policy: read_id(policy)?,
            fidelity: read_id(fidelity)?,
            sample_id: if sample == Value::Null {
                None
            } else {
                Some(read_id(sample)?)
            },
            resource_class,
            effect_class,
            budget: ResourceBudget {
                fuel: read_uint(fuel)?,
                memory_bytes: read_uint(memory)?,
                output_bytes: read_uint(output)?,
                host_calls: u32::try_from(read_uint(calls)?)
                    .map_err(|_| CodecError::InvalidLimits)?,
            },
        };
        request.budget.validate()?;
        if request.references() != object.refs {
            return Err(CodecError::InvalidObject);
        }
        Ok(request)
    }
}

impl Canonical for StepOutcome {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let exit = match self.exit {
            StepExit::Completed {
                result,
                delta,
                observation_root,
            } => vec![uint(0), id(result), id(delta), id(observation_root)],
            StepExit::Suspended {
                continuation,
                request,
                delta,
                observation_root,
            } => vec![
                uint(1),
                id(continuation),
                id(request),
                id(delta),
                id(observation_root),
            ],
        };
        Object {
            refs: self.references(),
            payload: Value::Array(vec![
                uint(OUTCOME_TAG),
                uint(SCHEMA),
                id(self.attempt),
                id(self.profile),
                Value::Array(exit),
            ]),
        }
        .encode()
    }

    fn decode(bytes: &[u8], limits: &Limits) -> Result<Self, CodecError> {
        let object = Object::decode(bytes, limits)?;
        let [tag, schema, attempt, profile, exit] = fields(object.payload)?;
        version(tag, schema, OUTCOME_TAG)?;
        let Value::Array(exit) = exit else {
            return Err(CodecError::InvalidObject);
        };
        let exit = match exit.as_slice() {
            [Value::Unsigned(0), _, _, _] => {
                let [_, result, delta, observation_root] = fields(Value::Array(exit))?;
                StepExit::Completed {
                    result: read_id(result)?,
                    delta: read_id(delta)?,
                    observation_root: read_id(observation_root)?,
                }
            }
            [Value::Unsigned(1), _, _, _, _] => {
                let [_, continuation, request, delta, observation_root] =
                    fields(Value::Array(exit))?;
                StepExit::Suspended {
                    continuation: read_id(continuation)?,
                    request: read_id(request)?,
                    delta: read_id(delta)?,
                    observation_root: read_id(observation_root)?,
                }
            }
            _ => return Err(CodecError::InvalidObject),
        };
        let outcome = Self {
            attempt: read_id(attempt)?,
            profile: read_id(profile)?,
            exit,
        };
        if outcome.references() != object.refs {
            return Err(CodecError::InvalidObject);
        }
        Ok(outcome)
    }
}

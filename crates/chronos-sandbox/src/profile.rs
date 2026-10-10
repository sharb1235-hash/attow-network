//! Versioned execution budgets bound to the exact supported runtime contract.
use std::collections::BTreeSet;

use chronos_codec::{
    Canonical, CodecError, Limits, OBJECT_KIND, OBJECT_SCHEMA, Object, ObjectId, Value, object_id,
};
use wasmtime::WasmFeatures;

use crate::SandboxError;

pub const MAX_CODE_BYTES: usize = 512 * 1024;
pub const MAX_MEMORY_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024;
pub const MAX_FUEL: u64 = 100_000_000;
pub const MAX_HOST_CALLS: u32 = 1_000_000;
pub const MAX_TABLE_ELEMENTS: usize = 1024;
pub const MAX_WORKERS: u32 = 16;
pub const MAX_WASM_STACK: usize = 512 * 1024;
pub const MAX_FUNCTIONS: u32 = 256;
pub const MAX_TYPES: u32 = 256;
pub const MAX_LOCALS: u32 = 4096;
pub const MAX_OPERATORS: u32 = 65_536;
pub const MAX_SECTIONS: u32 = 1024;
pub const MAX_AUX_ITEMS: u32 = 128;
pub const MAX_CORE_INSTANCE_BYTES: usize = 256 * 1024;
pub const MAX_COMPONENT_INSTANCE_BYTES: usize = 1024 * 1024;

pub(crate) fn supported_features() -> WasmFeatures {
    // wasmparser's MVP convenience set includes GC_TYPES, which this Wasmtime
    // build deliberately does not compile. Scalar floats are allowed alone.
    WasmFeatures::FLOATS
        | WasmFeatures::SATURATING_FLOAT_TO_INT
        | WasmFeatures::SIGN_EXTENSION
        | WasmFeatures::REFERENCE_TYPES
        | WasmFeatures::MULTI_VALUE
        | WasmFeatures::BULK_MEMORY
        | WasmFeatures::COMPONENT_MODEL
}

fn contract() -> Value {
    Value::Array(vec![
        Value::Text("wasmtime=48.0.5;wasmparser=0.254.2;attow:worldline@1.0.0;scalar-step-v1".into()),
        Value::Text(std::env::consts::OS.into()),
        Value::Text(std::env::consts::ARCH.into()),
        Value::Text("fuel=true;nan=canonical;memory-init-cow=false;fixed-memory-tables;mutable-globals=false;starts=false;imports=input-len,input-byte,output-byte;one-core-module;one-core-instance;one-host-bundle;one-memory;one-table;pooling;in-process;no-native-deserialization".into()),
        Value::Unsigned(supported_features().bits()),
        Value::Array([
            MAX_CODE_BYTES as u64, MAX_MEMORY_BYTES as u64, MAX_OUTPUT_BYTES as u64,
            MAX_FUEL, u64::from(MAX_HOST_CALLS), MAX_TABLE_ELEMENTS as u64,
            u64::from(MAX_WORKERS), MAX_WASM_STACK as u64, u64::from(MAX_FUNCTIONS),
            u64::from(MAX_TYPES), u64::from(MAX_LOCALS), u64::from(MAX_OPERATORS),
            u64::from(MAX_SECTIONS), u64::from(MAX_AUX_ITEMS),
            MAX_CORE_INSTANCE_BYTES as u64, MAX_COMPONENT_INSTANCE_BYTES as u64,
        ].into_iter().map(Value::Unsigned).collect()),
    ])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionProfile {
    pub fuel: u64,
    pub memory_bytes: usize,
    pub output_bytes: usize,
    pub profile_hash: ObjectId,
}

impl ExecutionProfile {
    pub fn new(fuel: u64, memory_bytes: usize, output_bytes: usize) -> Result<Self, SandboxError> {
        let mut profile = Self {
            fuel,
            memory_bytes,
            output_bytes,
            profile_hash: ObjectId([0; 32]),
        };
        profile.check_budgets()?;
        profile.profile_hash = object_id(OBJECT_KIND, OBJECT_SCHEMA, &profile.canonical_bytes()?);
        Ok(profile)
    }
    pub fn validate(&self) -> Result<(), SandboxError> {
        self.check_budgets()?;
        if self.profile_hash != object_id(OBJECT_KIND, OBJECT_SCHEMA, &self.canonical_bytes()?) {
            return Err(SandboxError::InvalidProfile);
        }
        Ok(())
    }
    pub fn encode(&self) -> Result<Vec<u8>, SandboxError> {
        self.validate()?;
        self.canonical_bytes()
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, SandboxError> {
        Self::decode_with_limits(bytes, &Limits::default())
    }
    fn check_budgets(&self) -> Result<(), SandboxError> {
        if !(1..=MAX_FUEL).contains(&self.fuel)
            || !(1..=MAX_MEMORY_BYTES).contains(&self.memory_bytes)
            || !(1..=MAX_OUTPUT_BYTES).contains(&self.output_bytes)
        {
            return Err(SandboxError::InvalidProfile);
        }
        Ok(())
    }
    fn canonical_bytes(&self) -> Result<Vec<u8>, SandboxError> {
        Ok(Object {
            refs: BTreeSet::new(),
            payload: Value::Array(vec![
                Value::Text("attow-wasm-profile".into()),
                Value::Unsigned(1),
                contract(),
                Value::Unsigned(self.fuel),
                Value::Unsigned(self.memory_bytes as u64),
                Value::Unsigned(self.output_bytes as u64),
            ]),
        }
        .encode()?)
    }
    fn decode_with_limits(bytes: &[u8], limits: &Limits) -> Result<Self, SandboxError> {
        let object = Object::decode(bytes, limits)?;
        if !object.refs.is_empty() {
            return Err(SandboxError::InvalidProfile);
        }
        let Value::Array(fields) = object.payload else {
            return Err(SandboxError::InvalidProfile);
        };
        let [
            Value::Text(tag),
            Value::Unsigned(1),
            supplied_contract,
            Value::Unsigned(fuel),
            Value::Unsigned(memory),
            Value::Unsigned(output),
        ]: [Value; 6] = fields
            .try_into()
            .map_err(|_| SandboxError::InvalidProfile)?
        else {
            return Err(SandboxError::InvalidProfile);
        };
        if tag != "attow-wasm-profile" || supplied_contract != contract() {
            return Err(SandboxError::InvalidProfile);
        }
        Self::new(
            fuel,
            usize::try_from(memory).map_err(|_| SandboxError::InvalidProfile)?,
            usize::try_from(output).map_err(|_| SandboxError::InvalidProfile)?,
        )
    }
}

impl Canonical for ExecutionProfile {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.encode().map_err(|_| CodecError::InvalidObject)
    }
    fn decode(bytes: &[u8], limits: &Limits) -> Result<Self, CodecError> {
        Self::decode_with_limits(bytes, limits).map_err(|_| CodecError::InvalidObject)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_roundtrip_binds_budgets_and_rejects_mutated_public_fields() {
        let profile = ExecutionProfile::new(10_000, 65_536, 1024).expect("valid profile");
        let bytes = profile.encode().expect("encode");
        assert_eq!(ExecutionProfile::decode(&bytes).expect("decode"), profile);
        for mutate in [0, 1, 2, 3] {
            let mut changed = profile.clone();
            match mutate {
                0 => changed.fuel += 1,
                1 => changed.memory_bytes += 1,
                2 => changed.output_bytes += 1,
                _ => changed.profile_hash.0[0] ^= 1,
            }
            assert!(changed.validate().is_err());
            assert!(changed.encode().is_err());
        }
        assert_ne!(
            profile.profile_hash,
            ExecutionProfile::new(10_001, 65_536, 1024)
                .expect("other")
                .profile_hash
        );
    }

    #[test]
    fn invalid_budgets_are_rejected_before_engine_work() {
        for budgets in [
            (0, 65_536, 1024),
            (MAX_FUEL + 1, 65_536, 1024),
            (1, 0, 1024),
            (1, MAX_MEMORY_BYTES + 1, 1024),
            (1, 1, 0),
            (1, 1, MAX_OUTPUT_BYTES + 1),
        ] {
            assert!(ExecutionProfile::new(budgets.0, budgets.1, budgets.2).is_err());
        }
        assert!(ExecutionProfile::new(MAX_FUEL, MAX_MEMORY_BYTES, MAX_OUTPUT_BYTES).is_ok());
    }
}

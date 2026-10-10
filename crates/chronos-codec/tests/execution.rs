use attow_codec::{
    Canonical, CodecError, Limits, Object, ObjectId, Value,
    execution::{EffectClass, ResourceBudget, ResourceClass, StepExit, StepOutcome, StepRequest},
};

fn id(value: u8) -> ObjectId {
    ObjectId([value; 32])
}

fn request() -> StepRequest {
    StepRequest {
        attempt: id(1),
        code: id(2),
        profile: id(3),
        tenant: id(4),
        security_scope: id(5),
        input_root: id(6),
        continuation: id(7),
        observation_root: id(8),
        policy: id(9),
        fidelity: id(10),
        sample_id: Some(id(11)),
        resource_class: ResourceClass::Wasm,
        effect_class: EffectClass::Pure,
        budget: ResourceBudget {
            fuel: 10_000,
            memory_bytes: 65_536,
            output_bytes: 4_096,
            host_calls: 32,
        },
    }
}

fn mutate_payload(bytes: &[u8], edit: impl FnOnce(&mut Vec<Value>)) -> Result<Vec<u8>, CodecError> {
    let mut envelope = Object::decode(bytes, &Limits::default())?;
    let Value::Array(fields) = &mut envelope.payload else {
        return Err(CodecError::InvalidObject);
    };
    edit(fields);
    envelope.encode()
}

#[test]
fn request_roundtrip_binds_every_identity_and_budget() -> Result<(), CodecError> {
    let request = request();
    let bytes = request.encode()?;
    assert_eq!(StepRequest::decode(&bytes, &Limits::default())?, request);
    assert_eq!(
        StepRequest::decode(&bytes, &Limits::default())?.encode()?,
        bytes
    );
    let envelope = Object::decode(&bytes, &Limits::default())?;
    let Value::Array(fields) = &envelope.payload else {
        return Err(CodecError::InvalidObject);
    };
    assert_eq!(fields[0], Value::Unsigned(0x4154_5251));
    assert_eq!(fields[1], Value::Unsigned(1));
    assert_eq!(fields.len(), 16);
    assert_eq!(envelope.refs, request.references());
    assert_eq!(envelope.refs.len(), 7);
    assert!(!envelope.refs.contains(&request.attempt));
    assert!(!envelope.refs.contains(&request.tenant));
    assert!(!envelope.refs.contains(&request.security_scope));
    assert!(!envelope.refs.contains(&id(11)));
    let mut other = request.clone();
    other.sample_id = Some(id(12));
    assert_ne!(other.encode()?, bytes);
    other = request.clone();
    other.security_scope = id(12);
    assert_ne!(other.encode()?, bytes);
    other = request;
    other.budget.fuel += 1;
    assert_ne!(other.encode()?, bytes);
    Ok(())
}

#[test]
fn request_rejects_version_shape_variant_and_reference_changes() -> Result<(), CodecError> {
    let bytes = request().encode()?;
    let bad_version = mutate_payload(&bytes, |fields| fields[1] = Value::Unsigned(2))?;
    assert_eq!(
        StepRequest::decode(&bad_version, &Limits::default()),
        Err(CodecError::UnknownSchema)
    );
    let extra = mutate_payload(&bytes, |fields| fields.push(Value::Null))?;
    assert!(StepRequest::decode(&extra, &Limits::default()).is_err());
    let bad_resource = mutate_payload(&bytes, |fields| fields[13] = Value::Unsigned(255))?;
    assert!(StepRequest::decode(&bad_resource, &Limits::default()).is_err());
    let bad_effect = mutate_payload(&bytes, |fields| fields[14] = Value::Unsigned(255))?;
    assert!(StepRequest::decode(&bad_effect, &Limits::default()).is_err());
    let short_id = mutate_payload(&bytes, |fields| fields[2] = Value::Bytes(vec![1; 31]))?;
    assert!(StepRequest::decode(&short_id, &Limits::default()).is_err());
    let mut object = Object::decode(&bytes, &Limits::default())?;
    object.refs.remove(&id(2));
    assert!(StepRequest::decode(&object.encode()?, &Limits::default()).is_err());
    object.refs.insert(id(2));
    object.refs.insert(id(99));
    assert!(StepRequest::decode(&object.encode()?, &Limits::default()).is_err());
    Ok(())
}

#[test]
fn request_requires_positive_bounded_budgets_on_encode_and_decode() -> Result<(), CodecError> {
    let mut invalid = request();
    invalid.budget.fuel = 0;
    assert!(invalid.encode().is_err());
    invalid = request();
    invalid.budget.memory_bytes = u64::MAX;
    assert!(invalid.encode().is_err());
    invalid = request();
    invalid.budget.output_bytes = u64::MAX;
    assert!(invalid.encode().is_err());
    invalid = request();
    invalid.budget.host_calls = u32::MAX;
    assert!(invalid.encode().is_err());
    let encoded = request().encode()?;
    let invalid = mutate_payload(&encoded, |fields| {
        fields[15] = Value::Array(vec![
            Value::Unsigned(0),
            Value::Unsigned(65_536),
            Value::Unsigned(4_096),
            Value::Unsigned(32),
        ]);
    })?;
    assert!(StepRequest::decode(&invalid, &Limits::default()).is_err());
    Ok(())
}

#[test]
fn requests_reject_truncation_trailing_and_insufficient_limits() -> Result<(), CodecError> {
    let bytes = request().encode()?;
    for length in 0..bytes.len() {
        assert!(StepRequest::decode(&bytes[..length], &Limits::default()).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(StepRequest::decode(&trailing, &Limits::default()).is_err());
    for limits in [
        Limits {
            object_bytes: bytes.len() - 1,
            ..Limits::default()
        },
        Limits {
            depth: 2,
            ..Limits::default()
        },
        Limits {
            items: 3,
            ..Limits::default()
        },
    ] {
        assert!(StepRequest::decode(&bytes, &limits).is_err());
    }
    Ok(())
}

#[test]
fn outcome_variants_roundtrip_without_dispatch_or_incomplete_success() -> Result<(), CodecError> {
    for exit in [
        StepExit::Completed {
            result: id(21),
            delta: id(22),
            observation_root: id(23),
        },
        StepExit::Suspended {
            continuation: id(24),
            request: id(25),
            delta: id(22),
            observation_root: id(23),
        },
    ] {
        let outcome = StepOutcome {
            attempt: id(1),
            profile: id(3),
            exit,
        };
        let bytes = outcome.encode()?;
        assert_eq!(StepOutcome::decode(&bytes, &Limits::default())?, outcome);
        let Value::Array(fields) = Object::decode(&bytes, &Limits::default())?.payload else {
            return Err(CodecError::InvalidObject);
        };
        assert_eq!(fields[0], Value::Unsigned(0x4154_5250));
        assert_eq!(fields[1], Value::Unsigned(1));
        assert_eq!(fields.len(), 5);
        assert_eq!(
            Object::decode(&bytes, &Limits::default())?.refs,
            outcome.references()
        );
        let version = mutate_payload(&bytes, |fields| fields[1] = Value::Unsigned(2))?;
        assert_eq!(
            StepOutcome::decode(&version, &Limits::default()),
            Err(CodecError::UnknownSchema)
        );
        let variant = mutate_payload(&bytes, |fields| {
            fields[4] = Value::Array(vec![Value::Unsigned(2)])
        })?;
        assert!(StepOutcome::decode(&variant, &Limits::default()).is_err());
    }
    Ok(())
}

#[test]
fn outcomes_must_match_the_attempt_and_profile() -> Result<(), CodecError> {
    let request = request();
    let mut outcome = StepOutcome {
        attempt: request.attempt,
        profile: request.profile,
        exit: StepExit::Completed {
            result: id(21),
            delta: id(22),
            observation_root: id(23),
        },
    };
    outcome.validate_for(&request)?;
    outcome.attempt = id(99);
    assert!(outcome.validate_for(&request).is_err());
    outcome.attempt = request.attempt;
    outcome.profile = id(99);
    assert!(outcome.validate_for(&request).is_err());
    Ok(())
}

#[test]
fn request_variants_are_explicit_and_roundtrip() -> Result<(), CodecError> {
    for resource_class in [
        ResourceClass::Wasm,
        ResourceClass::Browser,
        ResourceClass::Sql,
    ] {
        for effect_class in [
            EffectClass::Pure,
            EffectClass::ManagedRead,
            EffectClass::ManagedWrite,
            EffectClass::RecordedObservation,
            EffectClass::FreshObservation,
            EffectClass::PreparedExternal,
            EffectClass::IrreversibleExternal,
        ] {
            let request = StepRequest {
                resource_class,
                effect_class,
                sample_id: None,
                ..request()
            };
            assert_eq!(
                StepRequest::decode(&request.encode()?, &Limits::default())?,
                request
            );
        }
    }
    Ok(())
}

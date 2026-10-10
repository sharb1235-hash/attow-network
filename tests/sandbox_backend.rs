//! Admission must precede both execution and reusable-pure classification.
use attow_network::{
    ObjectId,
    codec::{Canonical, OBJECT_KIND, OBJECT_SCHEMA, object_id},
    sandbox::{
        SqliteSandboxRepository,
        runtime::{
            ExecutionProfile, Executor, SandboxRepository, WorkerAdmission, profile::MAX_HOST_CALLS,
        },
    },
    state::execution::{
        EffectClass, ExecutionError, ResourceBudget, ResourceClass, StepExit, StepRequest,
        WorkerBackend,
    },
};
use std::sync::Arc;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn request(executor: &Executor, fixture: &str) -> Result<StepRequest, Box<dyn std::error::Error>> {
    let wasm = wat::parse_file(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/original/wasm")
            .join(fixture),
    )?;
    let code = executor.admit(&wasm)?;
    let profile = ExecutionProfile::new(50_000, 65_536, 1024)?;
    let continuation = executor.initial_continuation(&code, &profile, &[23])?;
    Ok(StepRequest {
        attempt: ObjectId([1; 32]),
        code: code.id(),
        profile: profile.profile_hash,
        tenant: ObjectId([2; 32]),
        security_scope: ObjectId([3; 32]),
        input_root: executor.store_data(&[19])?,
        continuation,
        observation_root: executor.store_data(&[])?,
        policy: executor.store_data(b"policy")?,
        fidelity: executor.store_data(b"attow-wasm-v1")?,
        sample_id: None,
        resource_class: ResourceClass::Wasm,
        effect_class: EffectClass::Pure,
        budget: ResourceBudget {
            fuel: profile.fuel,
            memory_bytes: profile.memory_bytes as u64,
            output_bytes: profile.output_bytes as u64,
            host_calls: MAX_HOST_CALLS,
        },
    })
}
fn admission(request: &StepRequest) -> WorkerAdmission {
    WorkerAdmission {
        tenant: request.tenant,
        security_scope: request.security_scope,
        policy: request.policy,
        fidelity: request.fidelity,
        code: request.code,
        profile: request.profile,
        effect_class: request.effect_class,
    }
}

#[test]
fn caller_pure_label_needs_exact_host_admission_and_profile_budgets() -> TestResult {
    let directory = tempfile::tempdir()?;
    let executor = Executor::new(Arc::new(SqliteSandboxRepository::open(
        &directory.path().join("sandbox.sqlite"),
    )?))?;
    let request = request(&executor, "arithmetic.wat")?;
    assert!(executor.validate_request(&request).is_err());
    assert!(!executor.permits_pure_reuse(&request));
    assert!(WorkerBackend::run_step(&executor, request.clone()).is_err());
    executor.register(admission(&request))?;
    executor.validate_request(&request)?;
    assert!(executor.permits_pure_reuse(&request));
    for field in 0..12 {
        let mut changed = request.clone();
        let other = ObjectId([99; 32]);
        match field {
            0 => changed.tenant = other,
            1 => changed.security_scope = other,
            2 => changed.policy = other,
            3 => changed.fidelity = other,
            4 => changed.code = other,
            5 => changed.profile = other,
            6 => changed.effect_class = EffectClass::ManagedWrite,
            7 => changed.resource_class = ResourceClass::Browser,
            8 => changed.budget.fuel -= 1,
            9 => changed.budget.memory_bytes -= 1,
            10 => changed.budget.output_bytes -= 1,
            _ => changed.budget.host_calls -= 1,
        }
        assert!(
            executor.validate_request(&changed).is_err(),
            "field {field}"
        );
        assert!(!executor.permits_pure_reuse(&changed), "field {field}");
        assert!(
            WorkerBackend::run_step(&executor, changed).is_err(),
            "field {field}"
        );
    }
    for effect_class in [
        EffectClass::FreshObservation,
        EffectClass::IrreversibleExternal,
    ] {
        assert!(
            executor
                .register(WorkerAdmission {
                    effect_class,
                    ..admission(&request)
                })
                .is_err()
        );
    }
    Ok(())
}

#[test]
fn standard_outcome_is_durable_and_distinct_attempts_remain_bound() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("sandbox.sqlite");
    let repository = Arc::new(SqliteSandboxRepository::open(&path)?);
    let executor = Executor::new(repository.clone())?;
    let first = request(&executor, "arithmetic.wat")?;
    executor.register(admission(&first))?;
    let outcome = WorkerBackend::run_step(&executor, first.clone())?;
    outcome.validate_for(&first)?;
    let encoded = outcome.encode()?;
    let persisted = object_id(OBJECT_KIND, OBJECT_SCHEMA, &encoded);
    assert_eq!(repository.get(persisted)?.bytes, encoded);
    let StepExit::Completed { result, delta, .. } = outcome.exit else {
        return Err("expected completion".into());
    };
    assert_eq!(executor.read_data(result)?, [42]);
    assert!(executor.read_data(delta)?.is_empty());
    let second = StepRequest {
        attempt: ObjectId([7; 32]),
        sample_id: Some(ObjectId([8; 32])),
        ..first
    };
    let next = WorkerBackend::run_step(&executor, second.clone())?;
    next.validate_for(&second)?;
    assert_ne!(next.attempt, outcome.attempt);
    assert_eq!(next.exit, outcome.exit);
    drop(executor);
    drop(repository);
    attow_network::store::Store::open(&path)?.collect_garbage()?;
    assert_eq!(
        SqliteSandboxRepository::open(&path)?.get(persisted)?.bytes,
        encoded
    );
    Ok(())
}

#[test]
fn registered_pure_code_cannot_smuggle_a_managed_delta_or_external_request() -> TestResult {
    let directory = tempfile::tempdir()?;
    let executor = Executor::new(Arc::new(SqliteSandboxRepository::open(
        &directory.path().join("sandbox.sqlite"),
    )?))?;
    let mut request = request(&executor, "continuation.wat")?;
    let code = executor.load_code(request.code)?;
    let profile = ExecutionProfile::new(50_000, 65_536, 1024)?;
    request.continuation = executor.initial_continuation(&code, &profile, &[])?;
    executor.register(admission(&request))?;
    assert_eq!(
        WorkerBackend::run_step(&executor, request),
        Err(ExecutionError::InvalidRequest)
    );
    Ok(())
}

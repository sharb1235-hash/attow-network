//! Original real-component fixtures; no external provider or ambient WASI.
use attow_sandbox::{
    ExecutionProfile, Executor, SandboxError, SandboxRepository, StepExit, StepInput,
};
use chronos_codec::{Limits, OBJECT_KIND, OBJECT_SCHEMA, ObjectId, object_id, references};
use chronos_state::ObjectData;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

// Compile the checked-in WIT as part of the real integration target. The guest
// fixtures use its scalar execution world; records remain the public data model.
mod wit_contract {
    wasmtime::component::bindgen!({ path: "../../wit/attow-v1", world: "step" });
}

#[derive(Default)]
struct MemoryRepository {
    objects: Mutex<BTreeMap<ObjectId, ObjectData>>,
    commits: AtomicUsize,
    fail_next_commit: AtomicBool,
}

impl SandboxRepository for MemoryRepository {
    fn get(&self, id: ObjectId) -> Result<ObjectData, SandboxError> {
        self.objects
            .lock()
            .map_err(|_| SandboxError::Poisoned)?
            .get(&id)
            .cloned()
            .ok_or(SandboxError::MissingObject(id))
    }

    fn commit(
        &self,
        _owner: &str,
        objects: &[ObjectData],
        roots: &[ObjectId],
    ) -> Result<(), SandboxError> {
        if self.fail_next_commit.swap(false, Ordering::SeqCst) {
            return Err(SandboxError::Storage);
        }
        let mut guard = self.objects.lock().map_err(|_| SandboxError::Poisoned)?;
        let mut staged = guard.clone();
        for object in objects {
            staged.insert(
                object_id(object.kind, object.schema, &object.bytes),
                object.clone(),
            );
        }
        for object in objects {
            if (object.kind, object.schema) == (OBJECT_KIND, OBJECT_SCHEMA) {
                for child in references(
                    object.kind,
                    object.schema,
                    &object.bytes,
                    &Limits::default(),
                )? {
                    if !staged.contains_key(&child) {
                        return Err(SandboxError::MissingObject(child));
                    }
                }
            }
        }
        for root in roots {
            if !staged.contains_key(root) {
                return Err(SandboxError::MissingObject(*root));
            }
        }
        *guard = staged;
        self.commits.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

fn fixture(name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/original/wasm")
        .join(name);
    Ok(wat::parse_file(path)?)
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn real_component_arithmetic_returns_canonical_result() -> TestResult {
    let executor = Executor::new(Arc::new(MemoryRepository::default()))?;
    let profile = ExecutionProfile::new(50_000, 65_536, 1024)?;
    let code = executor.admit(&fixture("arithmetic.wat")?)?;
    let input = StepInput {
        code: code.id(),
        state: executor.store_data(&[19])?,
        continuation: executor.initial_continuation(&code, &profile, &[23])?,
        observation: None,
    };
    let StepExit::Done { result, delta } = executor.run_step(&code, input, &profile)? else {
        return Err("arithmetic component unexpectedly suspended".into());
    };
    assert_eq!(executor.read_data(result)?, [42]);
    assert!(executor.read_data(delta)?.is_empty());
    Ok(())
}

fn empty_input(
    executor: &Executor,
    code: &attow_sandbox::AdmittedCode,
    profile: &ExecutionProfile,
) -> Result<StepInput, SandboxError> {
    Ok(StepInput {
        code: code.id(),
        state: executor.store_data(&[])?,
        continuation: executor.initial_continuation(code, profile, &[])?,
        observation: None,
    })
}

fn done(exit: StepExit) -> Result<(ObjectId, ObjectId), Box<dyn std::error::Error>> {
    match exit {
        StepExit::Done { result, delta } => Ok((result, delta)),
        _ => Err("expected completed step".into()),
    }
}

#[test]
fn recorded_inputs_replay_to_identical_hashes() -> TestResult {
    let executor = Executor::new(Arc::new(MemoryRepository::default()))?;
    let profile = ExecutionProfile::new(50_000, 65_536, 1024)?;
    let code = executor.admit(&fixture("arithmetic.wat")?)?;
    let input = StepInput {
        code: code.id(),
        state: executor.store_data(&[19])?,
        continuation: executor.initial_continuation(&code, &profile, &[23])?,
        observation: None,
    };
    let first = done(executor.run_step(&code, input, &profile)?)?;
    let replayed = done(executor.run_step(&code, input, &profile)?)?;
    assert_eq!(first, replayed);
    Ok(())
}

#[test]
fn sibling_resumes_share_checkpoint_without_sharing_guest_state() -> TestResult {
    let executor = Executor::new(Arc::new(MemoryRepository::default()))?;
    let profile = ExecutionProfile::new(50_000, 65_536, 1024)?;
    let code = executor.admit(&fixture("continuation.wat")?)?;
    let state = executor.store_data(&[10])?;
    let initial = StepInput {
        code: code.id(),
        state,
        continuation: executor.initial_continuation(&code, &profile, &[])?,
        observation: None,
    };
    let StepExit::Suspended {
        continuation,
        request,
        delta,
    } = executor.run_step(&code, initial, &profile)?
    else {
        return Err("first step must suspend".into());
    };
    assert_eq!(executor.read_data(request)?, [99]);
    assert_eq!(executor.read_data(delta)?, [3]);
    let first = StepInput {
        code: code.id(),
        state,
        continuation,
        observation: Some(executor.store_data(&[1])?),
    };
    let second = StepInput {
        observation: Some(executor.store_data(&[2])?),
        ..first
    };
    let first_result = done(executor.run_step(&code, first, &profile)?)?;
    let second_result = done(executor.run_step(&code, second, &profile)?)?;
    assert_eq!(executor.read_data(first_result.0)?, [18]);
    assert_eq!(executor.read_data(second_result.0)?, [19]);
    assert_eq!(
        done(executor.run_step(&code, first, &profile)?)?,
        first_result
    );
    Ok(())
}

#[test]
fn unknown_and_ambient_environment_imports_are_denied() -> TestResult {
    let executor = Executor::new(Arc::new(MemoryRepository::default()))?;
    let original = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/original/wasm/arithmetic.wat"),
    )?;
    for forbidden in [
        "unknown-import",
        "filesystem-open",
        "network-send",
        "clock-now",
        "random-byte",
    ] {
        let changed = original.replacen(
            "(import \"input-len\"",
            &format!("(import \"{forbidden}\""),
            1,
        );
        let valid_component = wat::parse_str(changed)?;
        assert!(
            matches!(
                executor.admit(&valid_component),
                Err(SandboxError::Admission)
            ),
            "ambient import {forbidden} must not be admitted"
        );
    }
    Ok(())
}

#[test]
fn malformed_wasm_and_hidden_mutable_globals_are_rejected() -> TestResult {
    let executor = Executor::new(Arc::new(MemoryRepository::default()))?;
    assert!(matches!(
        executor.admit(b"\0asm\x0d"),
        Err(SandboxError::Admission)
    ));
    assert!(matches!(
        executor.admit(&fixture("hidden-global.wat")?),
        Err(SandboxError::Admission)
    ));
    Ok(())
}

#[test]
fn finite_fuel_traps_an_actual_guest_loop() -> TestResult {
    let repository = Arc::new(MemoryRepository::default());
    let executor = Executor::new(repository.clone())?;
    let profile = ExecutionProfile::new(5_000, 65_536, 1024)?;
    let code = executor.admit(&fixture("fuel-loop.wat")?)?;
    let input = empty_input(&executor, &code, &profile)?;
    let commits = repository.commits.load(Ordering::SeqCst);
    assert!(matches!(
        executor.run_step(&code, input, &profile),
        Err(SandboxError::Fuel)
    ));
    assert_eq!(repository.commits.load(Ordering::SeqCst), commits);
    Ok(())
}

#[test]
fn output_bound_is_enforced_before_persisting_partial_result() -> TestResult {
    let repository = Arc::new(MemoryRepository::default());
    let executor = Executor::new(repository.clone())?;
    let profile = ExecutionProfile::new(50_000, 65_536, 4)?;
    let code = executor.admit(&fixture("output-overflow.wat")?)?;
    let input = empty_input(&executor, &code, &profile)?;
    let commits = repository.commits.load(Ordering::SeqCst);
    assert!(matches!(
        executor.run_step(&code, input, &profile),
        Err(SandboxError::Output)
    ));
    assert_eq!(repository.commits.load(Ordering::SeqCst), commits);
    Ok(())
}

#[test]
fn declared_memory_cannot_exceed_the_worker_profile() -> TestResult {
    let executor = Executor::new(Arc::new(MemoryRepository::default()))?;
    let profile = ExecutionProfile::new(50_000, 65_536, 1024)?;
    let code = executor.admit(&fixture("memory-two-pages.wat")?)?;
    let input = empty_input(&executor, &code, &profile)?;
    assert!(matches!(
        executor.run_step(&code, input, &profile),
        Err(SandboxError::Trap | SandboxError::Limit(_))
    ));
    let multiple_memories = executor
        .admit(&fixture("multiple-memories.wat")?)
        .map(|_| ());
    assert!(
        matches!(multiple_memories, Err(SandboxError::Admission)),
        "unexpected multi-memory admission result: {multiple_memories:?}"
    );
    Ok(())
}

#[test]
fn trapped_instance_and_partial_output_are_discarded_before_next_attempt() -> TestResult {
    let repository = Arc::new(MemoryRepository::default());
    let executor = Executor::new(repository.clone())?;
    let profile = ExecutionProfile::new(50_000, 65_536, 1024)?;
    let code = executor.admit(&fixture("conditional-trap.wat")?)?;
    let first = StepInput {
        observation: Some(executor.store_data(&[0])?),
        ..empty_input(&executor, &code, &profile)?
    };
    let commits = repository.commits.load(Ordering::SeqCst);
    assert!(matches!(
        executor.run_step(&code, first, &profile),
        Err(SandboxError::Trap)
    ));
    assert_eq!(repository.commits.load(Ordering::SeqCst), commits);
    let next = StepInput {
        observation: Some(executor.store_data(&[1])?),
        ..first
    };
    let (result, _) = done(executor.run_step(&code, next, &profile)?)?;
    assert_eq!(executor.read_data(result)?, [1]);
    Ok(())
}

#[test]
fn invalid_host_channel_and_non_byte_output_fail_closed() -> TestResult {
    let executor = Executor::new(Arc::new(MemoryRepository::default()))?;
    let profile = ExecutionProfile::new(50_000, 65_536, 1024)?;
    for name in ["invalid-input-channel.wat", "invalid-output-byte.wat"] {
        let code = executor.admit(&fixture(name)?)?;
        let input = empty_input(&executor, &code, &profile)?;
        assert!(matches!(
            executor.run_step(&code, input, &profile),
            Err(SandboxError::InvalidInput)
        ));
    }
    Ok(())
}

#[test]
fn continuation_is_bound_to_its_original_code_and_profile() -> TestResult {
    let executor = Executor::new(Arc::new(MemoryRepository::default()))?;
    let profile = ExecutionProfile::new(50_000, 65_536, 1024)?;
    let code = executor.admit(&fixture("arithmetic.wat")?)?;
    let other = executor.admit(&fixture("continuation.wat")?)?;
    let input = StepInput {
        code: code.id(),
        state: executor.store_data(&[19])?,
        continuation: executor.initial_continuation(&code, &profile, &[23])?,
        observation: None,
    };
    let changed_profile = ExecutionProfile::new(60_000, 65_536, 1024)?;
    assert!(matches!(
        executor.run_step(&code, input, &changed_profile),
        Err(SandboxError::InvalidContinuation)
    ));
    assert!(matches!(
        executor.run_step(
            &other,
            StepInput {
                code: other.id(),
                ..input
            },
            &profile
        ),
        Err(SandboxError::InvalidContinuation)
    ));
    Ok(())
}

#[test]
fn suspension_is_not_returned_until_the_checkpoint_commits() -> TestResult {
    let repository = Arc::new(MemoryRepository::default());
    let executor = Executor::new(repository.clone())?;
    let profile = ExecutionProfile::new(50_000, 65_536, 1024)?;
    let code = executor.admit(&fixture("continuation.wat")?)?;
    let input = StepInput {
        code: code.id(),
        state: executor.store_data(&[10])?,
        continuation: executor.initial_continuation(&code, &profile, &[])?,
        observation: None,
    };
    let committed = repository.commits.load(Ordering::SeqCst);
    let before: Vec<_> = repository
        .objects
        .lock()
        .map_err(|_| SandboxError::Poisoned)?
        .keys()
        .copied()
        .collect();
    repository.fail_next_commit.store(true, Ordering::SeqCst);
    assert!(matches!(
        executor.run_step(&code, input, &profile),
        Err(SandboxError::Storage)
    ));
    assert_eq!(repository.commits.load(Ordering::SeqCst), committed);
    let after: Vec<_> = repository
        .objects
        .lock()
        .map_err(|_| SandboxError::Poisoned)?
        .keys()
        .copied()
        .collect();
    assert_eq!(before, after);
    let StepExit::Suspended {
        continuation,
        request,
        delta,
    } = executor.run_step(&code, input, &profile)?
    else {
        return Err("fresh retry should persist the checkpoint".into());
    };
    assert_eq!(repository.commits.load(Ordering::SeqCst), committed + 1);
    assert_eq!(executor.read_data(request)?, [99]);
    assert_eq!(executor.read_data(delta)?, [3]);
    let resumed = StepInput {
        continuation,
        observation: Some(executor.store_data(&[25])?),
        ..input
    };
    let (result, _) = done(executor.run_step(&code, resumed, &profile)?)?;
    assert_eq!(executor.read_data(result)?, [42]);
    Ok(())
}

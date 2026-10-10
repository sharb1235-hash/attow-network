//! Real retention pins, limits and rollback for scheduler results.
use attow_network::{
    ObjectId,
    codec::{Canonical, OBJECT_KIND, OBJECT_SCHEMA, Object, Value, object_id},
    scheduler::{ResultRepository, SqliteResultRepository},
    state::execution::{
        EffectClass, ResourceBudget, ResourceClass, StepExit, StepOutcome, StepRequest,
    },
    store::Store,
};
use std::{collections::BTreeSet, sync::Arc};
#[path = "support/scheduled_wasm.rs"]
mod scheduled_wasm;

struct FreshStoredOutput {
    path: std::path::PathBuf,
    calls: std::sync::atomic::AtomicUsize,
}
impl attow_network::state::execution::WorkerBackend for FreshStoredOutput {
    fn resource_class(&self) -> ResourceClass {
        ResourceClass::Wasm
    }
    fn validate_request(
        &self,
        _: &StepRequest,
    ) -> Result<(), attow_network::state::execution::ExecutionError> {
        Ok(())
    }
    fn run_step(
        &self,
        request: StepRequest,
    ) -> Result<StepOutcome, attow_network::state::execution::ExecutionError> {
        use attow_network::state::execution::ExecutionError;
        let number = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let bytes = Object {
            refs: BTreeSet::new(),
            payload: Value::Unsigned(number as u64),
        }
        .encode()
        .map_err(|_| ExecutionError::InvalidRequest)?;
        let result = Store::open(&self.path)
            .and_then(|mut store| store.put(OBJECT_KIND, OBJECT_SCHEMA, &bytes))
            .map_err(|_| ExecutionError::BackendUnavailable)?;
        Ok(StepOutcome {
            attempt: request.attempt,
            profile: request.profile,
            exit: StepExit::Completed {
                result,
                delta: request.input_root,
                observation_root: request.observation_root,
            },
        })
    }
}

fn fixture(
    path: &std::path::Path,
) -> Result<(StepRequest, StepOutcome), Box<dyn std::error::Error>> {
    let mut store = Store::open(path)?;
    let id = store.put(
        OBJECT_KIND,
        OBJECT_SCHEMA,
        &Object {
            refs: BTreeSet::new(),
            payload: Value::Null,
        }
        .encode()?,
    )?;
    let request = StepRequest {
        attempt: ObjectId([1; 32]),
        code: id,
        profile: id,
        tenant: ObjectId([2; 32]),
        security_scope: ObjectId([3; 32]),
        input_root: id,
        continuation: id,
        observation_root: id,
        policy: id,
        fidelity: id,
        sample_id: None,
        resource_class: ResourceClass::Wasm,
        effect_class: EffectClass::Pure,
        budget: ResourceBudget {
            fuel: 100,
            memory_bytes: 100,
            output_bytes: 100,
            host_calls: 10,
        },
    };
    let outcome = StepOutcome {
        attempt: request.attempt,
        profile: id,
        exit: StepExit::Completed {
            result: id,
            delta: id,
            observation_root: id,
        },
    };
    Ok((request, outcome))
}

#[test]
fn scheduler_retains_full_request_and_outcome_graph_until_owner_release()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("results.sqlite");
    let (request, outcome) = fixture(&path)?;
    let repository: Arc<dyn ResultRepository> = Arc::new(SqliteResultRepository::open(&path)?);
    let charge = repository.retain("run-one", &request, &outcome, 16 * 1024 * 1024)?;
    assert!(charge >= (request.encode()?.len() + outcome.encode()?.len()) as u64);
    let request_id = object_id(OBJECT_KIND, OBJECT_SCHEMA, &request.encode()?);
    let outcome_id = object_id(OBJECT_KIND, OBJECT_SCHEMA, &outcome.encode()?);
    let mut store = Store::open(&path)?;
    store.collect_garbage()?;
    assert!(store.contains(request_id)?);
    assert!(store.contains(outcome_id)?);
    assert!(store.contains(request.code)?);
    repository.release("run-one")?;
    repository.release("run-one")?;
    store.collect_garbage()?;
    assert!(!store.contains(request_id)?);
    assert!(!store.contains(outcome_id)?);
    assert!(!store.contains(request.code)?);
    Ok(())
}

#[test]
fn invalid_retention_does_not_publish_or_pin_partial_envelopes()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("results.sqlite");
    let (request, mut outcome) = fixture(&path)?;
    let repository = SqliteResultRepository::open(&path)?;
    assert!(
        repository
            .retain("too-small", &request, &outcome, 1)
            .is_err()
    );
    assert!(!Store::open(&path)?.contains(object_id(
        OBJECT_KIND,
        OBJECT_SCHEMA,
        &request.encode()?
    ))?);
    outcome.attempt = ObjectId([99; 32]);
    assert!(
        repository
            .retain("run-one", &request, &outcome, 16 * 1024 * 1024)
            .is_err()
    );
    outcome.attempt = request.attempt;
    outcome.exit = StepExit::Completed {
        result: ObjectId([98; 32]),
        delta: request.code,
        observation_root: request.code,
    };
    assert!(
        repository
            .retain("run-one", &request, &outcome, 16 * 1024 * 1024)
            .is_err()
    );
    let mut store = Store::open(&path)?;
    assert!(!store.contains(object_id(OBJECT_KIND, OBJECT_SCHEMA, &request.encode()?))?);
    store.collect_garbage()?;
    assert!(!store.contains(request.code)?);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_wasm_reuse_is_durable_and_replays_after_workers_are_dropped()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::atomic::Ordering;
    let directory = tempfile::tempdir()?;
    let setup = scheduled_wasm::setup(&directory.path().join("actual.sqlite"))?;
    let run = setup.scheduler.submit(setup.graph)?;
    let report = setup.scheduler.drive(run).await?;
    assert_eq!(report.outcomes.len(), 2);
    assert!(report.failures.is_empty());
    assert_eq!(setup.calls.load(Ordering::SeqCst), 1);
    let inputs = setup.repository.recorded_inputs(&report.trace)?;
    drop(setup.scheduler);
    drop(setup.repository);
    let replay = attow_network::scheduler::replay(&report.trace, &inputs)?;
    assert_eq!(replay.outcomes, report.outcomes);
    assert_eq!(setup.calls.load(Ordering::SeqCst), 1);
    for outcome in replay.outcomes.values() {
        let StepExit::Completed { result, .. } = outcome.exit else {
            return Err("WASM unexpectedly suspended".into());
        };
        let data = &inputs.objects[&result];
        let object = Object::decode(&data.bytes, &attow_network::codec::Limits::default())?;
        let Value::Array(fields) = object.payload else {
            return Err("missing result wrapper".into());
        };
        assert_eq!(fields.last(), Some(&Value::Bytes(vec![42])));
    }
    Ok(())
}

#[tokio::test]
async fn cancelling_queued_work_cannot_revoke_a_dispatching_authority_intent()
-> Result<(), Box<dyn std::error::Error>> {
    use attow_network::{
        ledger::{
            Authority, EffectProposal, EffectState, Policy, PromotionOutcome, PromotionPayload,
            PromotionRequest,
        },
        state::{Worldline, fork},
    };
    let directory = tempfile::tempdir()?;
    let mut authority = Authority::open(&directory.path().join("authority.sqlite"))?;
    let contract = ObjectId([80; 32]);
    let policy = authority.configure_policy(Policy {
        allowed_contracts: BTreeSet::from([contract]),
        max_effect_cost: 10,
        invariants: vec![],
    })?;
    authority.grant_right("budget", 10)?;
    let decision = ObjectId([81; 32]);
    authority.advance_generation(decision, 0)?;
    let mut branch = fork(authority.domain(), &authority.current_root()?, [82; 32])?;
    branch.set_worldline(Worldline {
        policy: Some(policy),
        ..Worldline::default()
    })?;
    let payload = authority.store_payload(&PromotionPayload {
        domain: authority.domain(),
        epoch: authority.epoch(),
        policy,
        effect: Some(EffectProposal {
            contract,
            provider: "owned-fixture".into(),
            semantic_slot: "already-dispatching".into(),
            operation: ObjectId([83; 32]),
            right: "budget".into(),
            cost: 5,
            body: vec![1],
        }),
    })?;
    let PromotionOutcome::AcceptedPending { intent, .. } = authority.promote(PromotionRequest {
        id: ObjectId([84; 32]),
        payload,
        branch: branch.freeze()?,
        decision,
        generation: 0,
    })?
    else {
        return Err("expected authorization".into());
    };
    let ticket = authority.claim_effect(intent)?;
    assert!(ticket.may_dispatch());
    let setup = scheduled_wasm::setup(&directory.path().join("workers.sqlite"))?;
    let run = setup.scheduler.submit(setup.graph)?;
    let cancellation = setup.scheduler.cancel(run).await?;
    assert_eq!(cancellation.effects_revoked, 0);
    assert_eq!(cancellation.cancelled, 2);
    assert_eq!(authority.effect_state(intent)?, EffectState::Dispatching);
    assert_eq!(authority.right_balance("budget")?.reserved, 5);
    drop(authority);
    let recovered = Authority::open(&directory.path().join("authority.sqlite"))?;
    assert_eq!(recovered.effect_state(intent)?, EffectState::Unknown);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn receipt_replacement_keeps_historical_result_objects_replayable_until_forget()
-> Result<(), Box<dyn std::error::Error>> {
    use attow_network::{
        scheduler::{GraphNode, ScheduleLimits, Scheduler, WorldlineGraph},
        state::{BranchId, execution::WorkerBackend},
    };
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("history.sqlite");
    let (mut request, _) = fixture(&path)?;
    request.effect_class = EffectClass::FreshObservation;
    let node = request.attempt;
    let observed = request.observation_root;
    let backend: Arc<dyn WorkerBackend> = Arc::new(FreshStoredOutput {
        path: path.clone(),
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let repository = Arc::new(SqliteResultRepository::open(&path)?);
    let scheduler = Scheduler::new(
        ScheduleLimits {
            resident_workers: 1,
            queued_branches: 8,
            retained_bytes: 1024 * 1024,
        },
        std::collections::BTreeMap::from([(ResourceClass::Wasm, backend)]),
        repository.clone(),
    )?;
    let run = scheduler.submit(WorldlineGraph {
        nodes: vec![GraphNode {
            id: node,
            branch: BranchId([4; 32]),
            request,
            dependencies: vec![],
        }],
    })?;
    let first = scheduler.drive(run).await?;
    let StepExit::Completed {
        result: old_result, ..
    } = first.outcomes[&node].exit
    else {
        return Err("expected result".into());
    };
    scheduler.replace_observation(run, node, observed)?;
    let second = scheduler.drive(run).await?;
    Store::open(&path)?.collect_garbage()?;
    assert!(
        Store::open(&path)?.contains(old_result)?,
        "history output was collected while the trace still referenced it"
    );
    let inputs = repository.recorded_inputs(&second.trace)?;
    assert_eq!(
        attow_network::scheduler::replay(&second.trace, &inputs)?.outcomes,
        second.outcomes
    );
    scheduler.forget(run)?;
    scheduler.clear_cache()?;
    Store::open(&path)?.collect_garbage()?;
    assert!(!Store::open(&path)?.contains(old_result)?);
    Ok(())
}

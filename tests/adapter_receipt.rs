use attow_network::{
    ObjectId,
    adapters::{
        AdapterStore, WorkerBinding,
        gateway::Gateway,
        provider_simulator::{Fault, Purchase, SimulatorConfig, SimulatorServer},
    },
    codec::Value,
    ledger::{
        Authority, EffectProposal, EffectState, Policy, PromotionOutcome, PromotionPayload,
        PromotionRequest,
    },
    scheduler::{
        GraphNode, ResponseBindingError, RunId, ScheduleLimits, Scheduler, SqliteResultRepository,
        WorldlineGraph, rerun_confirmed_receipt,
    },
    state::{
        BranchId, StateRoot, Worldline,
        execution::{
            EffectClass, ExecutionError, ResourceBudget, ResourceClass, StepExit, StepOutcome,
            StepRequest, WorkerBackend,
        },
        fork,
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;

// Original deterministic fixture interpreter, not a browser or WASM measurement.
struct ResponseFixture {
    store: AdapterStore,
    binding: WorkerBinding,
    calls: Arc<Mutex<Vec<(ObjectId, ObjectId)>>>,
    fail_confirmed: bool,
}
impl WorkerBackend for ResponseFixture {
    fn resource_class(&self) -> ResourceClass {
        ResourceClass::Wasm
    }
    fn validate_request(&self, request: &StepRequest) -> Result<(), ExecutionError> {
        self.binding.validate(request, ResourceClass::Wasm)
    }
    fn permits_pure_reuse(&self, _: &StepRequest) -> bool {
        false
    }
    fn run_step(&self, request: StepRequest) -> Result<StepOutcome, ExecutionError> {
        self.validate_request(&request)?;
        self.calls
            .lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?
            .push((request.attempt, request.observation_root));
        let object = self
            .store
            .get_object(request.observation_root)
            .map_err(|_| ExecutionError::InvalidRequest)?;
        let Value::Array(fields) = object.payload else {
            return Err(ExecutionError::InvalidRequest);
        };
        let confirmed = fields.first() == Some(&Value::Text("attow-evidence".into()));
        if confirmed && self.fail_confirmed {
            return Err(ExecutionError::Trapped);
        }
        if !confirmed && fields.first() != Some(&Value::Text("attow-model-observation".into())) {
            return Err(ExecutionError::InvalidRequest);
        }
        let result = self
            .store
            .put_object(
                "response-fixture-results",
                Value::Array(vec![
                    Value::Text(
                        if confirmed {
                            "receipt-backed"
                        } else {
                            "model-only"
                        }
                        .into(),
                    ),
                    Value::Bytes(request.observation_root.0.to_vec()),
                ]),
                BTreeSet::from([request.observation_root]),
            )
            .map_err(|_| ExecutionError::BackendUnavailable)?;
        let delta = self
            .store
            .put_object("response-fixture-empty", Value::Null, BTreeSet::new())
            .map_err(|_| ExecutionError::BackendUnavailable)?;
        Ok(StepOutcome {
            attempt: request.attempt,
            profile: request.profile,
            exit: StepExit::Completed {
                result,
                delta,
                observation_root: request.observation_root,
            },
        })
    }
}
struct Scenario {
    _directory: tempfile::TempDir,
    authority: Authority,
    server: SimulatorServer,
    store: AdapterStore,
    scheduler: Scheduler,
    repository: Arc<SqliteResultRepository>,
    calls: Arc<Mutex<Vec<(ObjectId, ObjectId)>>>,
    run: RunId,
    node: ObjectId,
    leaf: ObjectId,
    intent: ObjectId,
    model: ObjectId,
    policy: ObjectId,
    request: StepRequest,
    branch_base: StateRoot,
}
impl Scenario {
    async fn new(fail_confirmed: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("authority.sqlite");
        let mut authority = Authority::open(&path)?;
        let server = SimulatorServer::start(
            &directory.path().join("provider.sqlite"),
            &SimulatorConfig::default(),
        )?;
        let policy = authority.configure_policy(Policy {
            allowed_contracts: BTreeSet::from([server.contract_id()]),
            max_effect_cost: 1,
            invariants: vec![],
        })?;
        authority.grant_right("budget", 1)?;
        let decision = ObjectId([1; 32]);
        authority.advance_generation(decision, 0)?;
        authority.advance_generation(ObjectId([2; 32]), 0)?;
        let mut purchase = fork(authority.domain(), &authority.current_root()?, [11; 32])?;
        purchase.set_worldline(Worldline {
            policy: Some(policy),
            ..Worldline::default()
        })?;
        let payload = authority.store_payload(&PromotionPayload {
            domain: authority.domain(),
            epoch: authority.epoch(),
            policy,
            effect: Some(EffectProposal {
                contract: server.contract_id(),
                provider: SimulatorConfig::default().provider,
                semantic_slot: "receipt-test-purchase".into(),
                operation: ObjectId([3; 32]),
                right: "budget".into(),
                cost: 1,
                body: Purchase { quantity: 1 }.encode()?,
            }),
        })?;
        let PromotionOutcome::AcceptedPending { intent, .. } =
            authority.promote(PromotionRequest {
                id: ObjectId([4; 32]),
                payload,
                branch: purchase.freeze()?,
                decision,
                generation: 0,
            })?
        else {
            return Err("pending purchase expected".into());
        };
        let store = AdapterStore::open(&path)?;
        let model = store.put_object(
            "response-model",
            Value::Array(vec![
                Value::Text("attow-model-observation".into()),
                Value::Unsigned(1),
                Value::Text("Model".into()),
                Value::Text("prediction: confirmed".into()),
            ]),
            BTreeSet::new(),
        )?;
        let code = store.put_object(
            "response-code",
            Value::Text("original-response-fixture-v1".into()),
            BTreeSet::new(),
        )?;
        let profile = store.put_object(
            "response-profile",
            Value::Text("bounded-model-receipt-interpreter-v1".into()),
            BTreeSet::new(),
        )?;
        let fidelity = store.put_object(
            "response-fidelity",
            Value::Text("RecordedReplay".into()),
            BTreeSet::new(),
        )?;
        let empty = store.put_object("response-empty", Value::Null, BTreeSet::new())?;
        let scope = store.put_object(
            "response-scope",
            Value::Text("owned-response-scope".into()),
            BTreeSet::new(),
        )?;
        let binding = WorkerBinding {
            tenant: ObjectId([6; 32]),
            security_scope: scope,
            policy,
            code,
            profile,
            fidelity,
            effect_class: EffectClass::RecordedObservation,
            budget: ResourceBudget {
                fuel: 10,
                memory_bytes: 1_048_576,
                output_bytes: 4096,
                host_calls: 1,
            },
        };
        let request = StepRequest {
            attempt: ObjectId([8; 32]),
            code,
            profile,
            tenant: binding.tenant,
            security_scope: binding.security_scope,
            input_root: empty,
            continuation: empty,
            observation_root: model,
            policy,
            fidelity,
            sample_id: None,
            resource_class: ResourceClass::Wasm,
            effect_class: binding.effect_class,
            budget: binding.budget,
        };
        let branch_base = authority.current_root()?;
        let branch = fork(authority.domain(), &branch_base, [40; 32])?;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let backend: Arc<dyn WorkerBackend> = Arc::new(ResponseFixture {
            store: store.clone(),
            binding,
            calls: calls.clone(),
            fail_confirmed,
        });
        let repository = Arc::new(SqliteResultRepository::open(&path)?);
        let scheduler = Scheduler::new(
            ScheduleLimits {
                resident_workers: 1,
                queued_branches: 8,
                retained_bytes: 4 * 1024 * 1024,
            },
            BTreeMap::from([(ResourceClass::Wasm, backend)]),
            repository.clone(),
        )?;
        let node = ObjectId([30; 32]);
        let leaf = ObjectId([31; 32]);
        let mut child = request.clone();
        child.attempt = ObjectId([9; 32]);
        let run = scheduler.submit(WorldlineGraph {
            nodes: vec![
                GraphNode {
                    id: node,
                    branch: BranchId([39; 32]),
                    request: request.clone(),
                    dependencies: vec![],
                },
                GraphNode {
                    id: leaf,
                    branch: branch.id(),
                    request: child,
                    dependencies: vec![node],
                },
            ],
        })?;
        let first = scheduler.drive(run).await?;
        assert_eq!(first.outcomes.len(), 2);
        Ok(Self {
            _directory: directory,
            authority,
            server,
            store,
            scheduler,
            repository,
            calls,
            run,
            node,
            leaf,
            intent,
            model,
            policy,
            request,
            branch_base,
        })
    }
    fn followup(
        &mut self,
        observation: ObjectId,
    ) -> Result<PromotionRequest, Box<dyn std::error::Error>> {
        let mut branch = fork(self.authority.domain(), &self.branch_base, [40; 32])?;
        branch.set_worldline(Worldline {
            code: Some(self.request.code),
            profile: Some(self.request.profile),
            security_scope: Some(self.request.security_scope),
            policy: Some(self.policy),
            observation_root: Some(observation),
            ..Worldline::default()
        })?;
        branch.put(b"dependent-confirmed".to_vec(), b"receipt-backed".to_vec())?;
        let payload = self.authority.store_payload(&PromotionPayload {
            domain: self.authority.domain(),
            epoch: self.authority.epoch(),
            policy: self.policy,
            effect: None,
        })?;
        Ok(PromotionRequest {
            id: ObjectId([50; 32]),
            payload,
            branch: branch.freeze()?,
            decision: ObjectId([2; 32]),
            generation: 0,
        })
    }
    fn confirm(&mut self) -> Result<ObjectId, Box<dyn std::error::Error>> {
        let report =
            Gateway::new(&mut self.authority, &mut self.server.connect()).dispatch(self.intent)?;
        assert_eq!(report.state, EffectState::Confirmed);
        Ok(report.receipt)
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn model_reply_requires_real_receipt_and_descendant_rerun_before_dependent_promotion()
-> TestResult {
    let mut setup = Scenario::new(false).await?;
    assert!(matches!(
        rerun_confirmed_receipt(
            &setup.authority,
            &setup.scheduler,
            setup.run,
            setup.node,
            setup.intent
        )
        .await,
        Err(ResponseBindingError::Unconfirmed)
    ));
    setup
        .server
        .controller()
        .set_fault(Fault::CommitThenDisconnect)?;
    let unknown =
        Gateway::new(&mut setup.authority, &mut setup.server.connect()).dispatch(setup.intent)?;
    assert_eq!(unknown.state, EffectState::Unknown);
    assert!(matches!(
        rerun_confirmed_receipt(
            &setup.authority,
            &setup.scheduler,
            setup.run,
            setup.node,
            setup.intent
        )
        .await,
        Err(ResponseBindingError::Unconfirmed)
    ));
    assert_eq!(setup.calls.lock().map_err(|_| "calls")?.len(), 2);
    let confirmed =
        Gateway::new(&mut setup.authority, &mut setup.server.connect()).reconcile(setup.intent)?;
    let response = rerun_confirmed_receipt(
        &setup.authority,
        &setup.scheduler,
        setup.run,
        setup.node,
        setup.intent,
    )
    .await?;
    assert_eq!(response.receipt(), confirmed.receipt);
    assert_ne!(response.receipt(), setup.model);
    assert_eq!(
        setup
            .authority
            .current_root()?
            .get(b"dependent-confirmed")?,
        None
    );
    assert_eq!(setup.calls.lock().map_err(|_| "calls")?.len(), 4);
    for node in [setup.node, setup.leaf] {
        let StepExit::Completed {
            result,
            observation_root,
            ..
        } = response.report().outcomes[&node].exit
        else {
            return Err("dependent completion expected".into());
        };
        assert_eq!(observation_root, confirmed.receipt);
        let Value::Array(fields) = setup.store.get_object(result)?.payload else {
            return Err("result envelope".into());
        };
        assert_eq!(fields.first(), Some(&Value::Text("receipt-backed".into())));
    }
    let records = setup.repository.recorded_inputs(&response.report().trace)?;
    assert_eq!(
        attow_network::scheduler::replay(&response.report().trace, &records)?.outcomes,
        response.report().outcomes
    );
    let request = setup.followup(confirmed.receipt)?;
    assert!(matches!(
        response.promote(&mut setup.authority, request).await?,
        PromotionOutcome::LocalCommitted { .. }
    ));
    assert_eq!(
        setup
            .authority
            .current_root()?
            .get(b"dependent-confirmed")?,
        Some(b"receipt-backed".to_vec())
    );
    assert_eq!(setup.server.controller().stats()?.dispatch_requests, 1);
    setup.authority.rebuild_and_verify()?;
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn incomplete_receipt_rerun_and_model_bound_promotion_are_rejected() -> TestResult {
    let mut failed = Scenario::new(true).await?;
    failed.confirm()?;
    assert!(matches!(
        rerun_confirmed_receipt(
            &failed.authority,
            &failed.scheduler,
            failed.run,
            failed.node,
            failed.intent
        )
        .await,
        Err(ResponseBindingError::Incomplete)
    ));
    assert_eq!(
        failed
            .authority
            .current_root()?
            .get(b"dependent-confirmed")?,
        None
    );
    let mut setup = Scenario::new(false).await?;
    setup.confirm()?;
    let response = rerun_confirmed_receipt(
        &setup.authority,
        &setup.scheduler,
        setup.run,
        setup.node,
        setup.intent,
    )
    .await?;
    let wrong = setup.followup(setup.model)?;
    assert!(matches!(
        response.promote(&mut setup.authority, wrong).await,
        Err(ResponseBindingError::BindingMismatch)
    ));
    assert_eq!(
        setup
            .authority
            .current_root()?
            .get(b"dependent-confirmed")?,
        None
    );
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn later_observation_replacement_invalidates_an_earlier_response_guard() -> TestResult {
    let mut setup = Scenario::new(false).await?;
    let receipt = setup.confirm()?;
    let response = rerun_confirmed_receipt(
        &setup.authority,
        &setup.scheduler,
        setup.run,
        setup.node,
        setup.intent,
    )
    .await?;
    setup
        .scheduler
        .replace_observation(setup.run, setup.node, setup.model)?;
    setup.scheduler.drive(setup.run).await?;
    let request = setup.followup(receipt)?;
    assert!(matches!(
        response.promote(&mut setup.authority, request).await,
        Err(ResponseBindingError::BindingMismatch)
    ));
    assert_eq!(
        setup
            .authority
            .current_root()?
            .get(b"dependent-confirmed")?,
        None
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn response_guard_binds_branch_code_profile_scope_and_policy() -> TestResult {
    let mut setup = Scenario::new(false).await?;
    let receipt = setup.confirm()?;
    for field in 0..5 {
        let response = rerun_confirmed_receipt(
            &setup.authority,
            &setup.scheduler,
            setup.run,
            setup.node,
            setup.intent,
        )
        .await?;
        let mut request = setup.followup(receipt)?;
        let mut worldline = request.branch.worldline().clone();
        match field {
            0 => {
                let mut unrelated = fork(
                    setup.authority.domain(),
                    &setup.authority.current_root()?,
                    [99; 32],
                )?;
                unrelated.set_worldline(worldline)?;
                request.branch = unrelated.freeze()?;
            }
            _ => {
                match field {
                    1 => worldline.code = Some(setup.model),
                    2 => worldline.profile = Some(setup.model),
                    3 => worldline.security_scope = Some(setup.model),
                    _ => worldline.policy = Some(setup.model),
                }
                let mut branch = fork(setup.authority.domain(), &setup.branch_base, [40; 32])?;
                branch.set_worldline(worldline)?;
                request.branch = branch.freeze()?;
            }
        }
        assert!(
            matches!(
                response.promote(&mut setup.authority, request).await,
                Err(ResponseBindingError::BindingMismatch)
            ),
            "unbound field {field}"
        );
    }
    assert_eq!(
        setup
            .authority
            .current_root()?
            .get(b"dependent-confirmed")?,
        None
    );
    Ok(())
}

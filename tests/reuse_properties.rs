//! Independent scheduler reuse, cancellation, and resource-bound properties.
use attow_network::{
    codec::{Canonical, ObjectId, object_id},
    scheduler::{GraphNode, ResultRepository, ScheduleLimits, Scheduler, WorldlineGraph},
    state::{
        BranchId,
        execution::{
            EffectClass, ExecutionError, ResourceBudget, ResourceClass, StepExit, StepOutcome,
            StepRequest, WorkerBackend,
        },
    },
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn id(value: u64) -> ObjectId {
    let mut bytes = [0; 32];
    bytes[..8].copy_from_slice(&value.to_le_bytes());
    ObjectId(bytes)
}

fn request(attempt: u64) -> StepRequest {
    StepRequest {
        attempt: id(attempt),
        code: id(1),
        profile: id(2),
        tenant: id(3),
        security_scope: id(4),
        input_root: id(5),
        continuation: id(6),
        observation_root: id(7),
        policy: id(8),
        fidelity: id(9),
        sample_id: None,
        resource_class: ResourceClass::Wasm,
        effect_class: EffectClass::Pure,
        budget: ResourceBudget {
            fuel: 1000,
            memory_bytes: 65_536,
            output_bytes: 1024,
            host_calls: 20,
        },
    }
}

fn node(number: u64, request: StepRequest, dependencies: Vec<ObjectId>) -> GraphNode {
    GraphNode {
        id: id(number),
        branch: BranchId(id(number + 100_000).0),
        request,
        dependencies,
    }
}

fn pure_outcome(request: &StepRequest) -> Result<StepOutcome, ExecutionError> {
    let mut semantic = request.clone();
    semantic.attempt = id(0);
    let bytes = semantic
        .encode()
        .map_err(|_| ExecutionError::InvalidRequest)?;
    Ok(StepOutcome {
        attempt: request.attempt,
        profile: request.profile,
        exit: StepExit::Completed {
            result: object_id(400, 1, &bytes),
            delta: id(10),
            observation_root: request.observation_root,
        },
    })
}

#[derive(Default)]
struct Repository {
    owners: Mutex<BTreeMap<String, StepOutcome>>,
    fail_next: AtomicBool,
}

impl ResultRepository for Repository {
    fn retain(
        &self,
        owner: &str,
        request: &StepRequest,
        outcome: &StepOutcome,
        max_bytes: u64,
    ) -> Result<u64, ExecutionError> {
        outcome
            .validate_for(request)
            .map_err(|_| ExecutionError::InvalidRequest)?;
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(ExecutionError::BackendUnavailable);
        }
        if max_bytes < 8 {
            return Err(ExecutionError::ResourceExhausted);
        }
        self.owners
            .lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?
            .insert(owner.to_owned(), outcome.clone());
        Ok(8)
    }
    fn release(&self, owner: &str) -> Result<(), ExecutionError> {
        self.owners
            .lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?
            .remove(owner);
        Ok(())
    }
}

struct Backend {
    class: ResourceClass,
    calls: AtomicUsize,
    fail_next: AtomicBool,
    reject: AtomicBool,
    gate: Option<Arc<Gate>>,
    active: Arc<Activity>,
    global: Arc<Activity>,
    validations: AtomicUsize,
    validated: tokio::sync::Notify,
    reuse: AtomicBool,
    suspend: AtomicBool,
    started: Mutex<Vec<ObjectId>>,
}

impl Backend {
    fn new(class: ResourceClass) -> Self {
        Self {
            class,
            calls: AtomicUsize::new(0),
            fail_next: AtomicBool::new(false),
            reject: AtomicBool::new(false),
            gate: None,
            active: Arc::new(Activity::default()),
            global: Arc::new(Activity::default()),
            validations: AtomicUsize::new(0),
            validated: tokio::sync::Notify::new(),
            reuse: AtomicBool::new(true),
            suspend: AtomicBool::new(false),
            started: Mutex::new(Vec::new()),
        }
    }
    async fn validated_at_least(&self, count: usize) -> TestResult {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let notified = self.validated.notified();
                if self.validations.load(Ordering::SeqCst) >= count {
                    break;
                }
                notified.await;
            }
        })
        .await?;
        Ok(())
    }
}

impl WorkerBackend for Backend {
    fn resource_class(&self) -> ResourceClass {
        self.class
    }
    fn validate_request(&self, request: &StepRequest) -> Result<(), ExecutionError> {
        self.validations.fetch_add(1, Ordering::SeqCst);
        self.validated.notify_one();
        if self.reject.load(Ordering::SeqCst) || request.resource_class != self.class {
            return Err(ExecutionError::InvalidRequest);
        }
        request
            .budget
            .validate()
            .map_err(|_| ExecutionError::InvalidRequest)
    }
    fn permits_pure_reuse(&self, _request: &StepRequest) -> bool {
        self.reuse.load(Ordering::SeqCst)
    }
    fn run_step(&self, request: StepRequest) -> Result<StepOutcome, ExecutionError> {
        self.validate_request(&request)?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.started
            .lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?
            .push(request.tenant);
        let _class = self.active.enter();
        let _global = self.global.enter();
        if let Some(gate) = &self.gate {
            gate.wait()?;
        }
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(ExecutionError::Trapped);
        }
        if self.suspend.load(Ordering::SeqCst)
            && request.effect_class == EffectClass::RecordedObservation
        {
            return Ok(StepOutcome {
                attempt: request.attempt,
                profile: request.profile,
                exit: StepExit::Suspended {
                    continuation: id(600),
                    request: id(601),
                    delta: id(602),
                    observation_root: request.observation_root,
                },
            });
        }
        pure_outcome(&request)
    }
}

#[derive(Default)]
struct Activity {
    active: AtomicUsize,
    peak: AtomicUsize,
}
impl Activity {
    fn enter(&self) -> Active<'_> {
        let count = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(count, Ordering::SeqCst);
        Active(self)
    }
}
struct Active<'a>(&'a Activity);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    wake: Condvar,
    arrivals: AtomicUsize,
    arrived: tokio::sync::Notify,
}
impl Gate {
    fn wait(&self) -> Result<(), ExecutionError> {
        let guard = self
            .open
            .lock()
            .map_err(|_| ExecutionError::BackendUnavailable)?;
        self.arrivals.fetch_add(1, Ordering::SeqCst);
        self.arrived.notify_one();
        // This watchdog bounds failures; successful tests synchronize on notifications, never delays.
        let (open, timeout) = self
            .wake
            .wait_timeout_while(guard, Duration::from_secs(10), |open| !*open)
            .map_err(|_| ExecutionError::BackendUnavailable)?;
        if !*open || timeout.timed_out() {
            return Err(ExecutionError::Interrupted);
        }
        Ok(())
    }
    async fn entered(&self, count: usize) -> TestResult {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let notified = self.arrived.notified();
                if self.arrivals.load(Ordering::SeqCst) >= count {
                    break;
                }
                notified.await;
            }
        })
        .await?;
        Ok(())
    }
    fn release(&self) -> TestResult {
        *self.open.lock().map_err(|_| "gate poisoned")? = true;
        self.wake.notify_all();
        Ok(())
    }
}

fn scheduler(
    backend: Arc<Backend>,
    repository: Arc<Repository>,
) -> Result<Scheduler, attow_network::scheduler::ScheduleError> {
    let registry = BTreeMap::from([(ResourceClass::Wasm, backend as Arc<dyn WorkerBackend>)]);
    Scheduler::new(
        ScheduleLimits {
            resident_workers: 2,
            queued_branches: 128,
            retained_bytes: 1_000_000,
        },
        registry,
        repository,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identical_pure_work_executes_once_without_merging_branch_or_attempt_identity() -> TestResult
{
    let backend = Arc::new(Backend::new(ResourceClass::Wasm));
    let scheduler = scheduler(backend.clone(), Arc::new(Repository::default()))?;
    let graph = WorldlineGraph {
        nodes: vec![
            node(100, request(20), vec![]),
            node(101, request(21), vec![]),
        ],
    };
    let run = scheduler.submit(graph)?;
    let report = scheduler.drive(run).await?;
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    assert_eq!(report.outcomes.len(), 2);
    assert_eq!(report.outcomes[&id(100)].attempt, id(20));
    assert_eq!(report.outcomes[&id(101)].attempt, id(21));
    assert_eq!(
        report.outcomes[&id(100)].exit,
        report.outcomes[&id(101)].exit
    );
    assert_ne!(
        report.trace.graph.nodes[0].branch,
        report.trace.graph.nodes[1].branch
    );
    assert!(report.failures.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trapped_work_is_incomplete_and_not_reused() -> TestResult {
    let backend = Arc::new(Backend::new(ResourceClass::Wasm));
    backend.fail_next.store(true, Ordering::SeqCst);
    let scheduler = scheduler(backend.clone(), Arc::new(Repository::default()))?;
    let run = scheduler.submit(WorldlineGraph {
        nodes: vec![node(100, request(20), vec![])],
    })?;
    let failed = scheduler.drive(run).await?;
    assert_eq!(failed.failures[&id(100)], ExecutionError::Trapped);
    assert!(failed.outcomes.is_empty());
    let retry = scheduler.submit(WorldlineGraph {
        nodes: vec![node(101, request(21), vec![])],
    })?;
    assert_eq!(scheduler.drive(retry).await?.outcomes.len(), 1);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_security_semantic_sample_and_budget_change_reexecutes() -> TestResult {
    let wasm = Arc::new(Backend::new(ResourceClass::Wasm));
    let browser = Arc::new(Backend::new(ResourceClass::Browser));
    let registry = BTreeMap::from([
        (ResourceClass::Wasm, wasm.clone() as Arc<dyn WorkerBackend>),
        (
            ResourceClass::Browser,
            browser.clone() as Arc<dyn WorkerBackend>,
        ),
    ]);
    let scheduler = Scheduler::new(
        ScheduleLimits {
            resident_workers: 2,
            queued_branches: 128,
            retained_bytes: 1_000_000,
        },
        registry,
        Arc::new(Repository::default()),
    )?;
    let base = request(20);
    let run = scheduler.submit(WorldlineGraph {
        nodes: vec![node(100, base.clone(), vec![])],
    })?;
    assert_eq!(
        scheduler.drive(run).await?.outcomes[&id(100)],
        pure_outcome(&base)?
    );
    for field in 0..16 {
        let mut changed = request(30 + field);
        match field {
            0 => changed.code = id(999),
            1 => changed.profile = id(999),
            2 => changed.tenant = id(999),
            3 => changed.security_scope = id(999),
            4 => changed.input_root = id(999),
            5 => changed.continuation = id(999),
            6 => changed.observation_root = id(999),
            7 => changed.policy = id(999),
            8 => changed.fidelity = id(999),
            9 => changed.sample_id = Some(id(999)),
            10 => changed.sample_id = Some(id(998)),
            11 => changed.resource_class = ResourceClass::Browser,
            12 => changed.budget.fuel += 1,
            13 => changed.budget.memory_bytes += 1,
            14 => changed.budget.output_bytes += 1,
            _ => changed.budget.host_calls += 1,
        }
        let number = 200 + field;
        let run = scheduler.submit(WorldlineGraph {
            nodes: vec![node(number, changed.clone(), vec![])],
        })?;
        assert_eq!(
            scheduler.drive(run).await?.outcomes[&id(number)],
            pure_outcome(&changed)?,
            "field {field}"
        );
        assert_eq!(
            wasm.calls.load(Ordering::SeqCst) + browser.calls.load(Ordering::SeqCst),
            field as usize + 2,
            "field {field}"
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn non_pure_classes_never_share_or_hit_pure_cache() -> TestResult {
    for effect in [
        EffectClass::ManagedRead,
        EffectClass::ManagedWrite,
        EffectClass::RecordedObservation,
        EffectClass::FreshObservation,
        EffectClass::PreparedExternal,
        EffectClass::IrreversibleExternal,
    ] {
        let backend = Arc::new(Backend::new(ResourceClass::Wasm));
        let scheduler = scheduler(backend.clone(), Arc::new(Repository::default()))?;
        for attempt in 0..3 {
            let mut request = request(20 + attempt);
            request.effect_class = effect;
            let run = scheduler.submit(WorldlineGraph {
                nodes: vec![node(100 + attempt, request, vec![])],
            })?;
            assert_eq!(scheduler.drive(run).await?.outcomes.len(), 1);
        }
        assert_eq!(backend.calls.load(Ordering::SeqCst), 3, "effect {effect:?}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cached_result_still_requires_current_backend_admission() -> TestResult {
    let backend = Arc::new(Backend::new(ResourceClass::Wasm));
    let scheduler = scheduler(backend.clone(), Arc::new(Repository::default()))?;
    let first = scheduler.submit(WorldlineGraph {
        nodes: vec![node(100, request(20), vec![])],
    })?;
    scheduler.drive(first).await?;
    backend.reject.store(true, Ordering::SeqCst);
    match scheduler.submit(WorldlineGraph {
        nodes: vec![node(101, request(21), vec![])],
    }) {
        Err(attow_network::scheduler::ScheduleError::Backend(ExecutionError::InvalidRequest)) => {}
        Ok(run) => {
            let report = scheduler.drive(run).await?;
            assert!(report.outcomes.is_empty());
            assert_eq!(report.failures[&id(101)], ExecutionError::InvalidRequest);
        }
        other => panic!("unexpected admission result: {other:?}"),
    }
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_retention_cannot_publish_a_completed_or_cached_result() -> TestResult {
    let backend = Arc::new(Backend::new(ResourceClass::Wasm));
    let repository = Arc::new(Repository::default());
    repository.fail_next.store(true, Ordering::SeqCst);
    let scheduler = scheduler(backend.clone(), repository.clone())?;
    let run = scheduler.submit(WorldlineGraph {
        nodes: vec![node(100, request(20), vec![])],
    })?;
    let report = scheduler.drive(run).await?;
    assert!(report.outcomes.is_empty());
    assert_eq!(
        report.failures[&id(100)],
        ExecutionError::BackendUnavailable
    );
    assert!(
        repository
            .owners
            .lock()
            .map_err(|_| "owners poisoned")?
            .is_empty()
    );
    let retry = scheduler.submit(WorldlineGraph {
        nodes: vec![node(101, request(21), vec![])],
    })?;
    assert_eq!(scheduler.drive(retry).await?.outcomes.len(), 1);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_cancelled_consumer_does_not_corrupt_cross_run_single_flight() -> TestResult {
    let gate = Arc::new(Gate::default());
    let mut backend = Backend::new(ResourceClass::Wasm);
    backend.gate = Some(gate.clone());
    let backend = Arc::new(backend);
    let scheduler = Arc::new(scheduler(backend.clone(), Arc::new(Repository::default()))?);
    let first = scheduler.submit(WorldlineGraph {
        nodes: vec![node(100, request(20), vec![])],
    })?;
    let drive = {
        let scheduler = scheduler.clone();
        tokio::spawn(async move { scheduler.drive(first).await })
    };
    gate.entered(1).await?;
    let validations = backend.validations.load(Ordering::SeqCst);
    let second = scheduler.submit(WorldlineGraph {
        nodes: vec![node(101, request(21), vec![])],
    })?;
    let second_drive = {
        let scheduler = scheduler.clone();
        tokio::spawn(async move { scheduler.drive(second).await })
    };
    backend.validated_at_least(validations + 1).await?;
    // Admission occurs under the scheduler lock; acquiring it establishes that
    // the already-running evaluation has attached the second consumer.
    scheduler.metrics()?;
    let cancelled = scheduler.cancel(first).await?;
    assert_eq!(cancelled.effects_revoked, 0);
    assert_eq!(cancelled.cancelled, 1);
    gate.release()?;
    let survivor = second_drive.await??;
    assert_eq!(survivor.outcomes[&id(101)].attempt, id(21));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    assert!(drive.await??.cancelled.contains(&id(100)));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_queue_releases_capacity_and_dropped_drive_keeps_resident_bound() -> TestResult {
    let gate = Arc::new(Gate::default());
    let mut backend = Backend::new(ResourceClass::Wasm);
    backend.gate = Some(gate.clone());
    let backend = Arc::new(backend);
    let scheduler = Arc::new(Scheduler::new(
        ScheduleLimits {
            resident_workers: 1,
            queued_branches: 2,
            retained_bytes: 1_000_000,
        },
        BTreeMap::from([(
            ResourceClass::Wasm,
            backend.clone() as Arc<dyn WorkerBackend>,
        )]),
        Arc::new(Repository::default()),
    )?);
    let first = scheduler.submit(WorldlineGraph {
        nodes: vec![node(100, request(20), vec![])],
    })?;
    let drive = {
        let scheduler = scheduler.clone();
        tokio::spawn(async move { scheduler.drive(first).await })
    };
    gate.entered(1).await?;
    drive.abort();
    let _ = drive.await;
    let mut queued_request = request(21);
    queued_request.input_root = id(555);
    let queued = scheduler.submit(WorldlineGraph {
        nodes: vec![node(101, queued_request, vec![])],
    })?;
    assert_eq!(scheduler.cancel(queued).await?.cancelled, 1);
    assert_eq!(scheduler.cancel(first).await?.effects_revoked, 0);
    let mut later_request = request(22);
    later_request.input_root = id(556);
    let later = scheduler.submit(WorldlineGraph {
        nodes: vec![node(102, later_request, vec![])],
    })?;
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    assert_eq!(backend.active.active.load(Ordering::SeqCst), 1);
    gate.release()?;
    let report = scheduler.drive(later).await?;
    assert_eq!(report.outcomes.len(), 1);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    assert_eq!(backend.active.peak.load(Ordering::SeqCst), 1);
    assert!(scheduler.drive(queued).await?.outcomes.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn global_and_resource_class_limits_bound_real_simultaneous_workers() -> TestResult {
    let gate = Arc::new(Gate::default());
    let global = Arc::new(Activity::default());
    let mut registry = BTreeMap::new();
    let mut backends = Vec::new();
    let mut nodes = Vec::new();
    for (index, class) in [
        ResourceClass::Wasm,
        ResourceClass::Browser,
        ResourceClass::Sql,
    ]
    .into_iter()
    .enumerate()
    {
        let mut backend = Backend::new(class);
        backend.gate = Some(gate.clone());
        backend.global = global.clone();
        let backend = Arc::new(backend);
        registry.insert(class, backend.clone() as Arc<dyn WorkerBackend>);
        backends.push(backend);
        for offset in 0..2 {
            let number = (index * 2 + offset) as u64;
            let mut request = request(20 + number);
            request.resource_class = class;
            request.input_root = id(1000 + number);
            nodes.push(node(100 + number, request, vec![]));
        }
    }
    let scheduler = Arc::new(Scheduler::new(
        ScheduleLimits {
            resident_workers: 2,
            queued_branches: 16,
            retained_bytes: 1_000_000,
        },
        registry,
        Arc::new(Repository::default()),
    )?);
    for class in [
        ResourceClass::Wasm,
        ResourceClass::Browser,
        ResourceClass::Sql,
    ] {
        scheduler.set_resource_limit(class, 1)?;
    }
    let run = scheduler.submit(WorldlineGraph { nodes })?;
    let drive = {
        let scheduler = scheduler.clone();
        tokio::spawn(async move { scheduler.drive(run).await })
    };
    gate.entered(2).await?;
    assert_eq!(global.active.load(Ordering::SeqCst), 2);
    gate.release()?;
    assert_eq!(drive.await??.outcomes.len(), 6);
    assert_eq!(global.peak.load(Ordering::SeqCst), 2);
    for backend in backends {
        assert_eq!(backend.active.peak.load(Ordering::SeqCst), 1);
        assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replacing_a_receipt_reruns_only_the_affected_subtree() -> TestResult {
    let backend = Arc::new(Backend::new(ResourceClass::Wasm));
    let scheduler = scheduler(backend.clone(), Arc::new(Repository::default()))?;
    let nodes = (0..4)
        .map(|number| {
            let mut request = request(20 + number);
            request.input_root = id(1000 + number);
            node(
                100 + number,
                request,
                if number == 1 || number == 2 {
                    vec![id(99 + number)]
                } else {
                    vec![]
                },
            )
        })
        .collect();
    let run = scheduler.submit(WorldlineGraph { nodes })?;
    let original = scheduler.drive(run).await?;
    assert_eq!(original.outcomes.len(), 4);
    let invalidated = scheduler.replace_observation(run, id(100), id(777))?;
    assert_eq!(invalidated, vec![id(100), id(101), id(102)]);
    let revised = scheduler.drive(run).await?;
    assert_eq!(revised.outcomes.len(), 4);
    for affected in [100, 101, 102] {
        assert_ne!(
            revised.outcomes[&id(affected)],
            original.outcomes[&id(affected)]
        );
    }
    assert_eq!(revised.outcomes[&id(103)], original.outcomes[&id(103)]);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 7);
    assert_eq!(revised.trace.graph, original.trace.graph);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cycles_and_missing_dependencies_are_rejected_before_execution() -> TestResult {
    let backend = Arc::new(Backend::new(ResourceClass::Wasm));
    let scheduler = scheduler(backend.clone(), Arc::new(Repository::default()))?;
    for nodes in [
        vec![
            node(100, request(20), vec![id(101)]),
            node(101, request(21), vec![id(100)]),
        ],
        vec![node(100, request(20), vec![id(999)])],
        vec![node(100, request(20), vec![id(100)])],
    ] {
        assert!(scheduler.submit(WorldlineGraph { nodes }).is_err());
    }
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    Ok(())
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(32))]
    #[test]
    fn optimized_and_unoptimized_generated_dags_have_identical_branch_outputs(groups in proptest::collection::vec(0_u8..5, 1..17), edges in proptest::collection::vec(proptest::bool::ANY, 16)) {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("test runtime");
        runtime.block_on(async {
            let nodes: Vec<_> = groups.iter().enumerate().map(|(index, group)| {
                let mut request = request(20 + index as u64); request.input_root = id(1000 + u64::from(*group));
                node(100 + index as u64, request, if index > 0 && edges[index] { vec![id(99 + index as u64)] } else { vec![] })
            }).collect();
            let graph = WorldlineGraph { nodes };
            let optimized_backend = Arc::new(Backend::new(ResourceClass::Wasm));
            let baseline_backend = Arc::new(Backend::new(ResourceClass::Wasm));
            let optimized = scheduler(optimized_backend.clone(), Arc::new(Repository::default())).expect("optimized scheduler");
            let baseline = scheduler(baseline_backend.clone(), Arc::new(Repository::default())).expect("baseline scheduler");
            baseline.set_reuse_enabled(false).expect("disable reuse");
            let left = optimized.drive(optimized.submit(graph.clone()).expect("valid graph")).await.expect("optimized run");
            let right = baseline.drive(baseline.submit(graph).expect("valid graph")).await.expect("baseline run");
            proptest::prop_assert_eq!(left.outcomes, right.outcomes);
            proptest::prop_assert!(left.failures.is_empty() && right.failures.is_empty());
            proptest::prop_assert_eq!(baseline_backend.calls.load(Ordering::SeqCst), groups.len());
            proptest::prop_assert!(optimized_backend.calls.load(Ordering::SeqCst) <= baseline_backend.calls.load(Ordering::SeqCst));
            Ok(())
        })?;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backend_reuse_denial_overrides_a_caller_pure_label() -> TestResult {
    let backend = Arc::new(Backend::new(ResourceClass::Wasm));
    backend.reuse.store(false, Ordering::SeqCst);
    let scheduler = scheduler(backend.clone(), Arc::new(Repository::default()))?;
    let run = scheduler.submit(WorldlineGraph {
        nodes: vec![
            node(100, request(20), vec![]),
            node(101, request(21), vec![]),
        ],
    })?;
    assert_eq!(scheduler.drive(run).await?.outcomes.len(), 2);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn awaiting_a_receipt_parks_descendants_without_retaining_a_worker() -> TestResult {
    let backend = Arc::new(Backend::new(ResourceClass::Wasm));
    backend.suspend.store(true, Ordering::SeqCst);
    let scheduler = scheduler(backend.clone(), Arc::new(Repository::default()))?;
    let mut observation = request(20);
    observation.effect_class = EffectClass::RecordedObservation;
    let run = scheduler.submit(WorldlineGraph {
        nodes: vec![
            node(100, observation, vec![]),
            node(101, request(21), vec![id(100)]),
            node(102, request(22), vec![]),
        ],
    })?;
    let report = scheduler.drive(run).await?;
    assert_eq!(report.outcomes.len(), 2);
    assert_eq!(report.pending, std::collections::BTreeSet::from([id(101)]));
    assert!(matches!(
        report.outcomes[&id(100)].exit,
        StepExit::Suspended { .. }
    ));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    assert_eq!(backend.active.active.load(Ordering::SeqCst), 0);
    assert!(report.failures.is_empty());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ready_tenants_alternate_before_one_tenant_drains_its_queue() -> TestResult {
    let gate = Arc::new(Gate::default());
    let mut backend = Backend::new(ResourceClass::Wasm);
    backend.gate = Some(gate.clone());
    let backend = Arc::new(backend);
    let scheduler = Arc::new(Scheduler::new(
        ScheduleLimits {
            resident_workers: 1,
            queued_branches: 16,
            retained_bytes: 1_000_000,
        },
        BTreeMap::from([(
            ResourceClass::Wasm,
            backend.clone() as Arc<dyn WorkerBackend>,
        )]),
        Arc::new(Repository::default()),
    )?);
    let nodes = (0..4)
        .map(|number| {
            let mut request = request(20 + number);
            request.input_root = id(2000 + number);
            request.tenant = id(if number == 3 { 52 } else { 51 });
            node(100 + number, request, vec![])
        })
        .collect();
    let run = scheduler.submit(WorldlineGraph { nodes })?;
    let drive = {
        let scheduler = scheduler.clone();
        tokio::spawn(async move { scheduler.drive(run).await })
    };
    gate.entered(1).await?;
    gate.release()?;
    assert_eq!(drive.await??.outcomes.len(), 4);
    assert_eq!(
        *backend.started.lock().map_err(|_| "started poisoned")?,
        vec![id(51), id(52), id(51), id(51)]
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forgetting_a_run_and_clearing_cache_release_all_retention_owners() -> TestResult {
    let backend = Arc::new(Backend::new(ResourceClass::Wasm));
    let repository = Arc::new(Repository::default());
    let scheduler = scheduler(backend, repository.clone())?;
    let run = scheduler.submit(WorldlineGraph {
        nodes: vec![node(100, request(20), vec![])],
    })?;
    scheduler.drive(run).await?;
    assert!(
        !repository
            .owners
            .lock()
            .map_err(|_| "owners poisoned")?
            .is_empty()
    );
    scheduler.forget(run)?;
    scheduler.clear_cache()?;
    assert!(
        repository
            .owners
            .lock()
            .map_err(|_| "owners poisoned")?
            .is_empty()
    );
    assert_eq!(scheduler.metrics()?.retained_bytes, 0);
    assert!(matches!(
        scheduler.drive(run).await,
        Err(attow_network::scheduler::ScheduleError::UnknownRun)
    ));
    Ok(())
}
